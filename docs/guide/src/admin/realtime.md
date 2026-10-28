# Realtime

A panel page can show live data (stats, logs, progress) through **server-sent
events**, and talk both ways (terminals, consoles) through a
[**WebSocket**](#websocket), from an [admin route](admin-routes.md). A stream
runs on the panel's session, is rechecked while open, and ends when the user
signs out or loses access. Streams and sockets only read and run commands
you authorize; changes to data still go through ordinary POST admin routes.

![A live counter and an echo form on a script page](/img/admin/realtime-live.webp)
*A RustCMS demo application: the Live page (server-sent events and a WebSocket).*

![RDockPanel's dashboard: CPU, memory, disk and network figures with live sparklines](/img/admin/realtime-rdockpanel.webp)
*RDockPanel, a hosting panel built on RustCMS: the server figures on its dashboard are pushed over an event stream.*

## Quick start

```rust
// src/routes/admin.rs
AdminRoutes::new("/clock").events("/now", AdminAccess::authenticated(), now)

// src/http/controllers/clock.rs
use rustcms::auth::{EventStream, LiveSession};
use rustcms::realtime::{Event, stream, time};

pub async fn now(live: LiveSession) -> EventStream {
    // One event a second, counting from 0.
    let ticks = stream::unfold(0u64, |n| async move {
        time::sleep(std::time::Duration::from_secs(1)).await;
        Some((Event::json("tick", &n).ok()?, n + 1))
    });
    live.stream(ticks)
}
```

```js
// In a v1 script page
ctx.realtime.events("/clock/now", { onEvent(name, data) { /* name "tick", data 0, 1, 2… */ } });
```

## Full example

The demo streams a counter shared by every viewer, resets it with a POST
and echoes text over a WebSocket. The routes:

<<< @/../../../examples/demo/src/routes/admin.rs#admin-routes [src/routes/admin.rs]

The stream subscribes to a hub (one producer for every viewer) and sends a
first event at once:

<<< @/../../../examples/demo/src/http/controllers/live_controller.rs#ticks [src/http/controllers/live_controller.rs]

The service behind it, bound in `AppServiceProvider::register` with
`Ticker::new(app.shutdown())`:

<<< @/../../../examples/demo/src/services/ticker.rs#ticker [src/services/ticker.rs]

The page's script:

<<< @/../../../examples/demo/resources/admin/js/demo/live.js#events [resources/admin/js/demo/live.js]

## Server-sent events

- The route admits the request before the handler runs: the origin, the
  open limits and the stream's place. The handler takes `LiveSession` (the
  admitted stream, with `live.principal()`); a handler without it is
  admitted all the same.
- `live.stream(events)` takes any `Stream<Item = Event>` and returns the
  `EventStream` response.
- `Event::json(name, &value)` serializes a value. Names are 1–64 characters
  of `[a-z0-9_.-]`; the JSON is at most 64 KiB (`MAX_EVENT_BYTES`).
  `Event::name()` and `Event::data()` read them back.
- A stream must not change anything. Use a POST route for that.
- `rustcms::realtime` re-exports what producers need: `stream`
  (`unfold`, `once`, `iter`), `StreamExt` (`.chain`, `.map`) and `time`
  (`sleep`, `interval`).

### Per-target authorization

The route's `AdminAccess` gates the route; `.target(check)` decides for this
container, site or record. It runs before the first event and at every
recheck:

```rust
// Sketch: `Docker` is your service.
pub async fn stats(live: LiveSession, Path(id): Path<String>, Inject(docker): Inject<Docker>) -> AppResult<EventStream> {
    let events = docker.stats_stream(&id);
    Ok(live.stream(events).target(move |principal| {
        let (docker, id) = (docker.clone(), id.clone());
        async move { docker.authorize(&principal, &id).await }   // may this user still see it?
    }))
}
```

| `check` returns | The stream |
|---|---|
| `Ok(())` | goes on |
| `Err(Forbidden)`, `Err(NotFound)`, `Err(Unauthenticated)` | ends with `closed: forbidden` |
| any other error, or no answer within 5 s | ends with `closed: error` |

A check written once for several streams returns a `TargetFuture` (a boxed
future):

```rust
use rustcms::Principal;
use rustcms::auth::TargetFuture;

fn sees_container(docker: Docker, id: String) -> impl Fn(Principal) -> TargetFuture + Send + Sync + 'static {
    move |principal| {
        let (docker, id) = (docker.clone(), id.clone());
        Box::pin(async move { docker.authorize(&principal, &id).await })
    }
}
// live.stream(events).target(sees_container(docker, id))
```

## Many viewers, one producer: the hub

```rust
// Bound once, e.g. in a provider's register:
// app.bind(Stats(Hub::new(app.shutdown(), HubLimits::default())))
pub struct Stats(pub Hub<String>);

let subscription = stats.0
    .subscribe(format!("container:{id}"), move || docker_stats_stream(id))
    .map_err(|_| AppError::TooManyRequests { retry_after: 5 })?;
Ok(live.stream(subscription.into_events()).target(/* … */))
```

- The first subscriber of a key starts its producer; the others share it.
  The producer stops `idle_grace` (5 s) after the last one leaves.
- `hub.publish(&key, event)` sends to current subscribers, for example after
  a POST changed something (the demo's `reset`).
- A viewer that falls behind gets `event: resync` with `{"missed": n}`:
  reload what the page shows. A producer whose own source lost events (an
  upstream broadcast that lagged, a reconnect to the source) yields
  `Event::resync()` (data `{}`), so pages handle both the same way.
- The hub knows no users. A key must hold everything that decides its data
  (`container:<id>`). Never share a key between users who should see
  different data.
- The hub lives in one process: each instance runs its own producers.

| Hub API (`rustcms::realtime`) | |
|---|---|
| `Hub::new(app.shutdown(), HubLimits::default())` | a hub; `Hub` is `Clone` (clones share it) |
| `HubLimits { max_keys, capacity, idle_grace }` | keys at once (1000), events buffered per key (64), producer grace (5 s) |
| `hub.subscribe(key, make)` | `Result<Subscription, HubFull>`; `HubFull` when `max_keys` keys are active, and while the server is stopping |
| `hub.publish(&key, event)`, `hub.active_keys()` | send to a key's subscribers; how many keys are active |
| `subscription.into_events()` | a stream of `Event` (lags become `resync`) |
| `subscription.next()`, `into_stream()` | `HubEvent::Item(event)`, `Lagged(n)` or `Ended`, for use outside SSE |
| `Shutdown::{token, is_open, spawn}` | `app.shutdown()`: a cancellation token, whether new work may start, and tasks that get the grace period when the server stops |

## Events in a script page {#in-a-script-page}

```js
window.rustcms.registerPage("docker/stats", {
  apiVersion: 1,
  mount(element, ctx) {
    const value = document.createElement("output");
    element.append(value);
    ctx.realtime.events(`/docker/containers/${ctx.data.id}/stats`, {
      onEvent(name, data) {
        if (name === "stats") value.textContent = `${data.cpu}%`;
        if (name === "resync") ctx.reload();
      },
      onState(state) { /* state.state: "open", "reconnecting" or "closed"; also state.status or state.reason */ },
    });
    return { update() {}, unmount() {} };
  },
});
```

`ctx.realtime.events(path, handlers)` opens the route under
`{admin}/api/x` and returns `{ close() }`. In a React page,
`useEvents(props, path, handlers)` does the same while the component is
mounted ([React pages](react-pages.md#calling-your-routes)).

- The path starts with one `/`; each segment is checked and must decode to
  `A-Z a-z 0-9 . _ ~ -` (no `?`, `#`, `//` or encoded slashes): the call
  throws otherwise. Use ids made of those characters in stream paths.
- It is closed for you when the view is unmounted or the session changes
  (sign-in, sign-out, credentials rotated in any tab).
- It reconnects with backoff (1 s, doubling up to 30 s, with jitter) after a
  dropped connection or a server restart.
- It stops for good on 400, 401, 403, 404 or 405, when the server ends it
  with `signed_out` or `forbidden`, and with reason `session` when the
  response belongs to another account (the cookie changed under this tab).
- It is built on `fetch`. Do not use `EventSource`: it hides statuses and
  reconnects on its own after a sign-out.

## What the server enforces for streams {#what-the-server-enforces}

| | |
|---|---|
| Origin | an `Origin`, when sent, must be the panel's; `Sec-Fetch-Site`, when sent, `same-origin`; with neither (an old browser), a same-origin `Referer`. No CSRF token is needed |
| Access | the route's `AdminAccess` before anything runs (401, 403); anything but `GET` is 405 |
| While open | every `revalidate_secs` (30; the first recheck comes after 50–100% of it): the session, the user and its credential version, its permissions and `target`. Delivery stops within `revalidate_secs + 5 s` of a sign-out, a password change, a disabled account or a lost permission (`event: closed`) |
| Limits | per instance: `max_connections` (1000), `max_per_user` (10), 429 beyond. Opens per user (`opens_per_user_per_minute`, 60) and per IP (`opens_per_ip_per_minute`, 300) use the rate limiter, so with Redis they count across instances |
| Slow clients | a bounded queue (64 events, 1 MiB): a client that stops reading is dropped (`lagging`) and never holds the server |
| Keep-alive | a comment every 15 s when nothing else was sent |
| Shutdown | new streams get 503; open ones end with `closed: shutdown` within `shutdown_grace_secs` (5) |

The panel's origins are `auth.admin_origins`, or the origin of `APP_URL`
when that list is empty. Behind a proxy, set them to the address users
open.

Events end with `event: closed` and data `{"reason": …}`: one of
`signed_out`, `forbidden`, `shutdown`, `lagging` or `error`.

Each recheck reads the session, the user and the grants (about 3 queries),
plus what `target` does. At 1000 streams every 30 s that is about 100
queries per second: raise `revalidate_secs` or the database pool
(`database.max_connections`) accordingly.

Streams are not cached, carry the session's `Set-Cookie` like any admin
response, and never renew the session by themselves.

## WebSocket

The demo's echo socket sends back what it gets, and runs one command,
`whoami`, after authorizing it:

<<< @/../../../examples/demo/src/http/controllers/live_controller.rs#echo [src/http/controllers/live_controller.rs]

A terminal is the typical use. Sketch, with `Docker` as your service:

```rust
use rustcms::auth::{LiveSocket, SocketMessage, SocketResponse};

pub async fn exec(live: LiveSocket, Path(id): Path<String>, Inject(docker): Inject<Docker>) -> SocketResponse {
    let target = (docker.clone(), id.clone());
    live.accept(move |mut socket| async move {
        // Dropped when the socket ends: the process goes with it.
        let Ok(mut shell) = docker.exec(&id).kill_on_drop(true).spawn() else { return };
        loop {
            tokio::select! {
                input = socket.recv() => match input {
                    Some(SocketMessage::Text(command)) if command.starts_with("resize ") => {
                        // A command: authorized when it runs, on a fresh principal.
                        let Ok(principal) = socket.authorize().await else { break };
                        if !principal.can("docker.exec") { break }
                        shell.resize(&command[7..]);
                    }
                    Some(SocketMessage::Binary(keys)) => shell.write(&keys).await,
                    Some(_) => {}
                    None => break,
                },
                output = shell.read() => match output {
                    Some(bytes) => if socket.send(SocketMessage::Binary(bytes)).await.is_err() { break },
                    None => break,
                },
            }
        }
    })
    .target(move |principal| {
        let (docker, id) = target.clone();
        async move { docker.authorize(&principal, &id).await }
    })
}
```

- The route admits the handshake before the handler runs:
  - a mandatory `Origin` that is exactly the panel's (no `Referer` fallback);
  - the session's CSRF token in `Sec-WebSocket-Protocol`
    (`rustcms.v1, rustcms.csrf.<token>`, which `ctx.realtime.socket` sends),
    checked before the open limits;
  - the open limits and a place, shared with streams.

  The `101` selects `rustcms.v1` only.
- `live.accept(session)` runs `session` once the connection is upgraded and
  the first check passed (the session, the user, the access, `target`).
  `session` gets an `AdminSocket`:
  - `recv()` gives `Text` or `Binary` messages, and `None` once the socket
    ended;
  - `send(message)` waits at most `send_timeout_secs` (10) for a slow client,
    then the socket ends as `lagging`. It returns `SendError::TooLarge` for a
    message over `max_message_bytes` (the socket stays open) and
    `SendError::Closed` once the socket has ended;
  - `authorize()` checks again now and returns the fresh principal, or
    `SocketClosed` (stop).
- **Authorize every command when it runs:** call `authorize()` and check the
  command's own permission on what it returns, never on `live.principal()`.
  A raw terminal is one capability: `target` decides who may use it. Its
  input is accepted until the next recheck notices a revocation; it cannot
  check each shell command, nor undo one that ran.
- **Cleanup belongs in `Drop`.** When the socket ends (the client left, a
  recheck failed, a slow client, the server stopping), the session's future
  is dropped at once, before the close frame is sent. A child process needs
  `kill_on_drop`. A remote process (`docker exec` through the API) may need
  an explicit stop: dropping its stream does not end it.
- When `session` returns, its queued output is flushed within
  `send_timeout_secs` before the `1000` close; any other end discards it.
- Close codes: `4401 signed_out`, `4403 forbidden`, `1001 shutdown`,
  `4008 lagging`, `1009 too_big`, `1011 error` (also a session that panics),
  `1000` when the session returned. Delivering the code is best effort: a
  connection with unread input may be reset first.

### Sockets in a script page {#in-a-script-page-1}

The demo's echo form:

<<< @/../../../examples/demo/resources/admin/js/demo/live.js#socket [resources/admin/js/demo/live.js]

```js
const socket = ctx.realtime.socket(`/docker/containers/${ctx.data.id}/exec`, {
  onOpen() { /* … */ },
  onMessage(data) { /* a string, or an ArrayBuffer for binary messages */ },
  onState(state) { /* state.state: "open", "reconnecting" or "closed"; also state.code or state.reason */ },
}, { reconnect: false });
socket.send(new TextEncoder().encode("ls\n"));   // false when not open, too large, or 1 MiB is already queued
```

In a React page, `useSocket(props, path, handlers, { reconnect })` opens it
while mounted and returns `{ send }`.

- Paths follow the same rules as for events.
- Closed for you on unmount and on a session change.
- **No reconnect by default:** a new socket is a new session on the server
  (a new shell). With `{ reconnect: true }`:
  - it retries with backoff and a fresh token;
  - it stops on 4401 and 4403, and after 5 failed or short (under 30 s)
    connections;
  - a refused handshake (seen as 1006) checks the session once.
- Input is never queued for replay.
- `send` refuses (returns `false`) a message over **64 KiB**. This browser
  limit is fixed: raising `[realtime] max_message_bytes` raises what the
  server accepts and sends, not what a page can send.
- xterm.js injects `<style>` elements, which the panel's CSP blocks. A page
  that embeds it needs its own workaround in its script. The CSP stays as it
  is.

### What the server enforces for sockets {#what-the-server-enforces-1}

| | |
|---|---|
| Origin | mandatory, exactly one of the panel's origins; `Sec-Fetch-Site`, when sent, `same-origin` |
| CSRF | the session's token as the `rustcms.csrf.<token>` subprotocol, beside `rustcms.v1`, nothing else |
| Access | the route's `AdminAccess` before anything runs (401, 403); anything but `GET` is 405; a handler error before `accept` is an ordinary JSON error |
| While open | every `socket_revalidate_secs` (10; the first recheck comes after 50–100% of it) and at each `authorize()`: the session, the user and its version, the permissions and `target`. Work stops within `socket_revalidate_secs + 5 s` of a sign-out, an expired session, a password change, a disabled account or a lost permission |
| Limits | shared with streams: `max_connections`, `max_per_user`, the open rates; messages up to `max_message_bytes` (64 KiB) |
| Slow or flooding clients | 64 messages or 1 MiB queued each way; a client that floods a session, or stops reading, is dropped (`lagging`) |
| Keepalive | one ping at a time, 15 s after the last answer; no matching pong within 45 s ends the socket |
| Shutdown | new sockets get 503; open ones end with `1001` within `shutdown_grace_secs` |

Sockets never renew the session. Limits are per instance. Behind a proxy,
pass `Upgrade` and drop `Sec-WebSocket-Protocol` from access logs (see the
[production checklist](../security/production.md)).

## Configuration

The `[realtime]` section (`config/realtime.toml`, or `RUSTCMS_REALTIME__*`
variables), per instance:

| Key | Default | |
|---|---|---|
| `revalidate_secs` | `30` | stream rechecks (at least 1) |
| `max_connections` | `1000` | open streams and sockets on this instance |
| `max_per_user` | `10` | open streams and sockets of one user on this instance |
| `opens_per_user_per_minute` | `60` | |
| `opens_per_ip_per_minute` | `300` | shared IPs share it |
| `shutdown_grace_secs` | `5` | grace for open streams and sockets when the server stops |
| `socket_revalidate_secs` | `10` | socket rechecks (at least 1) |
| `max_message_bytes` | `65536` | largest WebSocket message, either way (1 byte to 1 MiB) |
| `send_timeout_secs` | `10` | longest wait for a socket send (at least 1) |

::: code-group

```toml [config/realtime.toml]
revalidate_secs = 30
max_per_user = 10
```

```sh [.env]
RUSTCMS_REALTIME__REVALIDATE_SECS=30
```

:::

See also [Configuration](../getting-started/configuration.md).

## Testing

Test access to stream and socket routes with `assert_access`
([Admin routes](admin-routes.md#testing)). Test a socket's behaviour over a
real connection with `client.socket(path)`
([Testing](../basics/testing.md)):

<<< @/../../../examples/demo/tests/feature/live.rs#socket-test [tests/feature/live.rs]

## Security notes

- Streams are read-only and rechecked while open; sockets authorize each
  command when it runs. Neither renews the session.
- Every stream and socket path is an admin route with an explicit access;
  there are no other ways to open one.
- WebSocket handshakes need the exact panel origin and the CSRF token.

## Related pages

- [Admin routes](admin-routes.md)
- [React pages](react-pages.md): `useEvents`, `useSocket`
- [Several instances](../digging-deeper/instances.md)
- [Production checklist](../security/production.md)
