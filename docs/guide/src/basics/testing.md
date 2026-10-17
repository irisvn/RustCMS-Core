# Testing

`rustcms-testing` gives each test its own database and drives the full router
(middleware included) without opening a socket.

Feature tests live in `tests/feature/`, one module per file, compiled as one
test binary (`tests/feature/main.rs` lists them; `cargo test --test feature`
runs them alone). Helpers shared by suites go in `tests/support/mod.rs`,
included with `#[path = "../support/mod.rs"] mod support;`.
`rustcms make:test PostAdminTest` writes `tests/feature/post_admin_test.rs`
and adds it to `main.rs`.

```rust,ignore
use blog::bootstrap;
use blog::database::migrations::Migrator;
use rustcms_testing::{TestApp, TestDb, UserFactory, Factory, db_test};

db_test!(lists_posts, migrate = Migrator, |db: TestDb| async move {
    let app = TestApp::new(bootstrap::build, db).await;
    let user = UserFactory::new().create(app.state().db()).await;
    // Without credentials the route is closed; with a token holding the permission it answers.
    app.get("/api/v1/posts").await.assert_status(StatusCode::UNAUTHORIZED);
    app.make_super_admin(&user).await;
    let client = app.with_token(&user, &["posts.view_any"]).await;
    client.get("/api/v1/posts").await.assert_status(StatusCode::OK).assert_json("/meta/total", json!(0));
});
```

`db_test!` generates three tests, `…::sqlite`, `…::postgres` and `…::mysql`.

## Databases

- **SQLite:** a temp file per test, opened with production options (foreign
  keys, WAL), so locking behaves like the real thing.
- **PostgreSQL:** set `RUSTCMS_TEST_PG` to an admin URL, e.g.
  `postgres://postgres:postgres@localhost:55439/postgres`. Each test gets its own
  database. With `migrate = M`, the migrated schema is built once as a template
  database and cloned per test. After editing an unreleased migration in place,
  drop the template databases (named `rustcms_tpl_…`): the template is keyed by
  the migration names, not their contents.

- **MySQL:** set `RUSTCMS_TEST_MYSQL` to an admin URL, e.g.
  `mysql://root:mysql@localhost:53306/mysql`. Each test gets its own database
  (`utf8mb4_0900_bin`); with `migrate = M`, a migrated template is built once and
  its tables (with their foreign keys) and rows are copied per test.
- Tables built from entities (`Schema::create_table_from_entity`) go through
  `rustcms_testing::create_table`, which gives MySQL the same instants as
  migrations (`DATETIME(6)`). Without `migrate = M`, also create the
  `audit_logs` table (`rustcms::models::audit_log::Entity`): resource writes
  record into it.

- **S3:** `rustcms_testing::TestS3::from_env()` reads `RUSTCMS_TEST_S3`
  (`http://host:port/bucket`, e.g. S3Mock: `docker run -p 59000:9090 -e
  COM_ADOBE_TESTING_S3MOCK_STORE_INITIAL_BUCKETS=test adobe/s3mock:5.2.3`)
  and `TestS3::configure(config)` points `[storage]` at it.

Without `RUSTCMS_TEST_PG` / `RUSTCMS_TEST_MYSQL`, those variants print a skip
notice and pass, unless `RUSTCMS_TEST_REQUIRE` lists them (`postgres,mysql`):
then they fail. Set them in CI so every test runs on the three databases.

An application that runs on one database can test only that one:
`RUSTCMS_TEST_BACKENDS=mysql` (any of `sqlite,postgres,mysql`) runs those
variants and makes them required, so a missing server fails instead of
skipping. An empty or unknown name fails, as does a backend that
`RUSTCMS_TEST_REQUIRE` asks for but the selection leaves out. Unset, all
three run as before.

## Test doubles

`TestApp::new` (and `with_config`) takes any `FnOnce(Config) -> AppBuilder`,
so a closure can bind a double next to your real providers:

```rust,ignore
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

## HTTP helpers

| Helper                                   | Does                                   |
|------------------------------------------|----------------------------------------|
| `app.get(uri)`, `app.post_json(uri, &v)` | send a request from `127.0.0.1`        |
| `app.request_from(peer, request)`        | choose the TCP peer (proxy tests)      |
| `.assert_status(code)`                   |                                        |
| `.assert_json("/pointer", value)`        | JSON pointer assertion                 |
| `.assert_validation_error("field")`      | 422 with a message for `field`         |

## Signing in

| Helper                                   | Use for                                                       |
|------------------------------------------|---------------------------------------------------------------|
| `app.acting_as(&user)`                   | authorization tests: real session row, no password needed     |
| `app.login_as(email, password)`          | authentication tests: the real `csrf` + `login` HTTP flow     |
| `app.with_token(&user, &["posts.view"])` | API tests: a real token with those abilities                  |
| `app.client()`                           | anonymous browser (sends `Origin`, keeps cookies)             |

Clients keep cookies, send `Origin: <APP_URL>` and the CSRF token, and every
request goes through the real guards, so CSRF, expiry and `auth_version`
checks apply. Adjust with `.origin(None)`, `.set_csrf(..)`, `.from(peer)`.

`UserFactory::new().password("secret")` creates users that can sign in.

## Authorization oracles

Keep a hand-written table of expected outcomes (route × actor → status), for
example in a TOML file next to your tests, and test the application against it. Derive
nothing from the code under test, and fail when a route has no row.

## Mail

With the `mail` feature of `rustcms-testing`, `test_config` sets the `memory`
mail driver: `app.mails()` lists what was sent, `app.take_mails()` takes it.
Run the queue (`queue::run_until_idle`) to send mails queued with
`send_later`. `RUSTCMS_TEST_SMTP=127.0.0.1:51025` (Mailpit, in
`docker compose`) enables the tests against a real SMTP server;
`RUSTCMS_TEST_SMTP_TLS=localhost:51026` with `RUSTCMS_TEST_SMTP_TLS_CA` (the
`mailpit-tls` service, its certificate made by `just test-tls-cert`) the
tests of STARTTLS, authentication and certificate checks.

## Factories

Implement `Factory::definition` for your entities; `create` and `create_many`
insert. Sequence numbers are unique per process, so unique columns stay unique.

`app.make_super_admin(&user)` gives a user the `super_admin` role of every guard
(through `RbacService`), for tests that need full access; prefer a role with the
exact permissions when the test is about authorization.
