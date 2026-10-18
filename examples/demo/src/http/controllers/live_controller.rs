//! The live page's stream, its reset and its echo socket (routes in
//! `crate::routes::admin`).

use rustcms::auth::{EventStream, LiveSession, LiveSocket, SocketMessage, SocketResponse};
use rustcms::prelude::*;
use rustcms::realtime::{Event, StreamExt, stream};

use crate::services::ticker::Ticker;

/// `GET /admin/api/x/demo/ticks`: the counter, as server-sent events.
pub async fn ticks(live: LiveSession, Inject(ticker): Inject<Ticker>) -> AppResult<EventStream> {
    let events = ticker.subscribe()?.into_events();
    // The current value first, then each tick.
    let first = stream::once(async { Event::json("hello", &json!({})).expect("small") });
    Ok(live.stream(first.chain(events)))
}

/// `POST /admin/api/x/demo/ticks/reset`.
pub async fn reset(Inject(ticker): Inject<Ticker>) -> StatusCode {
    ticker.reset();
    StatusCode::NO_CONTENT
}

/// `GET /admin/api/x/demo/echo` (WebSocket): sends back what it gets;
/// `whoami` is a command, authorized when it runs.
pub async fn echo(live: LiveSocket) -> SocketResponse {
    live.accept(|mut socket| async move {
        while let Some(message) = socket.recv().await {
            let reply = match message {
                SocketMessage::Text(text) if text == "whoami" => match socket.authorize().await {
                    Ok(principal) => SocketMessage::Text(format!("{}:{}", principal.guard, principal.id)),
                    Err(_) => break,
                },
                other => other,
            };
            if socket.send(reply).await.is_err() {
                break;
            }
        }
    })
}

/// `GET /admin/api/x/demo/hello`: read by the React demo page (`useAdminQuery`).
pub async fn hello() -> Json<serde_json::Value> {
    Json(json!({ "message": "Hello from AdminRoutes" }))
}
