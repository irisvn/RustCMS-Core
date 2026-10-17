//! A counter that ticks every second while someone watches it: the live page's
//! stream (`/admin/p/live`), one producer for every viewer through the hub.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use rustcms::realtime::{Event, Hub, HubLimits, Shutdown, StreamExt, Subscription, stream, time};
use rustcms::{AppError, AppResult};

/// The only key: everyone sees the same counter.
const KEY: &str = "ticks";

pub struct Ticker {
    hub: Hub<&'static str>,
    /// Changed and published under this lock, by ticks and resets alike: a
    /// tick never shows after a reset it preceded.
    count: Arc<Mutex<u64>>,
}

impl Ticker {
    pub fn new(shutdown: Arc<Shutdown>) -> Self {
        Self { hub: Hub::new(shutdown, HubLimits::default()), count: Arc::new(Mutex::new(0)) }
    }

    /// The counter's events, starting the ticking for the first viewer.
    pub fn subscribe(&self) -> AppResult<Subscription<&'static str>> {
        let (count, hub) = (self.count.clone(), self.hub.clone());
        self.hub
            .subscribe(KEY, move || {
                // The producer publishes each tick itself (under the lock) and yields nothing.
                stream::unfold((count, hub), |(count, hub)| async move {
                    time::sleep(Duration::from_secs(1)).await;
                    {
                        let mut value = count.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                        *value += 1;
                        hub.publish(&KEY, tick(*value));
                    }
                    Some(((), (count, hub)))
                })
                .filter_map(|()| async { None })
            })
            .map_err(|_| AppError::TooManyRequests { retry_after: 5 })
    }

    /// Back to zero, shown at once to every viewer.
    pub fn reset(&self) {
        let mut value = self.count.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        *value = 0;
        self.hub.publish(&KEY, tick(0));
    }
}

fn tick(count: u64) -> Event {
    Event::json("tick", &serde_json::json!({ "count": count })).expect("a small event")
}
