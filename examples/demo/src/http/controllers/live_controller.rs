//! The live page's stream and its reset (routes in `crate::routes::admin`).

use rustcms::auth::{EventStream, LiveSession};
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
