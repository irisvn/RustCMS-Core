//! WebSocket on the admin session (ADR 0029): declared with
//! [`AdminRoutes::socket`](crate::AdminRoutes::socket), admitted with a
//! mandatory exact `Origin` and the session's CSRF token as a subprotocol,
//! supervised from before the `101`, rechecked while open, bounded both ways.
//!
//! ```rust,ignore
//! async fn terminal(live: LiveSocket, Path(id): Path<String>, Inject(docker): Inject<Docker>) -> AppResult<SocketResponse> {
//!     let target = id.clone();
//!     Ok(live
//!         .accept(move |mut socket| async move {
//!             while let Some(message) = socket.recv().await {
//!                 if socket.send(message).await.is_err() { break }
//!             }
//!         })
//!         .target(move |principal| { let id = target.clone(); async move { may_use(&principal, &id) } }))
//! }
//! ```

use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::extract::{FromRequestParts, Request};
use axum::http::request::Parts;
use axum::http::{HeaderMap, header};
use axum::response::{IntoResponse, Response};
use futures_util::future::BoxFuture;
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{FutureExt, SinkExt, StreamExt};
use rustcms_core::{AppError, AppResult, AppState, Principal};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc, oneshot};
use tokio::time::Instant;

use super::{Closed, Live, OriginRule, QUEUE_BYTES, QUEUE_EVENTS, TargetCheck, TargetFuture, unavailable};
use crate::http::admin_routes::AdminAccess;
use crate::session::Session;
use crate::state::AuthState;

/// The only application subprotocol (selected in the `101`).
pub(crate) const PROTOCOL: &str = "rustcms.v1";
/// Prefix of the subprotocol that carries the session's CSRF token.
pub(crate) const CSRF_PROTOCOL: &str = "rustcms.csrf.";
/// Longest wait between the `101` and the upgraded connection.
const UPGRADE_WAIT: Duration = Duration::from_secs(10);
/// Longest attempt to send the close frame.
const CLOSE_WAIT: Duration = Duration::from_secs(2);
/// A ping goes out this long after the previous one was answered.
const PING_EVERY: Duration = Duration::from_secs(15);
/// A ping unanswered this long ends the socket.
const PONG_WAIT: Duration = Duration::from_secs(45);
/// How often the keepalive is looked at.
const KEEPALIVE_TICK: Duration = Duration::from_secs(1);

/// How a socket ends from outside its supervisor (a slow send, a failed
/// `authorize`): the first reason wins.
#[derive(Default)]
struct Ending {
    reason: Mutex<Option<Closed>>,
    notify: tokio::sync::Notify,
}

impl Ending {
    fn end(&self, reason: Closed) {
        self.reason.lock().unwrap_or_else(std::sync::PoisonError::into_inner).get_or_insert(reason);
        self.notify.notify_waiters();
    }

    fn reason(&self) -> Option<Closed> {
        *self.reason.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    async fn ended(&self) -> Closed {
        loop {
            let notified = self.notify.notified();
            let mut notified = std::pin::pin!(notified);
            // Registered before the reason is read: no wake-up is missed.
            notified.as_mut().enable();
            if let Some(reason) = self.reason() {
                return reason;
            }
            notified.await;
        }
    }
}

/// Socket admission: a mandatory `Origin`, exactly one, equal to an allowed
/// origin (not `null`); `Sec-Fetch-Site`, when sent, `same-origin`. A
/// repeated or unreadable header is refused.
pub(crate) fn check_socket_origin(auth: &AuthState, headers: &HeaderMap) -> AppResult<()> {
    let single = |name: header::HeaderName| -> AppResult<Option<String>> {
        let mut values = headers.get_all(name).iter();
        let first = values.next();
        if values.next().is_some() {
            return Err(AppError::Forbidden);
        }
        first.map(|v| v.to_str().map(str::to_owned).map_err(|_| AppError::Forbidden)).transpose()
    };
    let origin = single(header::ORIGIN)?.ok_or(AppError::Forbidden)?;
    let site = single(header::HeaderName::from_static("sec-fetch-site"))?;
    let exact = crate::config::origin_of(&origin).filter(|o| *o == origin.to_ascii_lowercase());
    if !exact.is_some_and(|o| auth.allowed_origins.contains(&o)) {
        return Err(AppError::Forbidden);
    }
    if site.is_some_and(|s| s != "same-origin") {
        return Err(AppError::Forbidden);
    }
    Ok(())
}

/// The offered subprotocols, every header and every comma-separated token:
/// exactly [`PROTOCOL`] and `rustcms.csrf.<the session's token>`.
pub(crate) fn check_protocols(headers: &HeaderMap, session: &Session) -> AppResult<()> {
    let mut tokens = Vec::new();
    for value in headers.get_all(header::SEC_WEBSOCKET_PROTOCOL) {
        let value = value.to_str().map_err(|_| AppError::Forbidden)?;
        for token in value.split(',') {
            let token = token.trim();
            if token.is_empty() {
                return Err(AppError::Forbidden);
            }
            tokens.push(token);
        }
    }
    let [a, b] = tokens[..] else { return Err(AppError::Forbidden) };
    let csrf = match (a, b) {
        (PROTOCOL, other) | (other, PROTOCOL) => other.strip_prefix(CSRF_PROTOCOL),
        _ => None,
    };
    match csrf {
        Some(token) if session.csrf_matches(token) => Ok(()),
        _ => Err(AppError::Forbidden),
    }
}

/// The admitted socket, waiting for its handler's `LiveSocket`.
#[derive(Clone)]
struct SocketAdmission(Arc<Mutex<Option<LiveSocket>>>);

/// Admits a socket request (the route layer, before the handler's
/// extractors): the upgrade, the origin, the CSRF subprotocol, then the
/// shared admission.
pub(crate) async fn admit(req: Request, access: &AdminAccess) -> Result<Request, Box<Response>> {
    let (mut parts, body) = req.into_parts();
    let state = parts.extensions.get::<AppState>().cloned();
    let Some(state) = state else {
        return Err(Box::new(AppError::internal("admin guard missing").into_response()));
    };
    // Extracted once: it takes the connection's upgrade.
    let upgrade =
        WebSocketUpgrade::from_request_parts(&mut parts, &state).await.map_err(|r| Box::new(r.into_response()))?;
    let mut req = Request::from_parts(parts, body);
    // The token before the limits: a refused handshake takes no place.
    let session = req.extensions().get::<Session>().cloned().unwrap_or_default();
    check_protocols(req.headers(), &session).map_err(|e| Box::new(e.into_response()))?;
    let live = super::admission(&mut req, access, OriginRule::Socket).await?;
    req.extensions_mut().insert(SocketAdmission(Arc::new(Mutex::new(Some(LiveSocket { live, upgrade })))));
    Ok(req)
}

/// An admitted WebSocket (extractor; only in
/// [`AdminRoutes::socket`](crate::AdminRoutes::socket) handlers). Its route
/// admitted the request before any extractor ran: the upgrade, the panel's
/// own origin and the session's CSRF token (403), the open limits and the
/// socket's place (429), the server not stopping (503). Turn it into the
/// response with [`accept`](Self::accept).
pub struct LiveSocket {
    live: Live,
    upgrade: WebSocketUpgrade,
}

impl FromRequestParts<AppState> for LiveSocket {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, _: &AppState) -> Result<Self, Response> {
        let admission = parts.extensions.get::<SocketAdmission>().cloned();
        let live = admission.and_then(|a| a.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take());
        live.ok_or_else(|| {
            AppError::internal("LiveSocket outside AdminRoutes::socket (or taken twice)").into_response()
        })
    }
}

type SessionFn = Box<dyn FnOnce(AdminSocket) -> BoxFuture<'static, ()> + Send>;

impl LiveSocket {
    /// The user's principal when the socket was opened. Authorize commands
    /// with [`AdminSocket::authorize`], never with this.
    pub fn principal(&self) -> &Principal {
        self.live.principal()
    }

    /// The `101` response; `session` runs once the connection is upgraded and
    /// the first check passed, and is dropped when the socket ends.
    pub fn accept<F, Fut>(self, session: F) -> SocketResponse
    where
        F: FnOnce(AdminSocket) -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        SocketResponse { socket: self, session: Box::new(move |s| session(s).boxed()), target: None }
    }
}

/// The response opening a socket (see [`LiveSocket::accept`]).
pub struct SocketResponse {
    socket: LiveSocket,
    session: SessionFn,
    target: Option<TargetCheck>,
}

impl SocketResponse {
    /// Checks, before the session starts, at every recheck and in
    /// [`AdminSocket::authorize`], that the user may still use this socket's
    /// target: `Err(Forbidden)` or `Err(NotFound)` ends it (close 4403).
    pub fn target<F, Fut>(mut self, check: F) -> Self
    where
        F: Fn(Principal) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = AppResult<()>> + Send + 'static,
    {
        self.target = Some(Arc::new(move |principal| -> TargetFuture { Box::pin(check(principal)) }));
        self
    }
}

impl IntoResponse for SocketResponse {
    fn into_response(self) -> Response {
        let SocketResponse { socket: LiveSocket { live, upgrade }, session, target } = self;
        let config = live.config();
        let max = config.max_message_bytes;
        let upgrade = upgrade
            .protocols([PROTOCOL])
            .max_message_size(max)
            .max_frame_size(max)
            .write_buffer_size(128 * 1024)
            .max_write_buffer_size(QUEUE_BYTES)
            .on_failed_upgrade(|error: axum::Error| tracing::debug!(%error, "websocket upgrade failed"));
        // The supervisor exists, holding the place, before the `101` is sent.
        let (deliver, delivered) = oneshot::channel::<WebSocket>();
        let shutdown = live.shutdown().clone();
        if shutdown.spawn(supervise(live, target, session, delivered)).is_none() {
            return unavailable();
        }
        upgrade.on_upgrade(move |socket| async move {
            // A supervisor that is gone (timed out, stopped) drops it.
            let _ = deliver.send(socket);
        })
    }
}

/// A message from or to the browser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SocketMessage {
    /// A text frame (UTF-8).
    Text(String),
    /// A binary frame.
    Binary(Vec<u8>),
}

impl SocketMessage {
    fn len(&self) -> usize {
        match self {
            Self::Text(text) => text.len(),
            Self::Binary(data) => data.len(),
        }
    }

    fn into_ws(self) -> Message {
        match self {
            Self::Text(text) => Message::Text(text.into()),
            Self::Binary(data) => Message::Binary(data.into()),
        }
    }
}

/// The socket has ended (or ends now): stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the socket is closed")]
pub struct SocketClosed;

/// Why [`AdminSocket::send`] did not send.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SendError {
    /// Larger than `[realtime] max_message_bytes`; the socket stays open.
    #[error("the message is larger than max_message_bytes")]
    TooLarge,
    /// The socket has ended (a slow client ends it as lagging).
    #[error("the socket is closed")]
    Closed,
}

struct Outgoing {
    message: Message,
    _bytes: OwnedSemaphorePermit,
}

/// One open socket, given to the session closure of
/// [`LiveSocket::accept`]. Dropped (with the closure's future) when the
/// socket ends: put cleanup in `Drop` (`kill_on_drop` for a child process).
pub struct AdminSocket {
    live: Arc<Live>,
    target: Option<TargetCheck>,
    input: mpsc::Receiver<(SocketMessage, usize)>,
    input_bytes: Arc<AtomicUsize>,
    output: mpsc::Sender<Outgoing>,
    output_bytes: Arc<Semaphore>,
    ended: Arc<Ending>,
}

impl AdminSocket {
    /// The next message from the browser; `None` once the socket ended.
    pub async fn recv(&mut self) -> Option<SocketMessage> {
        let (message, size) = self.input.recv().await?;
        self.input_bytes.fetch_sub(size, Ordering::SeqCst);
        Some(message)
    }

    /// Sends `message`, waiting at most `[realtime] send_timeout_secs` for
    /// room; a client that does not read in time ends the socket (lagging).
    pub async fn send(&self, message: SocketMessage) -> Result<(), SendError> {
        let size = message.len();
        if size > self.live.config().max_message_bytes {
            return Err(SendError::TooLarge);
        }
        if self.ended.reason().is_some() {
            return Err(SendError::Closed);
        }
        let room = async {
            let bytes = self.output_bytes.clone().acquire_many_owned(size.max(1) as u32).await.ok()?;
            self.output.send(Outgoing { message: message.into_ws(), _bytes: bytes }).await.ok()
        };
        match tokio::time::timeout(self.send_timeout(), room).await {
            Ok(Some(())) => Ok(()),
            Ok(None) => Err(SendError::Closed),
            Err(_) => {
                self.ended.end(Closed::Lagging);
                Err(SendError::Closed)
            }
        }
    }

    /// A fresh check (the session, the user, the route's access and the
    /// target), returning the fresh principal: call it before running each
    /// command, and check the command's own permission on what it returns.
    /// A failed check ends the socket.
    pub async fn authorize(&self) -> Result<Principal, SocketClosed> {
        if self.ended.reason().is_some() {
            return Err(SocketClosed);
        }
        match self.live.check_in_time(self.target.as_ref()).await {
            Ok(principal) => Ok(principal),
            Err(reason) => {
                self.ended.end(reason);
                Err(SocketClosed)
            }
        }
    }

    fn send_timeout(&self) -> Duration {
        Duration::from_secs(self.live.config().send_timeout_secs)
    }
}

/// Why a socket ended, and its close frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum End {
    Closed(Closed),
    /// The session closure returned.
    Finished,
    TooBig,
    /// The client closed or the connection broke: nothing to send.
    Gone,
}

impl End {
    fn frame(self) -> Option<CloseFrame> {
        let (code, reason) = match self {
            Self::Closed(Closed::SignedOut) => (4401, Closed::SignedOut.reason()),
            Self::Closed(Closed::Forbidden) => (4403, Closed::Forbidden.reason()),
            Self::Closed(Closed::Shutdown) => (1001, Closed::Shutdown.reason()),
            Self::Closed(Closed::Lagging) => (4008, Closed::Lagging.reason()),
            Self::Closed(Closed::Error) => (1011, Closed::Error.reason()),
            Self::Finished => (1000, ""),
            Self::TooBig => (1009, "too_big"),
            Self::Gone => return None,
        };
        Some(CloseFrame { code, reason: reason.into() })
    }
}

/// The keepalive: one ping outstanding at a time, its payload never replaced.
#[derive(Debug)]
pub(crate) struct KeepAlive {
    counter: u64,
    outstanding: Option<(u64, Instant)>,
    answered: Instant,
}

/// What the keepalive asks for.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Beat {
    Wait,
    Ping(u64),
    Dead,
}

impl KeepAlive {
    pub(crate) fn new(now: Instant) -> Self {
        Self { counter: 0, outstanding: None, answered: now }
    }

    pub(crate) fn tick(&mut self, now: Instant) -> Beat {
        match self.outstanding {
            Some((_, sent)) if now.duration_since(sent) >= PONG_WAIT => Beat::Dead,
            Some(_) => Beat::Wait,
            None if now.duration_since(self.answered) >= PING_EVERY => {
                self.counter += 1;
                self.outstanding = Some((self.counter, now));
                Beat::Ping(self.counter)
            }
            None => Beat::Wait,
        }
    }

    pub(crate) fn pong(&mut self, payload: &[u8], now: Instant) {
        if let Some((expected, _)) = self.outstanding
            && payload == expected.to_be_bytes()
        {
            self.outstanding = None;
            self.answered = now;
        }
    }
}

/// Runs one socket from before its `101` to its end, owning everything:
/// dropping it (an abort at shutdown) drops the session closure, the reader
/// and the writer at once, and releases the place.
async fn supervise(
    live: Live,
    target: Option<TargetCheck>,
    session: SessionFn,
    delivered: oneshot::Receiver<WebSocket>,
) {
    let token = live.shutdown().token();
    let socket = tokio::select! {
        socket = tokio::time::timeout(UPGRADE_WAIT, delivered) => match socket {
            Ok(Ok(socket)) => socket,
            _ => return,
        },
        () = token.cancelled() => return,
    };
    let (mut sink, stream) = socket.split();
    let live = Arc::new(live);
    let first = tokio::select! {
        checked = live.check_in_time(target.as_ref()) => checked,
        () = token.cancelled() => Err(Closed::Shutdown),
    };
    if let Err(reason) = first {
        close(&mut sink, End::Closed(reason)).await;
        return;
    }
    let config = live.config().clone();
    let (input_tx, input) = mpsc::channel(QUEUE_EVENTS);
    let (output, mut output_rx) = mpsc::channel::<Outgoing>(QUEUE_EVENTS);
    let (ping_tx, mut ping_rx) = mpsc::channel::<u64>(1);
    let input_bytes = Arc::new(AtomicUsize::new(0));
    let ended = Arc::new(Ending::default());
    let keepalive = Arc::new(Mutex::new(KeepAlive::new(Instant::now())));
    let admin = AdminSocket {
        live: live.clone(),
        target: target.clone(),
        input,
        input_bytes: input_bytes.clone(),
        output,
        output_bytes: Arc::new(Semaphore::new(QUEUE_BYTES)),
        ended: ended.clone(),
    };
    let send_timeout = Duration::from_secs(config.send_timeout_secs);
    let end = {
        let mut app = std::pin::pin!(AssertUnwindSafe(session(admin)).catch_unwind());
        let mut reader = std::pin::pin!(read(stream, input_tx, input_bytes, keepalive.clone()));
        let mut writer = std::pin::pin!(write(&mut sink, &mut output_rx, &mut ping_rx, send_timeout));
        let every = Duration::from_secs(config.socket_revalidate_secs);
        // Rechecks are spread out: sockets opened together do not check together.
        let offset = every.mul_f64(rand::random_range(0.5..1.0));
        let mut recheck = tokio::time::interval_at(Instant::now() + offset, every);
        recheck.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut beat = tokio::time::interval(KEEPALIVE_TICK);
        let mut checking: Option<BoxFuture<'_, Result<Principal, Closed>>> = None;
        loop {
            tokio::select! {
                biased;
                () = token.cancelled() => break End::Closed(Closed::Shutdown),
                reason = ended.ended() => break End::Closed(reason),
                done = &mut app => break match (done, ended.reason()) {
                    // A session that stopped because a send or `authorize` ended it.
                    (_, Some(reason)) => End::Closed(reason),
                    (Ok(()), None) => End::Finished,
                    (Err(_), None) => End::Closed(Closed::Error),
                },
                end = &mut reader => break end,
                end = &mut writer => break end,
                checked = async { checking.as_mut().expect("guarded").await }, if checking.is_some() => {
                    checking = None;
                    if let Err(reason) = checked {
                        break End::Closed(reason);
                    }
                }
                _ = recheck.tick(), if checking.is_none() => {
                    let (live, target) = (live.clone(), target.clone());
                    checking = Some(Box::pin(async move { live.check_in_time(target.as_ref()).await }));
                }
                _ = beat.tick() => {
                    let next = keepalive.lock().unwrap_or_else(std::sync::PoisonError::into_inner).tick(Instant::now());
                    match next {
                        Beat::Wait => {}
                        Beat::Ping(n) => { let _ = ping_tx.try_send(n); }
                        Beat::Dead => break End::Gone,
                    }
                }
            }
        }
        // The futures (the session closure first) are dropped here, before
        // anything is sent: application work has stopped.
    };
    // A session that returned has its last messages sent (within the send
    // deadline); any other end discards the queued output, and the close
    // frame goes first.
    if end == End::Finished {
        let flush = async {
            while let Ok(item) = output_rx.try_recv() {
                if sink.send(item.message).await.is_err() {
                    break;
                }
            }
        };
        let _ = tokio::time::timeout(send_timeout, flush).await;
    }
    drop(output_rx);
    close(&mut sink, end).await;
}

/// Sends the close frame (best effort, bounded) and ends the connection.
async fn close(sink: &mut SplitSink<WebSocket, Message>, end: End) {
    if let Some(frame) = end.frame() {
        let _ = tokio::time::timeout(CLOSE_WAIT, sink.send(Message::Close(Some(frame)))).await;
    }
}

/// Reads all the time (pongs and close frames are seen even when the session
/// does not call `recv`); data waits in a bounded queue.
async fn read(
    mut stream: SplitStream<WebSocket>,
    input: mpsc::Sender<(SocketMessage, usize)>,
    queued: Arc<AtomicUsize>,
    keepalive: Arc<Mutex<KeepAlive>>,
) -> End {
    while let Some(message) = stream.next().await {
        let message = match message {
            Ok(Message::Text(text)) => SocketMessage::Text(text.as_str().to_owned()),
            Ok(Message::Binary(data)) => SocketMessage::Binary(data.to_vec()),
            Ok(Message::Pong(payload)) => {
                keepalive.lock().unwrap_or_else(std::sync::PoisonError::into_inner).pong(&payload, Instant::now());
                continue;
            }
            Ok(Message::Ping(_)) => continue,
            Ok(Message::Close(_)) => return End::Gone,
            Err(error) => return if too_big(&error) { End::TooBig } else { End::Gone },
        };
        let size = message.len();
        if queued.load(Ordering::SeqCst) + size > QUEUE_BYTES {
            return End::Closed(Closed::Lagging);
        }
        queued.fetch_add(size, Ordering::SeqCst);
        match input.try_send((message, size)) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => return End::Closed(Closed::Lagging),
            // The session no longer reads: input is dropped.
            Err(mpsc::error::TrySendError::Closed(_)) => {
                queued.fetch_sub(size, Ordering::SeqCst);
            }
        }
    }
    End::Gone
}

fn too_big(error: &axum::Error) -> bool {
    use tokio_tungstenite::tungstenite::Error as WsError;
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(e) = source {
        if let Some(WsError::Capacity(_)) = e.downcast_ref::<WsError>() {
            return true;
        }
        source = e.source();
    }
    false
}

/// Writes queued output and pings; every network write has a deadline.
async fn write(
    sink: &mut SplitSink<WebSocket, Message>,
    output: &mut mpsc::Receiver<Outgoing>,
    pings: &mut mpsc::Receiver<u64>,
    deadline: Duration,
) -> End {
    loop {
        let message = tokio::select! {
            biased;
            Some(n) = pings.recv() => Message::Ping(n.to_be_bytes().to_vec().into()),
            Some(item) = output.recv() => item.message,
            else => return std::future::pending().await,
        };
        match tokio::time::timeout(deadline, sink.send(message)).await {
            Ok(Ok(())) => {}
            Ok(Err(_)) => return End::Gone,
            Err(_) => return End::Closed(Closed::Lagging),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_ping_waits_for_its_own_pong() {
        let start = Instant::now();
        let mut keep = KeepAlive::new(start);
        assert_eq!(keep.tick(start + Duration::from_secs(1)), Beat::Wait);
        assert_eq!(keep.tick(start + Duration::from_secs(15)), Beat::Ping(1));
        // Still outstanding: never replaced by a new ping.
        assert_eq!(keep.tick(start + Duration::from_secs(31)), Beat::Wait);
        // A stale or foreign payload answers nothing.
        keep.pong(&0u64.to_be_bytes(), start + Duration::from_secs(32));
        assert_eq!(keep.tick(start + Duration::from_secs(33)), Beat::Wait);
        // A pong 20 s late (more than 15, less than 45) keeps the socket.
        keep.pong(&1u64.to_be_bytes(), start + Duration::from_secs(35));
        assert_eq!(keep.tick(start + Duration::from_secs(40)), Beat::Wait);
        assert_eq!(keep.tick(start + Duration::from_secs(50)), Beat::Ping(2));
        assert_eq!(keep.tick(start + Duration::from_secs(95)), Beat::Dead);
    }

    #[test]
    fn close_codes() {
        let code = |end: End| end.frame().map(|f| f.code);
        assert_eq!(code(End::Closed(Closed::SignedOut)), Some(4401));
        assert_eq!(code(End::Closed(Closed::Forbidden)), Some(4403));
        assert_eq!(code(End::Closed(Closed::Shutdown)), Some(1001));
        assert_eq!(code(End::Closed(Closed::Lagging)), Some(4008));
        assert_eq!(code(End::Closed(Closed::Error)), Some(1011));
        assert_eq!(code(End::TooBig), Some(1009));
        assert_eq!(code(End::Finished), Some(1000));
        assert_eq!(code(End::Gone), None);
    }
}
