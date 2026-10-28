//! Server-sent events on the admin session (ADR 0027): declared with
//! [`AdminRoutes::events`](crate::AdminRoutes::events), admitted without a
//! CSRF token (a stream changes nothing, and other origins cannot read it),
//! rechecked while open, bounded per instance, ended by the server's shutdown.
//!
//! ```rust,ignore
//! async fn stats(live: LiveSession, Path(id): Path<String>, Inject(hub): Inject<Stats>) -> AppResult<EventStream> {
//!     let target = id.clone();
//!     let subscription = hub.0.subscribe(id.clone(), move || docker_stats(id)).map_err(|_| AppError::TooManyRequests { retry_after: 5 })?;
//!     Ok(live
//!         .stream(subscription.into_events())
//!         .target(move |principal| { let target = target.clone(); async move { may_see(&principal, &target) } }))
//! }
//! ```

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use futures_util::{Stream, StreamExt};
use rustcms_core::realtime::{Event, Shutdown};
use rustcms_core::{AppError, AppResult, AppState, ClientIp, Principal};
use sea_orm::EntityTrait;
use serde::Deserialize;
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc};

use crate::guard::Authenticated;
use crate::http::admin_routes::AdminAccess;
use crate::session::{self, Session};
use crate::state::{ADMIN_GUARD, AuthState};
use rustcms_core::models::user;

pub(crate) mod socket;
pub use socket::{AdminSocket, LiveSocket, SendError, SocketClosed, SocketMessage, SocketResponse};

/// `[realtime]` (`config/realtime.toml`), per instance.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct RealtimeConfig {
    /// Seconds between the rechecks of an open stream (at least 1; 10 or more
    /// in production: each recheck reads the session, the user and the grants).
    pub revalidate_secs: u64,
    /// Open streams on this instance.
    pub max_connections: usize,
    /// Open streams of one user on this instance.
    pub max_per_user: usize,
    /// Streams a user may open per minute.
    pub opens_per_user_per_minute: u32,
    /// Streams one IP may open per minute (shared IPs share it).
    pub opens_per_ip_per_minute: u32,
    /// Grace given to streams when the server stops (read by `serve`).
    pub shutdown_grace_secs: u64,
    /// Seconds between the rechecks of an open WebSocket (at least 1).
    pub socket_revalidate_secs: u64,
    /// Largest WebSocket message, either way (bytes).
    pub max_message_bytes: usize,
    /// Longest wait for a WebSocket send (queue room, then the network).
    pub send_timeout_secs: u64,
}

impl Default for RealtimeConfig {
    fn default() -> Self {
        Self {
            revalidate_secs: 30,
            max_connections: 1000,
            max_per_user: 10,
            opens_per_user_per_minute: 60,
            opens_per_ip_per_minute: 300,
            shutdown_grace_secs: 5,
            socket_revalidate_secs: 10,
            max_message_bytes: 64 * 1024,
            send_timeout_secs: 10,
        }
    }
}

/// Events queued for one stream before it is ended as lagging.
const QUEUE_EVENTS: usize = 64;
/// Bytes queued for one stream before it is ended as lagging.
const QUEUE_BYTES: usize = 1024 * 1024;
/// Longest recheck.
pub(crate) const CHECK_TIMEOUT: Duration = Duration::from_secs(5);
/// Comment sent when nothing else was.
const KEEP_ALIVE: Duration = Duration::from_secs(15);

/// Open streams of this instance (bound by the auth provider).
pub struct Realtime {
    pub(crate) config: RealtimeConfig,
    total: Arc<Semaphore>,
    users: Mutex<HashMap<i64, Arc<Semaphore>>>,
}

impl Realtime {
    pub(crate) fn new(mut config: RealtimeConfig) -> Self {
        config.revalidate_secs = config.revalidate_secs.max(1);
        config.socket_revalidate_secs = config.socket_revalidate_secs.max(1);
        config.send_timeout_secs = config.send_timeout_secs.max(1);
        config.max_message_bytes = config.max_message_bytes.clamp(1, QUEUE_BYTES);
        Self { total: Arc::new(Semaphore::new(config.max_connections)), users: Mutex::default(), config }
    }

    /// Both permits, or `None` when this instance or this user is at the limit.
    fn acquire(self: &Arc<Self>, user_id: i64) -> Option<Permits> {
        let total = self.total.clone().try_acquire_owned().ok()?;
        let semaphore = {
            let mut users = self.users.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            users.entry(user_id).or_insert_with(|| Arc::new(Semaphore::new(self.config.max_per_user))).clone()
        };
        let user = semaphore.try_acquire_owned().ok();
        let permits = Permits { user, total: Some(total), realtime: self.clone(), user_id };
        permits.user.is_some().then_some(permits)
    }

    /// Largest WebSocket message, either way: `[realtime] max_message_bytes`
    /// as applied (1 byte to 1 MiB). The admin panel sends it to the browser,
    /// which refuses to send larger messages.
    pub fn max_message_bytes(&self) -> usize {
        self.config.max_message_bytes
    }

    /// Open streams of `user_id` (tests).
    pub fn open_for(&self, user_id: i64) -> usize {
        let users = self.users.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        users.get(&user_id).map_or(0, |s| self.config.max_per_user - s.available_permits())
    }
}

/// Held by an open stream; released on every path when dropped.
pub(crate) struct Permits {
    user: Option<OwnedSemaphorePermit>,
    total: Option<OwnedSemaphorePermit>,
    realtime: Arc<Realtime>,
    user_id: i64,
}

impl Drop for Permits {
    fn drop(&mut self) {
        self.user.take();
        self.total.take();
        // The user's semaphore goes once nothing holds it (no two for one user).
        let mut users = self.realtime.users.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(semaphore) = users.get(&self.user_id)
            && Arc::strong_count(semaphore) == 1
            && semaphore.available_permits() == self.realtime.config.max_per_user
        {
            users.remove(&self.user_id);
        }
    }
}

/// Admission of a stream: no CSRF token, but only from the panel's own
/// origin. An `Origin`, when sent, must be an allowed one (exactly, not
/// `null`); `Sec-Fetch-Site`, when sent, must be `same-origin`; a request with
/// neither (an old browser) needs a same-origin `Referer`. A repeated or
/// unreadable header is refused.
pub(crate) fn check_streaming_origin(auth: &AuthState, headers: &HeaderMap) -> AppResult<()> {
    let single = |name: header::HeaderName| -> AppResult<Option<String>> {
        let mut values = headers.get_all(name).iter();
        let first = values.next();
        if values.next().is_some() {
            return Err(AppError::Forbidden);
        }
        first.map(|v| v.to_str().map(str::to_owned).map_err(|_| AppError::Forbidden)).transpose()
    };
    let allowed = |origin: &str| auth.allowed_origins.iter().any(|a| a == origin);
    // All three are well-formed, whichever decides.
    let origin = single(header::ORIGIN)?;
    let site = single(header::HeaderName::from_static("sec-fetch-site"))?;
    let referer = single(header::REFERER)?;
    if let Some(origin) = &origin {
        let exact = crate::config::origin_of(origin).filter(|o| *o == origin.to_ascii_lowercase());
        if !exact.is_some_and(|o| allowed(&o)) {
            return Err(AppError::Forbidden);
        }
    }
    if let Some(site) = &site
        && site != "same-origin"
    {
        return Err(AppError::Forbidden);
    }
    if origin.is_none() && site.is_none() {
        let referer = referer.and_then(|r| crate::config::origin_of(&r));
        if !referer.is_some_and(|o| allowed(&o)) {
            return Err(AppError::Forbidden);
        }
    }
    Ok(())
}

/// An open admin stream's identity (extractor; only in
/// [`AdminRoutes::events`](crate::AdminRoutes::events) handlers). Its route
/// admitted the request before any extractor ran: the panel's own origin
/// (403), the open limits, the stream's place (429), the server not stopping
/// (503). Turn it into the response with [`stream`](Self::stream).
pub struct LiveSession {
    live: Live,
}

/// What an admitted stream or socket knows of its user, and its place.
pub(crate) struct Live {
    state: AppState,
    auth: Arc<AuthState>,
    realtime: Arc<Realtime>,
    shutdown: Arc<Shutdown>,
    session_id: String,
    user_id: i64,
    auth_version: i64,
    access: AdminAccess,
    principal: Principal,
    /// Shared with the response body: the place is held until both end.
    pub(crate) permits: Arc<Permits>,
}

/// The admitted stream, waiting for its handler's `LiveSession`.
#[derive(Clone)]
struct Admission(Arc<Mutex<Option<LiveSession>>>);

/// Which origin rule admits a request.
#[derive(Clone, Copy)]
pub(crate) enum OriginRule {
    /// Streams: Origin or Fetch Metadata, a same-origin `Referer` without either.
    Stream,
    /// Sockets: a mandatory exact `Origin` (browsers always send one).
    Socket,
}

/// Admits a stream request (the route layer, before the handler's
/// extractors). A handler that takes no `LiveSession` releases it with the
/// request.
pub(crate) async fn admit(req: &mut axum::extract::Request, access: &AdminAccess) -> Result<(), Box<Response>> {
    // `&mut`: a shared `&Request` held across an await is not `Send`.
    let live = admission(req, access, OriginRule::Stream).await?;
    req.extensions_mut().insert(Admission(Arc::new(Mutex::new(Some(LiveSession { live })))));
    Ok(())
}

/// The checks shared by streams and sockets: the caller, the origin, the
/// server not stopping, the open limits and a place.
pub(crate) async fn admission(
    req: &mut axum::extract::Request,
    access: &AdminAccess,
    rule: OriginRule,
) -> Result<Live, Box<Response>> {
    let fail = |e: AppError| Box::new(e.into_response());
    let app =
        req.extensions().get::<AppState>().cloned().ok_or_else(|| fail(AppError::internal("admin guard missing")))?;
    let caller = req
        .extensions()
        .get::<Authenticated>()
        .filter(|c| c.principal.guard == ADMIN_GUARD)
        .cloned()
        .ok_or_else(|| fail(AppError::Unauthenticated))?;
    let session_id = req
        .extensions()
        .get::<Session>()
        .and_then(Session::stored_id)
        .ok_or_else(|| fail(AppError::Unauthenticated))?;
    let auth = app.service::<AuthState>().ok_or_else(|| fail(AppError::internal("AuthProvider not registered")))?;
    let realtime = app.service::<Realtime>().ok_or_else(|| fail(AppError::internal("AuthProvider not registered")))?;
    let shutdown = app.service::<Shutdown>().ok_or_else(|| fail(AppError::internal("no Shutdown bound")))?;
    match rule {
        OriginRule::Stream => check_streaming_origin(&auth, req.headers()),
        OriginRule::Socket => socket::check_socket_origin(&auth, req.headers()),
    }
    .map_err(fail)?;
    if !shutdown.is_open() {
        return Err(Box::new(unavailable()));
    }
    let ip = req.extensions().get::<ClientIp>().map(|c| c.0.to_string()).unwrap_or_default();
    let config = &realtime.config;
    auth.limiter
        .reserve(&[
            (&format!("realtime:open:{}", caller.user.id), config.opens_per_user_per_minute),
            (&format!("realtime:open-ip:{ip}"), config.opens_per_ip_per_minute),
        ])
        .await
        .map_err(fail)?;
    let permits = realtime.acquire(caller.user.id).ok_or_else(|| fail(AppError::TooManyRequests { retry_after: 5 }))?;
    Ok(Live {
        state: app.clone(),
        auth,
        realtime,
        shutdown,
        session_id,
        user_id: caller.user.id,
        auth_version: caller.user.auth_version,
        access: access.clone(),
        principal: caller.principal,
        permits: Arc::new(permits),
    })
}

impl FromRequestParts<AppState> for LiveSession {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, _: &AppState) -> Result<Self, Response> {
        let admission = parts.extensions.get::<Admission>().cloned();
        let live = admission.and_then(|a| a.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take());
        live.ok_or_else(|| {
            AppError::internal("LiveSession outside AdminRoutes::events (or taken twice)").into_response()
        })
    }
}

pub(crate) fn unavailable() -> Response {
    let body = serde_json::json!({"message": "The server is stopping.", "code": "unavailable"});
    (StatusCode::SERVICE_UNAVAILABLE, axum::Json(body)).into_response()
}

/// Why a stream ended early (sent as `event: closed`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Closed {
    SignedOut,
    Forbidden,
    Shutdown,
    Lagging,
    Error,
}

impl Closed {
    pub(crate) fn reason(self) -> &'static str {
        match self {
            Self::SignedOut => "signed_out",
            Self::Forbidden => "forbidden",
            Self::Shutdown => "shutdown",
            Self::Lagging => "lagging",
            Self::Error => "error",
        }
    }
}

/// What a target check returns, boxed: for a check written once and reused
/// by several streams.
///
/// ```rust,ignore
/// fn sees_container(docker: Docker, id: String) -> impl Fn(Principal) -> TargetFuture + Send + Sync + 'static {
///     move |principal| {
///         let (docker, id) = (docker.clone(), id.clone());
///         Box::pin(async move { docker.authorize(&principal, &id).await })
///     }
/// }
/// // live.stream(events).target(sees_container(docker, id))
/// ```
pub type TargetFuture = Pin<Box<dyn Future<Output = AppResult<()>> + Send>>;

pub(crate) type TargetCheck = Arc<dyn Fn(Principal) -> TargetFuture + Send + Sync>;

impl LiveSession {
    /// The user's principal when the stream was opened.
    pub fn principal(&self) -> &Principal {
        &self.live.principal
    }

    /// The response streaming `events`, rechecked while open.
    pub fn stream<S>(self, events: S) -> EventStream
    where
        S: Stream<Item = Event> + Send + 'static,
    {
        EventStream { live: self.live, events: events.boxed(), target: None }
    }
}

impl Live {
    pub(crate) fn principal(&self) -> &Principal {
        &self.principal
    }

    pub(crate) fn config(&self) -> &RealtimeConfig {
        &self.realtime.config
    }

    pub(crate) fn shutdown(&self) -> &Arc<Shutdown> {
        &self.shutdown
    }

    /// Whether the stream may go on: the same session, user and credential
    /// version, and a fresh principal passing the route's access and `target`
    /// (returned).
    async fn check(&self, target: Option<&TargetCheck>) -> Result<Principal, Closed> {
        let db = self.state.db();
        let signed_in = session::still_signed_in(db, &self.session_id, self.user_id, self.auth_version)
            .await
            .map_err(|_| Closed::Error)?;
        if !signed_in {
            return Err(Closed::SignedOut);
        }
        let user = user::Entity::find_by_id(self.user_id).one(db).await.map_err(|_| Closed::Error)?;
        let Some(user) = user.filter(|u| u.is_active && u.auth_version == self.auth_version) else {
            return Err(Closed::SignedOut);
        };
        let principal = self.auth.resolver.principal(db, ADMIN_GUARD, &user, None).await.map_err(|_| Closed::Error)?;
        if !self.access.allows(&principal) {
            return Err(Closed::Forbidden);
        }
        if let Some(target) = target {
            match target(principal.clone()).await {
                Ok(()) => {}
                Err(AppError::Forbidden | AppError::NotFound | AppError::Unauthenticated) => {
                    return Err(Closed::Forbidden);
                }
                Err(_) => return Err(Closed::Error),
            }
        }
        Ok(principal)
    }

    pub(crate) async fn check_in_time(&self, target: Option<&TargetCheck>) -> Result<Principal, Closed> {
        tokio::time::timeout(CHECK_TIMEOUT, self.check(target)).await.unwrap_or(Err(Closed::Error))
    }
}

/// A stream of events for one admin page (see [`LiveSession::stream`]).
pub struct EventStream {
    live: Live,
    events: futures_util::stream::BoxStream<'static, Event>,
    target: Option<TargetCheck>,
}

impl EventStream {
    /// Checks, before the first event and at every recheck, that the user may
    /// still see this stream's target (a container, a site): `Err(Forbidden)`
    /// or `Err(NotFound)` ends it (`closed: forbidden`).
    pub fn target<F, Fut>(mut self, check: F) -> Self
    where
        F: Fn(Principal) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = AppResult<()>> + Send + 'static,
    {
        self.target = Some(Arc::new(move |principal| Box::pin(check(principal))));
        self
    }
}

/// One queued SSE item and its size.
struct Queued(SseEvent, usize);

impl IntoResponse for EventStream {
    fn into_response(self) -> Response {
        let (tx, mut rx) = mpsc::channel::<Queued>(QUEUE_EVENTS);
        let queued = Arc::new(AtomicUsize::new(0));
        let shutdown = self.live.shutdown.clone();
        let permits = self.live.permits.clone();
        let pump = pump(self, tx, queued.clone());
        if shutdown.spawn(pump).is_none() {
            return unavailable();
        }
        // The body only reads the queue: a client that stops reading never
        // blocks the pump, its rechecks or its cancellation. It holds the
        // stream's place until it is dropped too.
        let body = futures_util::stream::poll_fn(move |cx| {
            let _held = &permits;
            rx.poll_recv(cx).map(|item| {
                item.map(|Queued(event, size)| {
                    queued.fetch_sub(size, Ordering::SeqCst);
                    Ok::<_, std::convert::Infallible>(event)
                })
            })
        });
        Sse::new(body).keep_alive(KeepAlive::new().interval(KEEP_ALIVE)).into_response()
    }
}

/// Forwards the events into the queue while the stream is allowed, until it
/// ends, the client leaves, a recheck fails or the server stops.
async fn pump(stream: EventStream, tx: mpsc::Sender<Queued>, queued: Arc<AtomicUsize>) {
    let EventStream { live, mut events, target } = stream;
    let token = live.shutdown.token();
    let every = Duration::from_secs(live.realtime.config.revalidate_secs);
    // Rechecks are spread out: streams opened together do not check together.
    let offset = every.mul_f64(rand::random_range(0.5..1.0));
    let mut recheck = tokio::time::interval_at(tokio::time::Instant::now() + offset, every);
    recheck.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let closed = |reason: Closed| {
        let data = serde_json::json!({"reason": reason.reason()}).to_string();
        Queued(SseEvent::default().event("closed").data(data), 0)
    };
    // The target may already be out of reach.
    let first = tokio::select! {
        checked = live.check_in_time(target.as_ref()) => checked,
        () = tx.closed() => return,
    };
    if let Err(reason) = first.map(drop) {
        let _ = tx.try_send(closed(reason));
        return;
    }
    let ended = loop {
        tokio::select! {
            biased;
            () = token.cancelled() => break Some(Closed::Shutdown),
            // The client left (its body was dropped): stop at once.
            () = tx.closed() => break None,
            _ = recheck.tick() => {
                tokio::select! {
                    checked = live.check_in_time(target.as_ref()) => {
                        if let Err(reason) = checked.map(drop) {
                            break Some(reason);
                        }
                    }
                    () = tx.closed() => break None,
                }
            }
            event = events.next() => {
                let Some(event) = event else { break None };
                let size = event.data().len() + event.name().len();
                if queued.load(Ordering::SeqCst) + size > QUEUE_BYTES {
                    break Some(Closed::Lagging);
                }
                let item = Queued(SseEvent::default().event(event.name()).data(event.data()), size);
                queued.fetch_add(size, Ordering::SeqCst);
                match tx.try_send(item) {
                    Ok(()) => {}
                    Err(mpsc::error::TrySendError::Full(_)) => {
                        queued.fetch_sub(size, Ordering::SeqCst);
                        break Some(Closed::Lagging);
                    }
                    Err(mpsc::error::TrySendError::Closed(_)) => break None,
                }
            }
        }
    };
    // Best effort: delivery has stopped either way, and the permits go with `live`.
    if let Some(reason) = ended {
        let _ = tx.try_send(closed(reason));
    }
    drop(live);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_message_limit_is_read_as_applied() {
        let with = |bytes| Realtime::new(RealtimeConfig { max_message_bytes: bytes, ..RealtimeConfig::default() });
        assert_eq!(Realtime::new(RealtimeConfig::default()).max_message_bytes(), 64 * 1024);
        assert_eq!(with(1000).max_message_bytes(), 1000);
        assert_eq!(with(0).max_message_bytes(), 1);
        assert_eq!(with(usize::MAX).max_message_bytes(), 1024 * 1024);
    }
}
