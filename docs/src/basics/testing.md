# Testing

`rustcms-testing` gives each test its own database and drives the full router
(middleware included) without opening a socket.

```rust,ignore
use rustcms_testing::{TestApp, TestDb, UserFactory, Factory, db_test};

db_test!(lists_posts, migrate = Migrator, |db: TestDb| async move {
    UserFactory::new().create(db.conn()).await;
    let app = TestApp::new(app::build, db).await;
    app.get("/api/v1/posts").await.assert_status(StatusCode::OK).assert_json("/meta/total", json!(0));
});
```

`db_test!` generates two tests, `…::sqlite` and `…::postgres`.

## Databases

- **SQLite:** a temp file per test, opened with production options (foreign
  keys, WAL), so locking behaves like the real thing.
- **PostgreSQL:** set `RUSTCMS_TEST_PG` to an admin URL, e.g.
  `postgres://postgres:postgres@localhost:55439/postgres`. Each test gets its own
  database. With `migrate = M`, the migrated schema is built once as a template
  database and cloned per test. After editing an unreleased migration in place,
  drop the templates (`just test-reset-db`).

Without `RUSTCMS_TEST_PG`, the PostgreSQL variants print a skip notice and pass.
CI always sets it.

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

Keep a hand-written table of expected outcomes (route × actor → status), like
`fixtures/authz/auth.toml`, and test the application against it. Derive
nothing from the code under test, and fail when a route has no row.

## Factories

Implement `Factory::definition` for your entities; `create` and `create_many`
insert. Sequence numbers are unique per process, so unique columns stay unique.

`app.make_super_admin(&user)` gives a user the `super_admin` role of every guard
(through `RbacService`), for tests that need full access; prefer a role with the
exact permissions when the test is about authorization.
