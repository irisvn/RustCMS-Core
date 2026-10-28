# Request lifecycle

What happens between running your binary and your handler answering a request:
which commands load what, the order in which `serve` starts, the layers every
request goes through, and how the server stops. Read it when you add
middleware, debug a start-up failure, or plan a deployment.

## Starting

`src/main.rs` hands your `build` function and your migrations to the console:

```rust
fn main() -> std::process::ExitCode {
    rustcms::Console::<blog::database::migrations::Migrator>::new(blog::bootstrap::build).run_blocking()
}
```

For `serve`, in order:

1. `.env` is loaded, then the [configuration](../getting-started/configuration.md)
   (`config/*.toml`, the environment) is read and checked (`APP_KEY`,
   locales);
2. logging starts (`log.filter` or `RUST_LOG`, `log.json`): from here on,
   failures are logged;
3. `build(config)` assembles the `AppBuilder`: each provider's `register` runs
   when it is added, binding services and adding routes, middleware,
   commands, jobs and schedules. Framework providers check their sections
   here (`[auth]`, `[storage]`, `[mail]`) and stop with a message naming the
   key;
4. the database pool connects; on MySQL, the database setup is checked
   (collation, engine, `sql_mode`, time zone);
5. Redis connects when `redis.url` is set; `[queue]` and `[schedule]` are
   checked; the keyring is built from `APP_KEY` and `APP_PREVIOUS_KEYS`;
6. each provider's `boot` runs, in registration order. The framework's
   providers check what needs the database or other services, and start their
   housekeeping (pruning expired sessions, unused media, old exports);
7. the router is assembled: your routes, your middleware, then the framework
   stack below;
8. the listener binds `app.listen` (or `serve --listen`), the queue workers
   and the scheduler start, then the server accepts requests.

An error in steps 1–7 stops the command before anything is served.

### What each command loads

| Command | Configuration | Assembles (`register`) | Database | Boots (`boot`) |
|---|---|---|---|---|
| `key:generate` | no (writes `.env`) | no | no | no |
| `migrate`, `migrate:rollback`, `migrate:refresh` | yes | no | connects, MySQL check | no |
| `migrate:fresh` | yes | with `--seed` | connects, MySQL check | with `--seed` |
| `migrate:status` | yes | no | connects | no |
| `route:list` | yes | yes | no | no |
| `db:seed` | yes | yes | yes | yes |
| `serve` | yes | yes | yes | yes, then workers, scheduler and HTTP |
| commands of providers and the application (`make:user`, `queue:work`, …) | yes | yes | yes | yes |

Queue workers and the scheduler start automatically only in `serve`: a
seeder or a console command does not start them. Two commands do that work on
purpose: `queue:work` runs workers alone, and `schedule:run <name>` runs one
schedule by hand (a task runs in the command, a job is queued). A
provider must not start its own background loop in `boot`; use
[`AppBuilder::schedule`](../digging-deeper/scheduling.md) for periodic work.

In tests and tools, `AppBuilder::build_with_db(db)` does steps 4–7 on an
existing connection (the testing kit uses it), `App::router()` returns the
complete router, and `App::serve_on(listener, stop)` serves on a listener you
bound until `stop` completes, then shuts down as `serve` does.

## The HTTP stack

Every request goes through, outermost first:

1. **Request id**: a client's `x-request-id` is kept when it is sent once and
   is 1–64 characters of letters, digits, `.`, `_`, `-` and `:`; otherwise a
   new UUID replaces it. It is returned on the response.
2. **Tracing**: one span per request with method, path and request id.
3. **Client IP**: the `ClientIp` extractor; see [proxies](#client-ip-and-proxies).
4. **Timeout**: `http.timeout_secs`, then 408 with an empty body.
   `rustcms::http::request_deadline()` returns the instant the current request
   will be cut off, so a handler can finish optional work in time.
   Inside it, a **panic** in a handler, a guard or your middleware answers
   the standard 500 (`internal_error`), logged with the request id (never the
   panic's message); the application keeps serving.
5. **Body limit**: `http.body_limit`, then 413, on every request body (a
   larger `Content-Length` is refused before the handler runs; chunked bodies
   stop at the limit). A route may have its own
   (`app.body_limit(method, path, bytes)`).
6. **Your middleware** (`app.middleware(…)`), around the application's routes.
7. **Guard**, chosen by path ([Authentication](../security/authentication.md)):
   - the panel's static files: no session is loaded;
   - under `auth.admin_path`: the admin session. Changes (`POST`, `PUT`, …)
     need an allowed `Origin` (or, when the request has no `Origin`, a
     `Referer` from an allowed origin) and the session's `X-CSRF-Token`.
     Responses of
     the admin API, and any response that sets the session cookie, get
     `Cache-Control: private, no-store`;
   - elsewhere: the API guard (`Authorization: Bearer …`, API token or JWT).
     A credential that is present but invalid is a 401, with no fallback.
8. **Route access**: the `Access` of an [API route](../basics/routing.md) or
   the `AdminAccess` of an [admin route](../admin/admin-routes.md), checked
   before any extractor reads the request. A public API route
   (`Access::public()`) answers anyone; a route that needs a user answers 401
   without one and 403 without the permission. Admin routes always need a
   signed-in admin.
9. **Your handler**: extractors, then your code.

Unknown routes answer 404 and wrong methods 405, both as JSON. `GET /health`
returns `{"status":"ok","database":"ok"}`, or 503 with
`{"status":"degraded","database":"unavailable"}` when the database does not
answer. `/health` and the framework's last 404/405 fallbacks are added after
your middleware, so neither they nor the guard see those requests. Under
`auth.admin_path` it differs: unknown admin API paths reach a catch-all route
behind the guard (JSON 404), and an unknown panel page (`GET`) gets the
panel's HTML, which shows its own "not found".

### Adding middleware

`AppBuilder::middleware` wraps every application route once all routes are
added, inside the framework stack (request id, client IP and limits are
already applied). A sketch, in a provider's `register`:

```rust
use rustcms::axum::extract::Request;
use rustcms::axum::http::HeaderValue;
use rustcms::axum::middleware::{self, Next};
use rustcms::axum::response::Response;

async fn frame_options(req: Request, next: Next) -> Response {
    let mut response = next.run(req).await;
    response.headers_mut().insert("x-frame-options", HeaderValue::from_static("DENY"));
    response
}

app.middleware(|router, _state| router.layer(middleware::from_fn(frame_options)));
```

The closure gets the router and the `AppState`. Middleware added later wraps
the earlier ones, so it runs first. The guard is itself middleware added by
`AuthProvider`, which comes first in the provider list: your middleware
usually runs **before** the guard, and a request there has no authenticated
user yet. Do not authorize in middleware; declare the access on the route.

## Responses and errors

Handlers return `AppResult<T>`. Errors become JSON in one format
([Errors](../basics/errors.md)); internal details are logged with the request
id, never returned. The framework's 408 timeout response has an empty body.

## Client IP and proxies

`X-Forwarded-For` is ignored unless the TCP peer is in `http.trusted_proxies`.
Then the client is the right-most address that is not a trusted proxy, so a
client cannot choose its address by sending the header itself. A header with
a value that is not an IP address is ignored as a whole: the client is the
peer.

```toml
# config/http.toml
trusted_proxies = ["10.0.0.0/8"]
```

Rate limits, the audit log and sign-in throttling use this address.

## Stopping

`serve` stops on Ctrl-C or SIGTERM (or, with `serve_on`, when `stop`
completes):

1. the server stops accepting connections, the queue workers stop claiming
   jobs, and open event streams and WebSockets are told to close;
2. streams and sockets get `realtime.shutdown_grace_secs` (5 s), then are
   aborted. The server waits the same grace for requests in progress, then
   stops waiting: a request still running ends when it finishes or when the
   process exits;
3. running jobs get `queue.shutdown_secs` (30 s); a job still running then is
   stopped and given back to the queue, except a job with `AT_MOST_ONCE`,
   which is marked failed so it never runs twice. Scheduled runs get
   `realtime.shutdown_grace_secs`.

`serve` returns when all three are done. Budget the stop timeout of your
service manager accordingly ([Deployment](../getting-started/deployment.md#stopping)).

## Related pages

- [Service providers & container](providers.md)
- [Deployment](../getting-started/deployment.md#what-serve-runs)
- [Routing](../basics/routing.md) and [Errors](../basics/errors.md)
- [Authentication](../security/authentication.md)
