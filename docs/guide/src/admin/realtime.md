# Realtime events

A panel page can show live data (stats, logs, progress) through **server-sent
events** from an [admin route](admin-routes.md). The stream runs on the
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

WebSocket, for commands and terminals, comes later. Until then, send commands
as POST admin routes.
