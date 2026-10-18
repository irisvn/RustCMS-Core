# Realtime

A panel page can show live data (stats, logs, progress) through **server-sent
events**, and talk both ways (terminals, consoles) through a
[**WebSocket**](#websocket), from an [admin route](admin-routes.md). The stream runs on the
panel's session, is rechecked while open, and ends when the user signs out
or loses access. Changes still go through ordinary POST admin routes.

```rust,ignore
// src/routes/admin.rs
AdminRoutes::new("/docker")
    .events("/containers/{id}/stats", AdminAccess::can("docker.view"), controllers::docker::stats)
    .post("/containers/{id}/restart", AdminAccess::can("docker.manage"), controllers::docker::restart)
```

```rust,ignore
// src/http/controllers/docker.rs
use rustcms::auth::{EventStream, LiveSession};
use rustcms::realtime::{Event, StreamExt, stream, time};

pub async fn stats(live: LiveSession, Path(id): Path<String>, Inject(docker): Inject<Docker>) -> AppResult<EventStream> {
    let source = docker.clone();
    let target = id.clone();
    let events = stream::unfold((), move |()| {
        let (source, id) = (source.clone(), id.clone());
        async move {
            time::sleep(std::time::Duration::from_secs(2)).await;
            let stats = source.stats(&id).await.ok()?;          // ends the stream on an error
            Some((Event::json("stats", &stats).ok()?, ()))
        }
    });
    Ok(live.stream(events).target(move |principal| {
        let (docker, id) = (docker.clone(), target.clone());
        async move { docker.authorize(&principal, &id).await }   // may this user still see it?
    }))
}
```

- The route admits the request before the handler runs: the origin, the
  open limits and the stream's place. The handler takes `LiveSession` (the
  admitted stream, with `live.principal()`); a handler without it is
  admitted all the same.
- `live.stream(events)` takes any `Stream<Item = Event>`.
  `Event::json(name, &value)` serializes a value (names `[a-z0-9_.-]`, at
  most 64 KiB of JSON).
- `.target(check)` is the per-target authorization. It runs before the first
  event and at every recheck; `Err(Forbidden)` or `Err(NotFound)` ends the
  stream. The route's `AdminAccess` gates the route, while `target` decides
  for this container, site or record.
- A check written once for several streams returns a `TargetFuture` (a
  boxed future):

  ```rust,ignore
  use rustcms::auth::TargetFuture;

  fn sees_container(docker: Docker, id: String) -> impl Fn(Principal) -> TargetFuture + Send + Sync + 'static {
      move |principal| {
          let (docker, id) = (docker.clone(), id.clone());
          Box::pin(async move { docker.authorize(&principal, &id).await })
      }
  }
  // live.stream(events).target(sees_container(docker, id))
  ```
- A stream must not change anything. Use a POST route for that.
- `rustcms::realtime` re-exports what producers need: `stream`
  (`unfold`, `once`, `iter`), `StreamExt` (`.chain`, `.map`) and `time`
  (`sleep`, `interval`).

## Many viewers, one producer: the hub

```rust,ignore
// bound once, e.g. in a provider's register: app.bind(Stats(Hub::new(app.shutdown(), HubLimits::default())))
pub struct Stats(pub Hub<String>);

let subscription = stats.0
    .subscribe(format!("container:{id}"), move || docker_stats_stream(id))
    .map_err(|_| AppError::TooManyRequests { retry_after: 5 })?;
Ok(live.stream(subscription.into_events()).target(/* … */))
```

- The first subscriber of a key starts its producer; the others share it.
  The producer stops 5 s after the last one leaves (`HubLimits::idle_grace`).
- `hub.publish(&key, event)` sends to current subscribers, for example after
  a POST changed something.
- A viewer that falls behind gets `event: resync`: reload what the page
  shows. A producer whose own source lost events (an upstream broadcast
  that lagged, a reconnect to the source) yields `Event::resync()`, so pages
  handle both the same way.
- The hub knows no users. A key must hold everything that decides its data
  (`container:<id>`). Never share a key between users who should see
  different data.
- The hub lives in one process: each instance runs its own producers.

## In a script page

```js
window.rustcms.registerPage("docker/stats", {
  apiVersion: 1,
  mount(element, ctx) {
    const value = document.createElement("output");
    element.append(value);
    ctx.realtime.events(`/docker/containers/${encodeURIComponent(ctx.data.id)}/stats`, {
      onEvent(name, data) {
        if (name === "stats") value.textContent = `${data.cpu}%`;
        if (name === "resync") ctx.reload();
      },
      onState(state) { /* "open", "reconnecting", or "closed" with a status or reason */ },
    });
    return { update() {}, unmount() {} };
  },
});
```

`ctx.realtime.events(path, handlers)` opens the route under
`{admin}/api/x` and returns `{ close() }`.

- It is closed for you when the view is unmounted or the session changes
  (sign-in, sign-out, credentials rotated in any tab).
- It reconnects with backoff after a dropped connection or a server restart.
- It stops for good on 401, 403 or 404, and when the server ends it with
  `signed_out` or `forbidden`.
- It is built on `fetch`. Do not use `EventSource`: it hides statuses and
  reconnects on its own after a sign-out.

## What the server enforces

| | |
|---|---|
| Origin | an `Origin`, when sent, must be the panel's; `Sec-Fetch-Site`, when sent, `same-origin`; with neither (an old browser), a same-origin `Referer`. No CSRF token is needed |
| Access | the route's `AdminAccess` before anything runs (401, 403); `HEAD` is 405 |
| While open | every `revalidate_secs` (30): the session, the user and its credential version, its permissions and `target`. Delivery stops within `revalidate_secs + 5 s` of a sign-out, a password change, a disabled account or a lost permission (`event: closed`) |
| Limits | per instance: `max_connections` (1000), `max_per_user` (10), 429 beyond. Opens per user (60/min) and per IP (300/min) use the rate limiter, so with Redis they count across instances |
| Slow clients | a bounded queue (64 events, 1 MiB): a client that stops reading is dropped (`lagging`) and never holds the server |
| Shutdown | new streams get 503; open ones end with `closed: shutdown` within `shutdown_grace_secs` (5) |

Events end with `event: closed` and data `{"reason": …}`: one of
`signed_out`, `forbidden`, `shutdown`, `lagging` or `error`.

Each recheck reads the session, the user and the grants (about 3 queries),
plus what `target` does. At 1000 streams every 30 s that is about 100
queries per second: raise `revalidate_secs` or the database pool
(`database.max_connections`) accordingly.

Streams are not cached, carry the session's `Set-Cookie` like any admin
response, and never renew the session by themselves.

## WebSocket

```rust,ignore
// src/routes/admin.rs
AdminRoutes::new("/docker")
    .socket("/containers/{id}/exec", AdminAccess::can("docker.exec"), controllers::docker::exec)
```

```rust,ignore
// src/http/controllers/docker.rs
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
    (`rustcms.v1, rustcms.csrf.<token>`, which `ctx.realtime.socket` sends);
  - the open limits and a place, shared with streams.

  The `101` selects `rustcms.v1` only.
- `live.accept(session)` runs `session` once the connection is upgraded and
  the first check passed (the session, the user, the access, `target`).
  `session` gets an `AdminSocket`:
  - `recv()` gives `Text` or `Binary` messages, and `None` once the socket
    ended;
  - `send(message)` waits at most `send_timeout_secs` for a slow client (then
    the socket ends as `lagging`) and refuses a message over
    `max_message_bytes` (`SendError::TooLarge`);
  - `authorize()` checks again now and returns the fresh principal.
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
- Close codes: `4401 signed_out`, `4403 forbidden`, `1001 shutdown`,
  `4008 lagging`, `1009 too_big`, `1011 error` (also a session that panics),
  `1000` when the session returned. Delivering the code is best effort: a
  connection with unread input may be reset first.

### In a script page

```js
const socket = ctx.realtime.socket(`/docker/containers/${encodeURIComponent(ctx.data.id)}/exec`, {
  onOpen() { /* … */ },
  onMessage(data) { /* a string, or an ArrayBuffer for binary messages */ },
  onState(state) { /* "open", "reconnecting", or "closed" with a code and reason */ },
});
socket.send(new TextEncoder().encode("ls\n"));   // false when not open, too large, or 1 MiB is already queued
```

- Closed for you on unmount and on a session change.
- **No reconnect by default:** a new socket is a new session on the server
  (a new shell). With `{ reconnect: true }`:
  - it retries with backoff and a fresh token;
  - it stops on 4401 and 4403, and after 5 failed or short (under 30 s)
    connections;
  - a refused handshake (seen as 1006) checks the session once.
- Input is never queued for replay.
- xterm.js injects `<style>` elements, which the panel's CSP blocks. A page
  that embeds it needs its own workaround in its script. The CSP stays as it
  is.

### What the server enforces

| | |
|---|---|
| Origin | mandatory, exactly one of the panel's origins; `Sec-Fetch-Site`, when sent, `same-origin` |
| CSRF | the session's token as the `rustcms.csrf.<token>` subprotocol, beside `rustcms.v1`, nothing else |
| Access | the route's `AdminAccess` before anything runs (401, 403); a handler error before `accept` is an ordinary JSON error |
| While open | every `socket_revalidate_secs` (10) and at each `authorize()`: the session, the user and its version, the permissions and `target`. Work stops within `socket_revalidate_secs + 5 s` of a sign-out, an expired session, a password change, a disabled account or a lost permission |
| Limits | shared with streams: `max_connections`, `max_per_user`, the open rates; messages up to `max_message_bytes` (64 KiB) |
| Slow or flooding clients | 64 messages or 1 MiB queued each way; a client that floods a session, or stops reading, is dropped (`lagging`) |
| Keepalive | one ping at a time, 15 s after the last answer; no matching pong within 45 s ends the socket |
| Shutdown | new sockets get 503; open ones end with `1001` within `shutdown_grace_secs` |

Sockets never renew the session. Limits are per instance. Behind a proxy,
pass `Upgrade` and drop `Sec-WebSocket-Protocol` from access logs (see the
[production checklist](../security/production.md)).
