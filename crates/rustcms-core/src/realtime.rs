//! Long-lived work beside HTTP: the shutdown of streams, server-sent events
//! and a process-local hub fanning one producer out to many subscribers
//! (ADR 0027, ADR 0028).

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::FutureExt;
use serde::Serialize;
use tokio::sync::broadcast;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::AppError;

/// Stream helpers and timers for producers and handlers (`stream::unfold`,
/// `.chain`, `time::sleep`, `time::interval`), so applications need no
/// crate of their own for them.
pub use futures_util::{Stream, StreamExt, stream};
pub use tokio::time;

/// Largest serialized event (its JSON data).
pub const MAX_EVENT_BYTES: usize = 64 * 1024;

/// How long aborted tasks are waited for after the grace.
const ABORT_WAIT: Duration = Duration::from_secs(1);

/// The end of long-lived tasks (streams, hub producers) when the server
/// stops: admission closes, the token is cancelled, tasks get a grace period,
/// then whatever remains is aborted. Bound in every app.
pub struct Shutdown {
    token: CancellationToken,
    open: AtomicBool,
    tasks: Mutex<JoinSet<()>>,
}

impl Default for Shutdown {
    fn default() -> Self {
        Self { token: CancellationToken::new(), open: AtomicBool::new(true), tasks: Mutex::new(JoinSet::new()) }
    }
}

impl Shutdown {
    /// Cancelled when the server stops.
    pub fn token(&self) -> CancellationToken {
        self.token.clone()
    }

    /// Whether new long-lived work may start (false once stopping).
    pub fn is_open(&self) -> bool {
        self.open.load(Ordering::SeqCst)
    }

    /// Runs `task` until it ends or the server stops (then it gets the grace
    /// period, and is aborted after it). `None` once stopping.
    pub fn spawn<F>(&self, task: F) -> Option<tokio::task::AbortHandle>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let mut tasks = self.tasks.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if !self.is_open() {
            return None;
        }
        // Finished tasks are reaped as new ones start.
        while tasks.try_join_next().is_some() {}
        Some(tasks.spawn(task))
    }

    /// Stops admission and cancels the token.
    pub fn begin(&self) {
        let _tasks = self.tasks.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        self.open.store(false, Ordering::SeqCst);
        self.token.cancel();
    }

    /// Waits up to `grace` for the tasks, then aborts the rest. Returns
    /// whether they all ended on their own.
    pub async fn drain(&self, grace: Duration) -> bool {
        let mut tasks = std::mem::take(&mut *self.tasks.lock().unwrap_or_else(std::sync::PoisonError::into_inner));
        let ended = tokio::time::timeout(grace, async { while tasks.join_next().await.is_some() {} }).await.is_ok();
        if !ended {
            tracing::warn!(left = tasks.len(), "long-lived tasks did not stop in time: aborted");
            tasks.abort_all();
            // Abort is cooperative: a task stuck in a poll is left behind (dropped with the set).
            let _ = tokio::time::timeout(ABORT_WAIT, async { while tasks.join_next().await.is_some() {} }).await;
        }
        ended
    }
}

/// One server-sent event: a name and JSON data, at most [`MAX_EVENT_BYTES`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    name: String,
    data: Arc<str>,
}

impl Event {
    /// `value` serialized as the event `name` (`[a-z0-9_.-]`, at most 64).
    ///
    /// # Errors
    /// A bad name, a value that does not serialize, or one larger than
    /// [`MAX_EVENT_BYTES`]; the error never quotes the value.
    pub fn json(name: &str, value: &impl Serialize) -> Result<Self, AppError> {
        let valid = !name.is_empty()
            && name.len() <= 64
            && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_.-".contains(&b));
        if !valid {
            return Err(AppError::internal(format!("invalid event name {name:?}")));
        }
        let data = serde_json::to_string(value).map_err(|e| AppError::internal(format!("event {name}: {e}")))?;
        if data.len() > MAX_EVENT_BYTES {
            return Err(AppError::internal(format!("event {name} is larger than {MAX_EVENT_BYTES} bytes")));
        }
        Ok(Self { name: name.to_owned(), data: data.into() })
    }

    /// `event: resync`: the page reloads what it shows. The hub sends it to a
    /// subscriber that fell behind; a producer yields it when its own source
    /// lost events (an upstream broadcast that lagged, a reconnect).
    pub fn resync() -> Self {
        Self { name: "resync".to_owned(), data: "{}".into() }
    }

    /// The event's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The event's JSON data (one line).
    pub fn data(&self) -> &str {
        &self.data
    }
}

/// What a hub subscription receives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HubEvent {
    /// An event of the key.
    Item(Event),
    /// This many events were missed: reload the state.
    Lagged(u64),
    /// The producer ended (or failed): nothing more comes.
    Ended,
}

/// More keys are active than the hub allows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("too many live streams")]
pub struct HubFull;

/// Hub bounds.
#[derive(Debug, Clone, Copy)]
pub struct HubLimits {
    /// Keys with a producer or subscribers at once.
    pub max_keys: usize,
    /// Events buffered per key (a slower subscriber gets `Lagged`).
    pub capacity: usize,
    /// How long a producer keeps running after its last subscriber leaves.
    pub idle_grace: Duration,
}

impl Default for HubLimits {
    fn default() -> Self {
        Self { max_keys: 1000, capacity: 64, idle_grace: Duration::from_secs(5) }
    }
}

/// Set once a producer task has ended, however it ended (completion, panic,
/// abort): the key's next producer waits for it.
#[derive(Default)]
struct Finished {
    done: AtomicBool,
    notify: tokio::sync::Notify,
}

impl Finished {
    fn is_done(&self) -> bool {
        self.done.load(Ordering::SeqCst)
    }

    async fn wait(&self) {
        loop {
            let notified = self.notify.notified();
            if self.is_done() {
                return;
            }
            notified.await;
        }
    }
}

/// Marks [`Finished`] when dropped (the producer future ending or being aborted).
struct MarkFinished(Arc<Finished>);

impl Drop for MarkFinished {
    fn drop(&mut self) {
        self.0.done.store(true, Ordering::SeqCst);
        self.0.notify.notify_waiters();
    }
}

struct Entry {
    /// Unique per entry: a timer or a producer of an older entry never touches it.
    id: u64,
    /// Advanced by every subscribe: an idle timer armed before is stale.
    activity: u64,
    subscribers: usize,
    tx: broadcast::Sender<HubEvent>,
    producer: Option<tokio::task::AbortHandle>,
    finished: Arc<Finished>,
}

struct HubState<K> {
    entries: HashMap<K, Entry>,
    /// Producers aborted and not yet finished: a new producer of the key waits
    /// for its predecessor, and they count against `max_keys` meanwhile.
    stopping: HashMap<K, Arc<Finished>>,
    next_id: u64,
}

/// Fans one producer per key out to its subscribers, inside this process
/// (instances each run their own). The first subscriber of a key starts its
/// producer; it stops `idle_grace` after the last one leaves. Producers run
/// under [`Shutdown`] and stop with it.
///
/// The hub knows no users: a key must hold everything that decides its data
/// (`container:<id>`), and access is checked before subscribing.
pub struct Hub<K> {
    state: Arc<Mutex<HubState<K>>>,
    limits: HubLimits,
    shutdown: Arc<Shutdown>,
}

impl<K> Clone for Hub<K> {
    fn clone(&self) -> Self {
        Self { state: self.state.clone(), limits: self.limits, shutdown: self.shutdown.clone() }
    }
}

impl<K: Eq + Hash + Clone + Send + Sync + 'static> Hub<K> {
    /// A hub whose producers stop with `shutdown` (`AppBuilder::shutdown()`).
    pub fn new(shutdown: Arc<Shutdown>, limits: HubLimits) -> Self {
        Self {
            state: Arc::new(Mutex::new(HubState { entries: HashMap::new(), stopping: HashMap::new(), next_id: 0 })),
            limits,
            shutdown,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HubState<K>> {
        self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Subscribes to `key`, starting its producer if it has none. `make` runs
    /// in the producer's task, outside the hub's lock, once the key's previous
    /// producer has finished; a panic in it ends the subscribers (`Ended`).
    ///
    /// # Errors
    /// [`HubFull`] when `max_keys` keys are active, or the server is stopping.
    pub fn subscribe<S, F>(&self, key: K, make: F) -> Result<Subscription<K>, HubFull>
    where
        F: FnOnce() -> S + Send + 'static,
        S: Stream<Item = Event> + Send + 'static,
    {
        let mut state = self.lock();
        if let Some(entry) = state.entries.get_mut(&key) {
            entry.subscribers += 1;
            entry.activity += 1;
            let rx = entry.tx.subscribe();
            return Ok(Subscription { rx, hub: self.clone(), key: Some(key), id: entry.id });
        }
        state.stopping.retain(|_, finished| !finished.is_done());
        if state.entries.len() + state.stopping.len() >= self.limits.max_keys {
            return Err(HubFull);
        }
        state.next_id += 1;
        let id = state.next_id;
        let (tx, rx) = broadcast::channel(self.limits.capacity.max(1));
        let previous = state.stopping.remove(&key);
        let finished = Arc::new(Finished::default());
        let run = self.clone().run(key.clone(), id, tx.clone(), make, previous, finished.clone());
        let Some(producer) = self.shutdown.spawn(run) else { return Err(HubFull) };
        let entry = Entry { id, activity: 0, subscribers: 1, tx, producer: Some(producer), finished };
        state.entries.insert(key.clone(), entry);
        Ok(Subscription { rx, hub: self.clone(), key: Some(key), id })
    }

    /// Sends `event` to the subscribers of `key`, if any (never creates a key).
    pub fn publish(&self, key: &K, event: Event) {
        if let Some(entry) = self.lock().entries.get(key) {
            let _ = entry.tx.send(HubEvent::Item(event));
        }
    }

    /// Keys with an entry (tests, metrics).
    pub fn active_keys(&self) -> usize {
        self.lock().entries.len()
    }

    async fn run<S, F>(
        self,
        key: K,
        id: u64,
        tx: broadcast::Sender<HubEvent>,
        make: F,
        previous: Option<Arc<Finished>>,
        finished: Arc<Finished>,
    ) where
        F: FnOnce() -> S + Send + 'static,
        S: Stream<Item = Event> + Send + 'static,
    {
        let _finished = MarkFinished(finished);
        // Never two producers of one key at once.
        if let Some(previous) = previous {
            previous.wait().await;
        }
        let cancel = self.shutdown.token();
        let produced = std::panic::AssertUnwindSafe(async {
            let stream = make();
            let mut stream = std::pin::pin!(stream);
            loop {
                tokio::select! {
                    () = cancel.cancelled() => break,
                    item = stream.next() => match item {
                        Some(event) => { let _ = tx.send(HubEvent::Item(event)); }
                        None => break,
                    },
                }
            }
        })
        .catch_unwind()
        .await;
        if produced.is_err() {
            tracing::error!("a hub producer panicked");
        }
        // Ended and the entry's removal together: a subscriber joins either
        // before (and gets Ended) or after (and starts a new producer).
        let mut state = self.lock();
        if state.entries.get(&key).is_some_and(|e| e.id == id) {
            state.entries.remove(&key);
        }
        let _ = tx.send(HubEvent::Ended);
    }

    fn release(&self, key: &K, id: u64) {
        let mut state = self.lock();
        let Some(entry) = state.entries.get_mut(key).filter(|e| e.id == id) else { return };
        entry.subscribers = entry.subscribers.saturating_sub(1);
        if entry.subscribers > 0 {
            return;
        }
        let activity = entry.activity;
        let hub = self.clone();
        let key = key.clone();
        let grace = self.limits.idle_grace;
        tokio::spawn(async move {
            tokio::time::sleep(grace).await;
            let mut state = hub.lock();
            // Nobody came back during the grace, and it is still that entry.
            let idle =
                state.entries.get(&key).is_some_and(|e| e.id == id && e.subscribers == 0 && e.activity == activity);
            if idle && let Some(entry) = state.entries.remove(&key) {
                if let Some(producer) = entry.producer {
                    producer.abort();
                }
                state.stopping.insert(key, entry.finished);
            }
        });
    }
}

/// A subscription to one key; leaving drops it.
pub struct Subscription<K: Eq + Hash + Clone + Send + Sync + 'static> {
    rx: broadcast::Receiver<HubEvent>,
    hub: Hub<K>,
    key: Option<K>,
    id: u64,
}

impl<K: Eq + Hash + Clone + Send + Sync + 'static> Subscription<K> {
    /// The next event; `None` after `Ended`.
    pub async fn next(&mut self) -> Option<HubEvent> {
        match self.rx.recv().await {
            Ok(event) => Some(event),
            Err(broadcast::error::RecvError::Lagged(missed)) => Some(HubEvent::Lagged(missed)),
            Err(broadcast::error::RecvError::Closed) => None,
        }
    }

    /// As server-sent events: items as they are, `Lagged` as an event
    /// `resync` (the page reloads its state), ending with the producer.
    pub fn into_events(self) -> impl Stream<Item = Event> + Send {
        self.into_stream().filter_map(|event| async move {
            match event {
                HubEvent::Item(event) => Some(event),
                HubEvent::Lagged(missed) => Event::json("resync", &serde_json::json!({"missed": missed})).ok(),
                HubEvent::Ended => None,
            }
        })
    }

    /// As a stream of events, ending after `Ended`.
    pub fn into_stream(self) -> impl Stream<Item = HubEvent> + Send {
        futures_util::stream::unfold((self, false), |(mut sub, done)| async move {
            if done {
                return None;
            }
            let event = sub.next().await?;
            let ended = event == HubEvent::Ended;
            Some((event, (sub, ended)))
        })
    }
}

impl<K: Eq + Hash + Clone + Send + Sync + 'static> Drop for Subscription<K> {
    fn drop(&mut self) {
        if let Some(key) = self.key.take() {
            self.hub.release(&key, self.id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    fn event(n: u64) -> Event {
        Event::json("tick", &n).unwrap()
    }

    /// A producer counting its starts, ticking every second.
    fn ticking(starts: &Arc<AtomicUsize>) -> impl FnOnce() -> futures_util::stream::BoxStream<'static, Event> + use<> {
        let starts = starts.clone();
        move || {
            starts.fetch_add(1, Ordering::SeqCst);
            futures_util::stream::unfold(0u64, |n| async move {
                tokio::time::sleep(Duration::from_secs(1)).await;
                Some((event(n), n + 1))
            })
            .boxed()
        }
    }

    fn hub(shutdown: &Arc<Shutdown>) -> Hub<&'static str> {
        Hub::new(shutdown.clone(), HubLimits { max_keys: 2, capacity: 4, idle_grace: Duration::from_secs(5) })
    }

    #[test]
    fn events_are_named_bounded_json() {
        let ok = Event::json("container.stats", &serde_json::json!({"cpu": 1})).unwrap();
        assert_eq!((ok.name(), ok.data()), ("container.stats", r#"{"cpu":1}"#));
        assert!(Event::json("Bad Name", &1).is_err());
        assert_eq!((Event::resync().name(), Event::resync().data()), ("resync", "{}"));
        assert!(Event::json("", &1).is_err());
        let big = "x".repeat(MAX_EVENT_BYTES);
        let err = Event::json("big", &big).unwrap_err().to_string();
        assert!(!err.contains("xxxx"), "the value is never quoted: {err}");
    }

    #[tokio::test(start_paused = true)]
    async fn one_producer_per_key_stopping_after_the_grace() {
        let shutdown = Arc::new(Shutdown::default());
        let hub = hub(&shutdown);
        let starts = Arc::new(AtomicUsize::new(0));
        let mut a = hub.subscribe("c1", ticking(&starts)).unwrap();
        let mut b = hub.subscribe("c1", ticking(&starts)).unwrap();
        assert_eq!(a.next().await, Some(HubEvent::Item(event(0))));
        assert_eq!(b.next().await, Some(HubEvent::Item(event(0))));
        assert_eq!(starts.load(Ordering::SeqCst), 1, "the second subscriber shares the producer");
        drop(a);
        drop(b);
        // Back during the grace: the same producer.
        tokio::time::sleep(Duration::from_secs(2)).await;
        let c = hub.subscribe("c1", ticking(&starts)).unwrap();
        tokio::time::sleep(Duration::from_secs(10)).await;
        assert_eq!(starts.load(Ordering::SeqCst), 1, "the stale idle timer did not stop it");
        assert_eq!(hub.active_keys(), 1);
        drop(c);
        tokio::time::sleep(Duration::from_secs(6)).await;
        assert_eq!(hub.active_keys(), 0, "stopped after the grace");
        let mut d = hub.subscribe("c1", ticking(&starts)).unwrap();
        assert_eq!(d.next().await, Some(HubEvent::Item(event(0))));
        assert_eq!(starts.load(Ordering::SeqCst), 2, "a new producer once the old one stopped");
    }

    #[tokio::test(start_paused = true)]
    async fn slow_subscribers_lag_and_keys_are_bounded() {
        let shutdown = Arc::new(Shutdown::default());
        let hub = hub(&shutdown);
        let starts = Arc::new(AtomicUsize::new(0));
        let mut slow = hub.subscribe("c1", ticking(&starts)).unwrap();
        tokio::time::sleep(Duration::from_secs(10)).await;
        assert!(matches!(slow.next().await, Some(HubEvent::Lagged(n)) if n > 0));
        let _two = hub.subscribe("c2", ticking(&starts)).unwrap();
        assert_eq!(hub.subscribe("c3", ticking(&starts)).err(), Some(HubFull));
    }

    #[tokio::test(start_paused = true)]
    async fn publish_never_creates_keys_and_ended_producers_are_reported() {
        let shutdown = Arc::new(Shutdown::default());
        let hub = hub(&shutdown);
        hub.publish(&"nobody", event(1));
        assert_eq!(hub.active_keys(), 0);
        let mut once = hub.subscribe("c1", || futures_util::stream::iter([event(7)])).unwrap();
        assert_eq!(once.next().await, Some(HubEvent::Item(event(7))));
        assert_eq!(once.next().await, Some(HubEvent::Ended));
        tokio::task::yield_now().await;
        assert_eq!(hub.active_keys(), 0, "an ended producer leaves");
        let mut quiet = hub.subscribe("c2", futures_util::stream::pending::<Event>).unwrap();
        hub.publish(&"c2", event(3));
        assert_eq!(quiet.next().await, Some(HubEvent::Item(event(3))));
    }

    #[tokio::test(start_paused = true)]
    async fn a_factory_runs_outside_the_lock_and_may_panic() {
        let shutdown = Arc::new(Shutdown::default());
        let hub = hub(&shutdown);
        // A factory using the hub itself does not deadlock.
        let inner = hub.clone();
        let mut sub = hub
            .subscribe("c1", move || {
                inner.publish(&"c1", event(9));
                futures_util::stream::iter([event(1)])
            })
            .unwrap();
        let mut seen = Vec::new();
        while let Some(e) = sub.next().await {
            seen.push(e);
        }
        assert!(seen.contains(&HubEvent::Item(event(1))) && seen.last() == Some(&HubEvent::Ended), "{seen:?}");
        let mut failed =
            hub.subscribe("c2", || -> futures_util::stream::Empty<Event> { panic!("factory failed") }).unwrap();
        assert_eq!(failed.next().await, Some(HubEvent::Ended));
    }

    #[tokio::test(start_paused = true)]
    async fn a_panicking_producer_ends_its_subscribers() {
        let shutdown = Arc::new(Shutdown::default());
        let hub = hub(&shutdown);
        let mut sub = hub
            .subscribe("c1", || {
                futures_util::stream::once(async {
                    panic!("producer failed");
                })
                .boxed()
            })
            .unwrap();
        assert_eq!(sub.next().await, Some(HubEvent::Ended));
    }

    #[tokio::test(start_paused = true)]
    async fn shutdown_stops_producers_and_admission_then_aborts_the_rest() {
        let shutdown = Arc::new(Shutdown::default());
        let hub = hub(&shutdown);
        let starts = Arc::new(AtomicUsize::new(0));
        let mut sub = hub.subscribe("c1", ticking(&starts)).unwrap();
        // A task that ignores the token is aborted after the grace.
        assert!(shutdown.spawn(std::future::pending()).is_some());
        let polite = shutdown.token();
        assert!(shutdown.spawn(async move { polite.cancelled().await }).is_some());
        assert_eq!(sub.next().await, Some(HubEvent::Item(event(0))));
        shutdown.begin();
        assert!(!shutdown.is_open());
        assert!(shutdown.spawn(async {}).is_none(), "no new work once stopping");
        assert_eq!(hub.subscribe("c2", ticking(&starts)).err(), Some(HubFull), "no producer once stopping");
        assert_eq!(sub.next().await, Some(HubEvent::Ended), "producers stop with the token");
        assert!(!shutdown.drain(Duration::from_secs(5)).await, "one task had to be aborted");
    }
}
