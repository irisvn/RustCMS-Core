//! A counter that ticks every second while someone watches it: the live page's
//! stream (`/admin/p/live`), one producer for every viewer through the hub.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use rustcms::realtime::{Event, Hub, HubLimits, Shutdown, Subscription, stream, time};
use rustcms::{AppError, AppResult};

/// The only key: everyone sees the same counter.
const KEY: &str = "ticks";

pub struct Ticker {
    hub: Hub<&'static str>,
    count: Arc<AtomicU64>,
}

impl Ticker {
    pub fn new(shutdown: &Shutdown) -> Self {
        Self { hub: Hub::new(shutdown, HubLimits::default()), count: Arc::new(AtomicU64::new(0)) }
    }

    /// The counter's events, starting the ticking for the first viewer.
    pub fn subscribe(&self) -> AppResult<Subscription<&'static str>> {
        let count = self.count.clone();
        self.hub
            .subscribe(KEY, move || {
                stream::unfold(count, |count| async move {
                    time::sleep(Duration::from_secs(1)).await;
                    let value = count.fetch_add(1, Ordering::SeqCst) + 1;
                    Some((tick(value), count))
                })
            })
            .map_err(|_| AppError::TooManyRequests { retry_after: 5 })
    }

    /// Back to zero, shown at once to every viewer.
    pub fn reset(&self) {
        self.count.store(0, Ordering::SeqCst);
        self.hub.publish(&KEY, tick(0));
    }
}

fn tick(count: u64) -> Event {
    Event::json("tick", &serde_json::json!({ "count": count })).expect("a small event")
}
