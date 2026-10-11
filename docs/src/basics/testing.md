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

Authentication helpers (`acting_as`, `login_as`) arrive with the guards in M2.

## Factories

Implement `Factory::definition` for your entities; `create` and `create_many`
insert. Sequence numbers are unique per process, so unique columns stay unique.
