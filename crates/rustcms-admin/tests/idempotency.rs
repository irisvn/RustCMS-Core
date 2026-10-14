//! Idempotency keys: a create or action retried with the same key and request
//! runs once; the replay is re-rendered for the user now. Both databases.

mod support;

use std::sync::Arc;

use axum::http::{Method, StatusCode};
use rustcms_admin::{Ctx, Data, DynResource};
use rustcms_core::AppError;
use rustcms_testing::{TestDb, db_test};
use sea_orm::{EntityTrait, PaginatorTrait};
use serde_json::{Value, json};
use support::{Fixture, PrincipalExt, admin, editor, note, post};

fn data(value: Value) -> Data {
    value.as_object().expect("object").clone()
}

fn keyed(ctx: Ctx, key: &str) -> Ctx {
    ctx.with_idempotency_key(Some(key.to_owned()))
}

async fn fixture(db: TestDb) -> Fixture {
    db.migrate::<rustcms_migration::Migrator>().await;
    Fixture::new(db).await
}

async fn receipts(fx: &Fixture) -> u64 {
    use sea_orm::{ConnectionTrait, Statement};
    let backend = fx.db.conn().get_database_backend();
    let row = fx
        .db
        .conn()
        .query_one_raw(Statement::from_string(backend, "SELECT COUNT(*) AS n FROM idempotency_keys"))
        .await
        .unwrap()
        .unwrap();
    row.try_get::<i64>("", "n").unwrap() as u64
}

db_test!(a_retried_create_runs_once, |db: TestDb| async move {
    let fx = fixture(db).await;
    let notes = fx.resource("notes");
    let ctx = keyed(fx.ctx(admin()), "create-1");
    let first = notes.create(&ctx, data(json!({"title": "Once"}))).await.unwrap();
    let again = notes.create(&ctx, data(json!({"title": "Once"}))).await.unwrap();
    assert_eq!(first.key, again.key);
    assert_eq!(again.data["title"], json!("Once"), "re-rendered from the record");
    assert_eq!(note::Entity::find().count(fx.db.conn()).await.unwrap(), 1);

    // Same key, another request: refused, nothing written.
    let err = notes.create(&ctx, data(json!({"title": "Twice"}))).await.unwrap_err();
    assert!(matches!(&err, AppError::Validation(v) if v.errors.contains_key("idempotency_key")), "{err:?}");
    // Reverse check: without a key, a second create is a second record.
    let plain = fx.ctx(admin());
    notes.create(&plain, data(json!({"title": "Plain"}))).await.unwrap();
    notes.create(&plain, data(json!({"title": "Plain 2"}))).await.unwrap();
    assert_eq!(note::Entity::find().count(fx.db.conn()).await.unwrap(), 3);
});

db_test!(keys_belong_to_one_user_and_resource, |db: TestDb| async move {
    let fx = fixture(db).await;
    let posts = fx.resource("posts");
    let body = || data(json!({"title": "Same"}));
    let a = posts.create(&keyed(fx.ctx(editor("7")), "shared"), body()).await.unwrap();
    let b = posts.create(&keyed(fx.ctx(editor("8")), "shared"), body()).await.unwrap();
    assert_ne!(a.key, b.key, "another user's key is independent");
    let n = fx.resource("notes").create(&keyed(fx.ctx(admin()), "shared"), body()).await;
    assert!(n.is_ok(), "another resource's key is independent");
    assert_eq!(post::Entity::find().count(fx.db.conn()).await.unwrap(), 2);
});

db_test!(a_failed_write_releases_its_key, |db: TestDb| async move {
    let fx = fixture(db).await;
    let tags = fx.resource("tags");
    let ctx = keyed(fx.ctx(admin()), "boom");
    // `after_save` fails on "boom": the claim rolls back with the write.
    assert!(tags.create(&ctx, data(json!({"slug": "a", "label": "boom"}))).await.is_err());
    assert_eq!(receipts(&fx).await, 0);
    assert!(tags.create(&ctx, data(json!({"slug": "a", "label": "boom"}))).await.is_err(), "runs again, not replayed");
    tags.create(&ctx, data(json!({"slug": "a", "label": "fine"}))).await.unwrap();
    assert_eq!(receipts(&fx).await, 1);
});

db_test!(replays_are_authorized_and_rendered_now, |db: TestDb| async move {
    let fx = fixture(db).await;
    let notes = fx.resource("notes");
    let writer = editor("7").with(&["notes.create", "notes.view_any", "notes.view"]);
    let created = notes.create(&keyed(fx.ctx(writer.clone()), "k"), data(json!({"title": "Mine"}))).await.unwrap();

    // Access revoked meanwhile: the replay is refused like a new request.
    let revoked = keyed(fx.ctx(writer.clone().without_all()), "k");
    assert!(matches!(notes.create(&revoked, data(json!({"title": "Mine"}))).await, Err(AppError::Forbidden)));

    // No longer visible: the replay carries the key only.
    let blind = keyed(fx.ctx(editor("7").without_all().with(&["notes.create"])), "k");
    let replay = notes.create(&blind, data(json!({"title": "Mine"}))).await.unwrap();
    assert_eq!(replay.key, created.key);
    assert!(replay.data.is_empty() && replay.title.is_none(), "{replay:?}");
});

db_test!(a_retried_action_reports_the_first_run, |db: TestDb| async move {
    let fx = fixture(db).await;
    let notes = fx.resource("notes");
    let admin_ctx = fx.ctx(admin());
    let note = notes.create(&admin_ctx, data(json!({"title": "Act"}))).await.unwrap();
    let ctx = keyed(admin_ctx.clone(), "act-1");
    let keys = vec![note.key.clone(), "999".to_owned()];
    let first = notes.run_action(&ctx, "touch", &keys, Data::new()).await.unwrap();
    assert!(!first.replayed);
    // The record is gone now: a real second run would find it missing.
    notes.delete(&admin_ctx, &note.key).await.unwrap();
    let again = notes.run_action(&ctx, "touch", &keys, Data::new()).await.unwrap();
    assert!(again.replayed);
    assert_eq!(first.processed, vec![note.key.clone()]);
    // Not run again, and what is hidden now is not disclosed (nor called missing).
    assert!(again.processed.is_empty(), "{again:?}");
    assert_eq!(again.missing, first.missing);
    // Other keys, other request: refused.
    let err = notes.run_action(&ctx, "touch", std::slice::from_ref(&note.key), Data::new()).await.unwrap_err();
    assert!(matches!(err, AppError::Validation(_)));
});

db_test!(expired_keys_are_pruned_and_start_over, |db: TestDb| async move {
    use sea_orm::{ConnectionTrait, sea_query::Expr};
    let fx = fixture(db).await;
    let notes = fx.resource("notes");
    let ctx = keyed(fx.ctx(admin()), "old");
    notes.create(&ctx, data(json!({"title": "First"}))).await.unwrap();
    let past = chrono::Utc::now() - chrono::Duration::hours(1);
    let update = sea_orm::sea_query::Query::update()
        .table(sea_orm::sea_query::Alias::new("idempotency_keys"))
        .value(sea_orm::sea_query::Alias::new("expires_at"), Expr::value(past))
        .to_owned();
    fx.db.conn().execute(&update).await.unwrap();
    assert_eq!(rustcms_admin::idempotency::prune(fx.db.conn()).await.unwrap(), 1);
    // After the receipt is gone, the key is a new request.
    notes.create(&ctx, data(json!({"title": "Second"}))).await.unwrap();
    assert_eq!(note::Entity::find().count(fx.db.conn()).await.unwrap(), 2);
});

/// Notes whose create waits while `title` is "slow" (a long write holding its claim).
struct SlowNotes;

#[async_trait::async_trait]
impl rustcms_admin::Resource for SlowNotes {
    type Entity = note::Entity;
    const SLUG: &'static str = "slow_notes";

    fn meta() -> rustcms_admin::ResourceMeta {
        rustcms_admin::ResourceMeta::new("Note")
    }

    fn fields() -> Vec<rustcms_admin::Field> {
        vec![rustcms_admin::Field::make("title")]
    }

    async fn mutate_before_create(
        _ctx: &Ctx,
        _txn: &sea_orm::DatabaseTransaction,
        data: Data,
    ) -> rustcms_core::AppResult<Data> {
        if data.get("title") == Some(&json!("slow")) {
            tokio::time::sleep(std::time::Duration::from_secs(7)).await;
        }
        Ok(data)
    }
}

db_test!(a_concurrent_request_with_the_same_key_waits_then_conflicts, |db: TestDb| async move {
    let fx = fixture(db).await;
    let registry = rustcms_admin::Registry::new().register::<SlowNotes>().finish();
    let slow: Arc<dyn DynResource> = registry.get("slow_notes").unwrap().clone();
    let ctx = keyed(Ctx::new(fx.db.conn().clone(), admin(), registry.clone()), "busy");
    let (first_slow, first_ctx) = (slow.clone(), ctx.clone());
    let first = tokio::spawn(async move { first_slow.create(&first_ctx, data(json!({"title": "slow"}))).await });
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    let started = std::time::Instant::now();
    let err = slow.create(&ctx, data(json!({"title": "slow"}))).await.unwrap_err();
    assert!(matches!(err, AppError::Conflict { code: "write_conflict", .. }), "{err:?}");
    assert!(started.elapsed() < std::time::Duration::from_secs(7), "bounded wait");
    let created = first.await.unwrap().unwrap();
    // Afterwards, the same retry replays the committed create.
    assert_eq!(slow.create(&ctx, data(json!({"title": "slow"}))).await.unwrap().key, created.key);
    assert_eq!(note::Entity::find().count(fx.db.conn()).await.unwrap(), 1);
});

db_test!(concurrent_identical_creates_make_one_record, |db: TestDb| async move {
    let fx = fixture(db).await;
    let notes = fx.resource("notes");
    let ctx = keyed(fx.ctx(admin()), "race");
    let tasks: Vec<_> = (0..4)
        .map(|_| {
            let (notes, ctx) = (notes.clone(), ctx.clone());
            tokio::spawn(async move { notes.create(&ctx, data(json!({"title": "Race"}))).await })
        })
        .collect();
    let mut keys = Vec::new();
    for task in tasks {
        match task.await.unwrap() {
            Ok(envelope) => keys.push(envelope.key),
            // SQLite may report a busy database to a waiter: retryable, nothing written.
            Err(AppError::Conflict { .. }) => {}
            Err(other) => panic!("{other:?}"),
        }
    }
    keys.dedup();
    assert_eq!(keys.len(), 1, "every success names the same record");
    assert_eq!(note::Entity::find().count(fx.db.conn()).await.unwrap(), 1);
});

db_test!(the_header_is_validated_over_http, |db: TestDb| async move {
    let (app, users) = support::app::setup(db).await;
    let client = app.acting_as(&users.admin).await;
    let url = app.admin_api("resources/notes/records");
    let send = |key: &'static str| {
        let request = axum::http::Request::builder()
            .method(Method::POST)
            .uri(url.clone())
            .header("idempotency-key", key)
            .header("content-type", "application/json")
            .body(axum::body::Body::from(json!({"title": format!("t-{key}")}).to_string()))
            .unwrap();
        client.request(request)
    };
    send("bad key!").await.assert_status(StatusCode::UNPROCESSABLE_ENTITY).assert_validation_error("idempotency_key");
    let first = send("good-key").await;
    first.assert_status(StatusCode::CREATED);
    let again = send("good-key").await;
    again.assert_status(StatusCode::CREATED);
    assert_eq!(first.json()["data"]["key"], again.json()["data"]["key"]);
});

db_test!(action_outcomes_disclose_only_viewable_records, |db: TestDb| async move {
    let fx = fixture(db).await;
    let notes = fx.resource("notes");
    let note = notes.create(&fx.ctx(admin()), data(json!({"title": "Unseen"}))).await.unwrap();
    // May touch (update) but not view: the record is processed, its key not echoed.
    let blind = fx.ctx(editor("7").without_all().with(&["notes.update"]));
    let outcome = notes.run_action(&blind, "touch", std::slice::from_ref(&note.key), Data::new()).await.unwrap();
    assert!(outcome.processed.is_empty() && outcome.denied.is_empty() && outcome.missing.is_empty(), "{outcome:?}");
    // Reverse check: with view, the key is reported.
    let seeing = fx.ctx(editor("7").without_all().with(&["notes.update", "notes.view"]));
    let outcome = notes.run_action(&seeing, "touch", std::slice::from_ref(&note.key), Data::new()).await.unwrap();
    assert_eq!(outcome.processed, vec![note.key.clone()]);
});

db_test!(the_key_quota_never_blocks_a_retry, |db: TestDb| async move {
    use sea_orm::{ConnectionTrait, Statement};
    let fx = fixture(db).await;
    let notes = fx.resource("notes");
    let ctx = keyed(fx.ctx(admin()), "kept");
    let first = notes.create(&ctx, data(json!({"title": "Kept"}))).await.unwrap();
    let backend = fx.db.conn().get_database_backend();
    let expires = (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339();
    for i in 0..rustcms_admin::idempotency::MAX_LIVE_KEYS {
        let sql = format!(
            "INSERT INTO idempotency_keys (id, owner, request_hash, result, created_at, expires_at) \
             VALUES ('filler-{i}', 'admin:1', 'x', '{{}}', '{expires}', '{expires}')"
        );
        fx.db.conn().execute_raw(Statement::from_string(backend, sql)).await.unwrap();
    }
    let err = notes.create(&keyed(fx.ctx(admin()), "new"), data(json!({"title": "New"}))).await.unwrap_err();
    assert!(matches!(err, AppError::TooManyRequests { .. }), "{err:?}");
    assert_eq!(
        notes.create(&ctx, data(json!({"title": "Kept"}))).await.unwrap().key,
        first.key,
        "retries still replay"
    );
});

/// Titles whose first create already failed (per title: tests run concurrently).
static FAILED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// Notes whose first "flaky" create waits, then fails (rolling its claim back).
struct FlakyNotes;

#[async_trait::async_trait]
impl rustcms_admin::Resource for FlakyNotes {
    type Entity = note::Entity;
    const SLUG: &'static str = "flaky_notes";

    fn meta() -> rustcms_admin::ResourceMeta {
        rustcms_admin::ResourceMeta::new("Note")
    }

    fn fields() -> Vec<rustcms_admin::Field> {
        vec![rustcms_admin::Field::make("title")]
    }

    async fn mutate_before_create(
        _ctx: &Ctx,
        _txn: &sea_orm::DatabaseTransaction,
        data: Data,
    ) -> rustcms_core::AppResult<Data> {
        let title = data.get("title").and_then(|t| t.as_str()).unwrap_or_default().to_owned();
        let first = {
            let mut failed = FAILED.lock().unwrap();
            let first = !failed.contains(&title);
            failed.push(title);
            first
        };
        if first {
            tokio::time::sleep(std::time::Duration::from_millis(1_500)).await;
            return Err(AppError::internal("first attempt fails"));
        }
        Ok(data)
    }
}

db_test!(a_waiter_takes_over_when_the_first_claim_rolls_back, |db: TestDb| async move {
    let fx = fixture(db).await;
    let registry = rustcms_admin::Registry::new().register::<FlakyNotes>().finish();
    let flaky: Arc<dyn DynResource> = registry.get("flaky_notes").unwrap().clone();
    let ctx = keyed(Ctx::new(fx.db.conn().clone(), admin(), registry.clone()), "flaky");
    let (first_flaky, first_ctx) = (flaky.clone(), ctx.clone());
    let title = format!("flaky-{}", fx.db.url().len());
    let body = data(json!({ "title": title }));
    let first_body = body.clone();
    let first = tokio::spawn(async move { first_flaky.create(&first_ctx, first_body).await });
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let second = flaky.create(&ctx, body).await;
    assert!(first.await.unwrap().is_err());
    second.expect("the waiter acquires the released claim and writes");
    assert_eq!(note::Entity::find().count(fx.db.conn()).await.unwrap(), 1);
});

/// Runs `n` identical keyed creates at once; the keys of those that succeeded.
async fn race(notes: &Arc<dyn DynResource>, ctx: &Ctx, title: &str, n: usize) -> Vec<String> {
    let tasks: Vec<_> = (0..n)
        .map(|_| {
            let (notes, ctx, title) = (notes.clone(), ctx.clone(), title.to_owned());
            tokio::spawn(async move { notes.create(&ctx, data(json!({"title": title}))).await })
        })
        .collect();
    let mut keys = Vec::new();
    for task in tasks {
        match task.await.unwrap() {
            Ok(envelope) => keys.push(envelope.key),
            // SQLite may report a busy database to a waiter: retryable, nothing written.
            Err(AppError::Conflict { .. }) => {}
            Err(other) => panic!("{other:?}"),
        }
    }
    keys.sort();
    keys.dedup();
    keys
}

db_test!(an_expired_unpruned_key_is_replaced_once, |db: TestDb| async move {
    use sea_orm::{ConnectionTrait, sea_query::Expr};
    let fx = fixture(db).await;
    let notes = fx.resource("notes");
    for round in 0..5 {
        let ctx = keyed(fx.ctx(admin()), &format!("old-{round}"));
        notes.create(&ctx, data(json!({"title": format!("First {round}")}))).await.unwrap();
        let past = chrono::Utc::now() - chrono::Duration::hours(1);
        let update = sea_orm::sea_query::Query::update()
            .table(sea_orm::sea_query::Alias::new("idempotency_keys"))
            .value(sea_orm::sea_query::Alias::new("expires_at"), Expr::value(past))
            .to_owned();
        fx.db.conn().execute(&update).await.unwrap();
        // Not pruned: concurrent requests find the expired receipt and replace it, once.
        let keys = race(&notes, &ctx, &format!("Second {round}"), 4).await;
        assert_eq!(keys.len(), 1, "every success names the same record");
        assert_eq!(note::Entity::find().count(fx.db.conn()).await.unwrap(), 2 * (round + 1));
    }
});

db_test!(a_racing_retry_at_the_quota_replays, |db: TestDb| async move {
    use sea_orm::{ConnectionTrait, Statement};
    let fx = fixture(db).await;
    let notes = fx.resource("notes");
    let backend = fx.db.conn().get_database_backend();
    let expires = (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339();
    // One key left before the quota.
    for i in 0..rustcms_admin::idempotency::MAX_LIVE_KEYS - 1 {
        let sql = format!(
            "INSERT INTO idempotency_keys (id, owner, request_hash, result, created_at, expires_at) \
             VALUES ('filler-{i}', 'admin:1', 'x', '{{}}', '{expires}', '{expires}')"
        );
        fx.db.conn().execute_raw(Statement::from_string(backend, sql)).await.unwrap();
    }
    // The first takes the last key; the others are retries of it and replay (no 429).
    let keys = race(&notes, &keyed(fx.ctx(admin()), "edge"), "Edge", 4).await;
    assert_eq!(keys.len(), 1);
    assert_eq!(note::Entity::find().count(fx.db.conn()).await.unwrap(), 1);
});
