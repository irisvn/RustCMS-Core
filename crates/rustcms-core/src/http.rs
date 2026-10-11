//! HTTP stack: middleware, JSON extractor, client IP, health check.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::extract::rejection::JsonRejection;
use axum::extract::{ConnectInfo, DefaultBodyLimit, FromRequest, Request, State};
use axum::http::{HeaderName, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use http_body_util::Limited;
use ipnet::IpNet;
use serde::Serialize;
use serde::de::DeserializeOwned;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, RequestId, SetRequestIdLayer};
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;

use crate::app::{AppState, RouteFn};
use crate::error::AppError;

const REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");

/// JSON body extractor and response, with rustcms errors:
/// an oversized body is a 413, malformed JSON a 400, a shape mismatch a 422.
#[derive(Debug, Clone, Copy, Default)]
pub struct Json<T>(pub T);

impl<T: DeserializeOwned, S: Send + Sync> FromRequest<S> for Json<T> {
    type Rejection = AppError;

    async fn from_request(req: Request, state: &S) -> Result<Self, AppError> {
        match axum::Json::<T>::from_request(req, state).await {
            Ok(axum::Json(value)) => Ok(Self(value)),
            Err(rejection) => Err(json_rejection(rejection)),
        }
    }
}

fn json_rejection(rejection: JsonRejection) -> AppError {
    match rejection.status() {
        StatusCode::PAYLOAD_TOO_LARGE => AppError::PayloadTooLarge,
        StatusCode::UNPROCESSABLE_ENTITY => AppError::Validation(crate::ValidationErrors {
            form_errors: vec![rejection.body_text()],
            ..Default::default()
        }),
        _ => AppError::BadRequest(rejection.body_text()),
    }
}

impl<T: Serialize> IntoResponse for Json<T> {
    fn into_response(self) -> Response {
        axum::Json(self.0).into_response()
    }
}

/// IP address of the client, resolved through trusted proxies. Inserted for every request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientIp(pub IpAddr);

impl<S: Send + Sync> axum::extract::FromRequestParts<S> for ClientIp {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut axum::http::request::Parts, _: &S) -> Result<Self, AppError> {
        parts.extensions.get::<ClientIp>().copied().ok_or_else(|| AppError::internal("ClientIp middleware missing"))
    }
}

/// Resolves the client address.
///
/// `X-Forwarded-For` is only read when the socket peer is a trusted proxy; the
/// client is then the right-most address that is not itself a trusted proxy,
/// so values injected by the client on the left are ignored.
pub fn resolve_client_ip<'a>(
    peer: IpAddr,
    forwarded_for: impl IntoIterator<Item = &'a str>,
    trusted: &[IpNet],
) -> IpAddr {
    let is_trusted = |ip: &IpAddr| trusted.iter().any(|net| net.contains(ip));
    if !is_trusted(&peer) {
        return peer;
    }
    let mut chain = Vec::new();
    for value in forwarded_for {
        for part in value.split(',') {
            match part.trim().parse::<IpAddr>() {
                Ok(ip) => chain.push(ip),
                Err(_) => return peer,
            }
        }
    }
    chain.iter().rev().find(|ip| !is_trusted(ip)).or(chain.first()).copied().unwrap_or(peer)
}

/// Caps every request body at `limit` bytes, whatever reads it.
///
/// A declared `Content-Length` above the limit is rejected before the handler
/// runs; other bodies (chunked) are wrapped so reading past the limit fails.
/// Any 413 produced downstream (e.g. by axum's `Bytes` extractor) is rendered
/// in the standard JSON error format.
async fn limit_body(State(limit): State<usize>, req: Request, next: Next) -> Response {
    let declared = req.headers().get(header::CONTENT_LENGTH).and_then(|v| v.to_str().ok()?.parse::<u64>().ok());
    if declared.is_some_and(|len| len > limit as u64) {
        return AppError::PayloadTooLarge.into_response();
    }
    let req = req.map(|body| Body::new(Limited::new(body, limit)));
    let response = next.run(req).await;
    let is_json =
        response.headers().get(header::CONTENT_TYPE).is_some_and(|v| v.as_bytes().starts_with(b"application/json"));
    if response.status() == StatusCode::PAYLOAD_TOO_LARGE && !is_json {
        return AppError::PayloadTooLarge.into_response();
    }
    response
}

async fn client_ip(State(state): State<AppState>, mut req: Request, next: Next) -> Response {
    let peer =
        req.extensions().get::<ConnectInfo<SocketAddr>>().map_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED), |c| c.0.ip());
    let forwarded = req.headers().get_all("x-forwarded-for").iter().filter_map(|v| v.to_str().ok());
    let ip = resolve_client_ip(peer, forwarded, &state.config().http.trusted_proxies);
    req.extensions_mut().insert(ClientIp(ip));
    next.run(req).await
}

#[derive(Serialize)]
struct Health {
    status: &'static str,
    database: &'static str,
}

async fn health(State(state): State<AppState>) -> (StatusCode, Json<Health>) {
    if crate::db::ping(state.db()).await {
        (StatusCode::OK, Json(Health { status: "ok", database: "ok" }))
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, Json(Health { status: "degraded", database: "unavailable" }))
    }
}

pub(crate) fn router(state: AppState, routes: Vec<RouteFn>) -> Router {
    let http = state.config().http.clone();
    let mut router = Router::new().route("/health", get(health));
    for add in routes {
        router = add(router);
    }
    router
        .fallback(|| async { AppError::NotFound })
        .method_not_allowed_fallback(|| async { AppError::MethodNotAllowed })
        .layer(DefaultBodyLimit::max(http.body_limit))
        .layer(middleware::from_fn_with_state(http.body_limit, limit_body))
        .layer(TimeoutLayer::with_status_code(StatusCode::REQUEST_TIMEOUT, Duration::from_secs(http.timeout_secs)))
        .layer(middleware::from_fn_with_state(state.clone(), client_ip))
        .layer(PropagateRequestIdLayer::new(REQUEST_ID))
        .layer(TraceLayer::new_for_http().make_span_with(|req: &Request| {
            let id = req.extensions().get::<RequestId>().and_then(|id| id.header_value().to_str().ok()).unwrap_or("-");
            tracing::info_span!("request", method = %req.method(), path = %req.uri().path(), request_id = %id)
        }))
        .layer(SetRequestIdLayer::new(REQUEST_ID, MakeRequestUuid))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nets(list: &[&str]) -> Vec<IpNet> {
        list.iter().map(|n| n.parse().unwrap()).collect()
    }

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn untrusted_peer_ignores_forwarded_for() {
        let trusted = nets(&["10.0.0.0/8"]);
        assert_eq!(resolve_client_ip(ip("203.0.113.9"), ["1.2.3.4"], &trusted), ip("203.0.113.9"));
        assert_eq!(resolve_client_ip(ip("203.0.113.9"), ["1.2.3.4"], &[]), ip("203.0.113.9"));
    }

    #[test]
    fn trusted_proxy_yields_rightmost_untrusted_hop() {
        let trusted = nets(&["10.0.0.0/8"]);
        // The client spoofed 6.6.6.6; the proxy appended the real address.
        assert_eq!(resolve_client_ip(ip("10.0.0.2"), ["6.6.6.6, 198.51.100.7"], &trusted), ip("198.51.100.7"));
        assert_eq!(
            resolve_client_ip(ip("10.0.0.2"), ["6.6.6.6", "198.51.100.7, 10.0.0.5"], &trusted),
            ip("198.51.100.7")
        );
    }

    #[test]
    fn malformed_or_missing_header_falls_back_to_peer() {
        let trusted = nets(&["10.0.0.0/8"]);
        assert_eq!(resolve_client_ip(ip("10.0.0.2"), ["nonsense"], &trusted), ip("10.0.0.2"));
        assert_eq!(resolve_client_ip(ip("10.0.0.2"), [], &trusted), ip("10.0.0.2"));
    }
}
