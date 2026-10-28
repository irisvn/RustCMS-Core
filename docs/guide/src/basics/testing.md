# Testing

`rustcms-testing` runs your application in tests: each test gets its own
database, and requests go through the full router (middleware, guards, CSRF)
without opening a socket. One test body runs on SQLite, PostgreSQL and MySQL.

## Quick start

Feature tests live in `tests/feature/`, one module per file, compiled as one
test binary. `tests/feature/main.rs` lists the modules and includes the shared
helpers of `tests/support/mod.rs` once:

```rust
// tests/feature/main.rs
#[path = "../support/mod.rs"]
mod support;

mod health;
// rustcms:tests
```

`rustcms make:test PostAdminTest` writes `tests/feature/post_admin_test.rs` and
adds it to `main.rs`. A test of the demo application:

<<< @/../../../examples/demo/tests/feature/health.rs#health-test [tests/feature/health.rs]

```sh
cargo test                      # every test (SQLite; PostgreSQL and MySQL when configured)
cargo test --test feature       # the feature tests only
RUSTCMS_TEST_BACKENDS=sqlite cargo test   # SQLite only: the fast inner loop
```

`db_test!` generates three tests, `health_and_ping::sqlite`, `…::postgres` and
`…::mysql`. With `migrate = Migrator` each database is migrated first.

## Full example

The demo's API test sends the same request anonymously, with an admin session
cookie (not an API credential: 401) and with an API token limited to reading,
and checks what each gets. It seeds the database first with `crate::support::seed`, which runs the
`database` seeder:

<<< @/../../../examples/demo/tests/feature/api.rs#api-access [tests/feature/api.rs]

Two concurrent requests for the last seat of a course, in the LMS example.
`CourseFactory` and `LearnerFactory` are the application's own
[factories](#factories); `enroll` and `active` are helpers of its
`tests/support/mod.rs`:

<<< @/../../../examples/lms/tests/feature/enrollments.rs#last-seat [tests/feature/enrollments.rs]

## `db_test!` and `TestDb`

```rust
db_test!(name, migrate = Migrator, |db: TestDb| async move { … });   // migrated database
db_test!(name, |db: TestDb| async move { … });                       // empty database
```

Each variant opens a fresh database, runs the body, then deletes the database.
A test that panics skips that clean-up: on PostgreSQL and MySQL its database
is left behind (drop leftover test databases now and then).

| Database | Each test gets |
|---|---|
| SQLite | a temporary file, opened with production options (foreign keys, WAL), so locking behaves like the real thing |
| PostgreSQL | a database of its own, from `RUSTCMS_TEST_PG` (an admin URL). With `migrate = M`, the migrated schema is built once as a template database (`rustcms_tpl_<hash of the migration names>`) and cloned per test |
| MySQL | a database of its own (`utf8mb4_0900_bin`), from `RUSTCMS_TEST_MYSQL` (an admin URL). With `migrate = M`, a migrated database is built once and its tables (with their foreign keys) and rows are copied per test |

The template is keyed by the migration **names**, not their contents: after
editing an unreleased migration in place, drop the `rustcms_tpl_…` databases.

`TestDb` can also be used directly:

| Method | Does |
|---|---|
| `TestDb::sqlite()` | a fresh SQLite file |
| `TestDb::empty(backend)` / `TestDb::postgres()` / `TestDb::mysql()` | a fresh empty database; `None` when the server's variable is not set |
| `TestDb::migrated::<M>(backend)` | a fresh database with every migration of `M` |
| `db.migrate::<M>()` | runs the migrations of `M`, on one connection as `migrate` does |
| `db.single_connection()` | the same database through one connection: tests that run migrations and read the schema between steps |
| `db.reopen()` | the same database through a pool of its own, as a second application instance would open it (its `cleanup` only closes that pool) |
| `db.conn()` / `db.url()` / `db.backend()` | the pool, its URL, the `Backend` (`Sqlite`, `Postgres`, `Mysql`) |
| `db.cleanup()` | closes the pool and deletes the database (`db_test!` does it) |

Tables built from entities (`Schema::create_table_from_entity`) go through
`rustcms_testing::create_table(db, &statement)`, which gives MySQL the same
instants as migrations (`DATETIME(6)`). Without `migrate = M`, also create the
`audit_logs` table (`rustcms::models::audit_log::Entity`): resource writes
record into it.

### Environment variables

| Variable | Does |
|---|---|
| `RUSTCMS_TEST_PG` | PostgreSQL admin URL, e.g. `postgres://postgres:postgres@localhost:5432/postgres` |
| `RUSTCMS_TEST_MYSQL` | MySQL admin URL, e.g. `mysql://root:secret@localhost:3306/mysql` |
| `RUSTCMS_TEST_BACKENDS` | runs only these variants (`sqlite`, `postgres`, `mysql`, comma-separated), and makes them required |
| `RUSTCMS_TEST_REQUIRE` | fails instead of skipping when a listed service is not configured (`postgres`, `mysql`, `s3`, `redis`) |
| `RUSTCMS_TEST_S3` | an S3 bucket, `http://host:port/bucket` (with `AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY`) |
| `RUSTCMS_TEST_SMTP`, `RUSTCMS_TEST_SMTP_TLS`, `RUSTCMS_TEST_SMTP_TLS_CA` | RustCMS's own tests against a real SMTP server (Mailpit) |
| `RUSTCMS_TEST_REDIS` | RustCMS's own tests of several instances (with `--features rustcms/redis`) |

Without `RUSTCMS_TEST_PG` or `RUSTCMS_TEST_MYSQL`, those variants print a skip
notice and pass. Set them in CI, with `RUSTCMS_TEST_REQUIRE=postgres,mysql`, so
a broken setup fails instead of passing as skipped. With
`RUSTCMS_TEST_BACKENDS`, an empty or unknown name fails, as does a backend that
`RUSTCMS_TEST_REQUIRE` asks for but the selection leaves out.

Test servers are faster in memory and without fsync, for example PostgreSQL
with `-c fsync=off -c synchronous_commit=off -c full_page_writes=off`, MySQL
with `--innodb-flush-log-at-trx-commit=0 --innodb-doublewrite=OFF
--innodb-flush-method=nosync --sync-binlog=0 --skip-log-bin`, data
directories on `tmpfs`. Never use these settings for data you keep.

For S3, `TestS3::from_env()` reads `RUSTCMS_TEST_S3` (`None` and a skip
notice when it is not set), and `configure` on that value points `[storage]`
at the bucket:

```rust
let Some(s3) = rustcms_testing::TestS3::from_env() else { return };
let config = s3.configure(rustcms_testing::test_config(db.url()));
let app = TestApp::with_config(bootstrap::build, config, db).await;
```

 S3Mock works: `docker run -p 9090:9090 -e
COM_ADOBE_TESTING_S3MOCK_STORE_INITIAL_BUCKETS=test adobe/s3mock:5.2.3`.

## `TestApp`

| Method | Does |
|---|---|
| `TestApp::new(build, db)` | builds the application with `test_config(db.url())` on `db` |
| `TestApp::with_config(build, config, db)` | the same with your configuration |
| `rustcms_testing::test_config(url)` | the app's `config/` files, then `app.env = "testing"`, a random `APP_KEY`, the database URL and `mail.driver = "memory"` (these override environment variables) |
| `app.state()` / `app.db()` | the `AppState` / the `TestDb` |
| `app.get(uri)` / `app.post_json(uri, &body)` / `app.json(method, uri, &body)` | sends a request from `127.0.0.1`, without credentials |
| `app.request(request)` / `app.request_from(peer, request)` | sends any `Request<Body>`, from `127.0.0.1` or from `peer` (proxy tests) |
| `app.router()` | the full router, to read a response yourself |
| `app.listen()` | the address of a real listener on `127.0.0.1` (started once, stopped with the app) |
| `app.admin_api(path)` | `<admin_path>/api/<path>` |
| `app.make_super_admin(&user)` | gives the user the `super_admin` role of every guard, through `RbacService` |
| `app.mails()` / `app.take_mails()` | mails sent with the `memory` driver (feature `mail`) |

`build` is any `FnOnce(Config) -> AppBuilder`, usually `bootstrap::build`. A
server-sent events response is not read (it would never end): its status and
headers come back with an empty body.

## Signing in

| Helper | Use for |
|---|---|
| `app.client()` | an anonymous browser: sends `Origin: <APP_URL>`, keeps cookies |
| `app.acting_as(&user)` | authorization tests: a real admin session row, no password needed |
| `app.login_as(email, password)` | authentication tests: the real `csrf` and `login` HTTP flow |
| `app.with_token(&user, &["posts.view"])` | API tests: a real API token named `test` with those abilities (`&["*"]`: all) |

`acting_as` and `login_as` sign in to the **admin** guard only; `with_token`
authenticates the **API** guard only. Every request goes through the real
guards, so CSRF, expiry and `auth_version` checks apply.

`UserFactory` creates users:

| Method | Does |
|---|---|
| `UserFactory::new()` | an active user named `User <n>`, with a unique email |
| `.name(..)` / `.email(..)` | sets them (`email` only with `create`, not `create_many`) |
| `.password("secret password")` | hashes it, so the user can sign in with `login_as` |
| `.inactive()` | a disabled user |

Without `.password(..)`, the stored hash matches no password: use `acting_as`.

## `TestClient`

| Method | Does |
|---|---|
| `client.get(uri)` / `client.post_json(uri, &body)` | sends with the client's cookies, `Origin`, CSRF token and bearer token |
| `client.send(method, uri, Some(&body))` | any method, optional JSON body |
| `client.request(request)` | any `Request<Body>` |
| `client.fetch_csrf()` | `GET <admin>/api/auth/csrf`, remembers and returns the token |
| `client.csrf_token()` / `client.cookie(name)` | the token it sends / a cookie it holds |
| `client.set_csrf(Some(..))` / `client.set_csrf(None)` | overrides the CSRF token / sends none |
| `.origin(Some(..))` / `.origin(None)` | another `Origin` / none |
| `.from(peer)` | the TCP peer the app sees |
| `.languages("vi")` | sends `Accept-Language` |
| `.bearer(token)` | sends `Authorization: Bearer <token>` |
| `client.socket(path)` / `client.socket_with(path, protocols, headers)` | opens a WebSocket (below) |

The builder methods (`origin`, `from`, `languages`, `bearer`) take the client
by value: `app.client().origin(None)`.

## `TestResponse`

| Member | Does |
|---|---|
| `.status`, `.headers`, `.body` | the response's parts (`body` is `Bytes`) |
| `.json()` | the body as `serde_json::Value` (panics if not JSON) |
| `.text()` | the body as text |
| `.header(name)` | a header as text |
| `.assert_status(code)` | asserts the status; prints the body on failure |
| `.assert_json("/pointer", value)` | asserts the value at a JSON pointer |
| `.assert_validation_error("field")` | asserts a 422 with a message for `field` |

The assertions return `&Self`, so they chain.

## WebSockets

An upgrade needs a real connection: `client.socket(path)` starts a listener
for the app on `127.0.0.1` (once, stopped with the app) and opens the socket
with the client's cookies, `Origin` and CSRF subprotocol
(`rustcms.v1, rustcms.csrf.<token>`), as the panel does. The demo's echo
socket:

<<< @/../../../examples/demo/tests/feature/live.rs#socket-test [tests/feature/live.rs]

| Member of `TestSocket` | Does |
|---|---|
| `send_text(text)` / `send_binary(bytes)` | sends a message |
| `next_data(wait)` | the next text or binary message within `wait`, or `None` |
| `closed(wait)` | reads until the server's close frame: `Some((code, reason))` |
| `protocol` | the subprotocol the server selected |
| `inner` | the raw tokio-tungstenite stream |

A refused handshake comes back as `Err(Refused { status, body })`.
`socket_with(path, protocols, headers)` sends exactly these subprotocols
(`None`: no header) and extra headers.

## Authorization tests

Write the expected outcome of every route for every actor, and test the
application against that table. Derive nothing from the code under test, and
fail when a route has no row.

For [admin routes](../admin/admin-routes.md),
`rustcms_testing::admin_routes::{Case, assert_access}` does this. Each `Case`
is a request (method and a concrete path under `<admin>/api/x`) and the status
each actor must get; `assert_access` sends every case as every actor (unsafe
methods with an empty JSON body) and **fails when a route of the
`AdminRoutes` has no case**:

<<< @/../../../examples/demo/tests/feature/live.rs#assert-access [tests/feature/live.rs]

For API routes, keep a similar table (a TOML file next to your tests works) and
loop over it with `with_token` clients.

## Mail and the queue

`test_config` always sets the `memory` mail driver, so nothing leaves the test.
To read what was sent, enable the `mail` feature of `rustcms-testing`:

```toml
[dev-dependencies]
rustcms-testing = { …, features = ["mail"] }
```

Then `app.mails()` lists the mails sent so far and `app.take_mails()` takes
them. Mails queued with `send_later` are sent by the queue: run it with
`rustcms::queue::run_until_idle(state, queues, max)`, which returns how many
jobs ran. From the LMS:

<<< @/../../../examples/lms/tests/feature/enrollments.rs#queued-mail [tests/feature/enrollments.rs]

## Factories

Implement `Factory::definition` for your entities ([Database](database.md#factories));
`create` and `create_many` insert and panic on a database error, `try_create`
and `try_create_many` return the error. `rustcms_testing::Factory` is the same
trait as `rustcms::database::Factory`. The sequence number `n` is unique per
process, so unique columns stay unique.

`app.make_super_admin(&user)` gives full access, for tests that need it; when
the test is about authorization, prefer a role with the exact permissions.

## Test doubles

`TestApp::new` (and `with_config`) takes any `FnOnce(Config) -> AppBuilder`,
so a closure can bind a double next to your real providers. A sketch:

```rust
struct Docker(Arc<dyn DockerApi>); // a newtype: `bind` keys services by type

let fake = Arc::new(FakeDocker::default());
let double = fake.clone();
let app = TestApp::new(
    move |config| {
        let mut app = bootstrap::build(config);
        app.bind(Docker(double));    // replaces the one bound by your provider
        app
    },
    db,
)
.await;
```

`bind` replaces a service of the **same concrete type**: bind a newtype
around `Arc<dyn Trait>` (not the trait object itself) and resolve it with
`Inject<Docker>`, so the double and the real service share one key.

## Pitfalls

- `acting_as` gives an admin session: API routes answer 401 to it. Use
  `with_token` for the API.
- A user made by `UserFactory::new()` without `.password(..)` cannot sign in
  with `login_as`.
- `mails()` needs the `mail` feature of `rustcms-testing` and a registered
  `MailProvider`.
- A test that leaves PostgreSQL or MySQL out passes with a skip notice unless
  `RUSTCMS_TEST_REQUIRE` or `RUSTCMS_TEST_BACKENDS` names them.
- After editing a migration in place, drop the `rustcms_tpl_…` template
  databases, or tests keep the old schema.

## Related pages

- [Database & migrations](database.md)
- [Routing](routing.md)
- [Admin routes](../admin/admin-routes.md)
- [Authentication](../security/authentication.md)
- [Queues](../digging-deeper/queues.md)
