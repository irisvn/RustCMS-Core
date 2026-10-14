//! Audit log of the engine's writes (ADR 0014): one entry per write, in its
//! transaction, redacted by declaration, with the actor and the request.

mod support;

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use base64::Engine;
use rustcms_admin::{Action, Ctx, Data, Field, Form, Registry, Resource, ResourceMeta, TextInput, components};
use rustcms_core::audit::{AuditActor, AuditContext, AuditEntry, RequestMeta};
use rustcms_core::models::audit_log;
use rustcms_core::{AppError, AppResult};
use rustcms_testing::{TestApp, TestClient, TestDb, TestResponse, db_test};
use sea_orm::entity::prelude::*;
use sea_orm::{ActiveValue::Set, ConnectionTrait, DatabaseTransaction, IntoActiveModel, QueryOrder};
use serde_json::{Value, json};
use support::app::setup;
use support::{Fixture, PrincipalExt, admin, note, tag};

fn data(value: Value) -> Data {
    value.as_object().expect("object").clone()
}

async fn entries(conn: &DatabaseConnection) -> Vec<audit_log::Model> {
    audit_log::Entity::find()
        .order_by_asc(audit_log::Column::At)
        .order_by_asc(audit_log::Column::Id)
        .all(conn)
        .await
        .unwrap()
}

async fn events(conn: &DatabaseConnection) -> Vec<(String, String, Option<String>)> {
    entries(conn).await.into_iter().map(|e| (e.event, e.subject_type, e.subject_key)).collect()
}

fn ev(event: &str, subject: &str, key: &str) -> (String, String, Option<String>) {
    (event.into(), subject.into(), Some(key.into()))
}

async fn fixture(db: TestDb) -> Fixture {
    db.migrate::<rustcms_migration::Migrator>().await;
    Fixture::new(db).await
}

/// The admin acting from a request.
fn audited(fx: &Fixture) -> Ctx {
    let principal = admin();
    let request = RequestMeta {
        ip: Some("203.0.113.9".into()),
        user_agent: Some("Tester/1.0".into()),
        request_id: Some("req-1".into()),
    };
    fx.ctx(principal.clone()).with_audit(AuditContext::new(AuditActor::user(&principal), request))
}

db_test!(each_write_records_one_entry, |db: TestDb| async move {
    let fx = fixture(db).await;
    let ctx = audited(&fx);
    let notes = fx.resource("notes");
    let conn = fx.db.conn();
    let body = json!({"type": "doc", "version": 1, "content": [{"type": "paragraph", "content": [{"type": "text", "text": "Hi"}]}]});
    let created = notes.create(&ctx, data(json!({"title": "First", "body": body}))).await.unwrap();
    let key = created.key.as_str();

    let all = entries(conn).await;
    assert_eq!(all.len(), 1);
    let entry = &all[0];
    assert_eq!(
        (entry.event.as_str(), entry.subject_type.as_str(), entry.subject_key.as_deref()),
        ("created", "notes", Some(key))
    );
    assert_eq!(
        (entry.actor_kind.as_str(), entry.actor_user_id, entry.actor_guard.as_deref()),
        ("user", Some(1), Some("admin"))
    );
    assert_eq!(
        (entry.ip.as_deref(), entry.user_agent.as_deref(), entry.request_id.as_deref()),
        (Some("203.0.113.9"), Some("Tester/1.0"), Some("req-1"))
    );
    let changes = entry.changes.clone().unwrap();
    assert_eq!(changes["title"], json!([null, "First"]));
    // Rich text: its size and digest, never the document.
    assert_eq!(changes["body"][0], Value::Null);
    assert_eq!(changes["body"][1]["document"], json!(true));
    assert_eq!(changes["body"][1]["sha256"].as_str().unwrap().len(), 64);
    assert!(!changes.to_string().contains("Hi\""), "the document is not stored");
    assert!(changes.get("deleted_at").is_none(), "unchanged fields are omitted");
    assert!(entry.meta.is_none());

    // Updates record what changed only; an update that changes nothing records nothing.
    notes.update(&ctx, key, data(json!({"title": "Second"}))).await.unwrap();
    notes.update(&ctx, key, data(json!({"title": "Second"}))).await.unwrap();
    let all = entries(conn).await;
    assert_eq!(all.len(), 2);
    assert_eq!(all[1].event, "updated");
    assert_eq!(all[1].changes, Some(json!({"title": ["First", "Second"]})));

    // Trash, restore, force delete.
    notes.delete(&ctx, key).await.unwrap();
    notes.restore(&ctx, key).await.unwrap();
    notes.delete(&ctx, key).await.unwrap();
    notes.force_delete(&ctx, key).await.unwrap();
    // Without soft deletes, a delete is `deleted`.
    let categories = fx.resource("categories");
    let category = categories.create(&ctx, data(json!({"name": "C", "slug": "c"}))).await.unwrap();
    categories.delete(&ctx, &category.key).await.unwrap();
    assert_eq!(
        events(conn).await,
        [
            ev("created", "notes", key),
            ev("updated", "notes", key),
            ev("trashed", "notes", key),
            ev("restored", "notes", key),
            ev("trashed", "notes", key),
            ev("force_deleted", "notes", key),
            ev("created", "categories", &category.key),
            ev("deleted", "categories", &category.key),
        ]
    );
});

db_test!(refused_and_failed_writes_record_nothing, |db: TestDb| async move {
    let fx = fixture(db).await;
    let conn = fx.db.conn();
    let tags = fx.resource("tags");
    let ctx = fx.ctx(admin());
    // `after_save` fails: the write and its entry roll back together.
    let err = tags.create(&ctx, data(json!({"slug": "b", "label": "boom"}))).await.unwrap_err();
    assert!(matches!(err, AppError::Internal(_)), "{err:?}");
    // Refused by validation, or by the policy.
    tags.create(&ctx, data(json!({"slug": "v"}))).await.unwrap_err();
    let reader = fx.ctx(support::editor("7"));
    let before = entries(conn).await.len();
    tags.create(&reader, data(json!({"slug": "r", "label": "r"}))).await.unwrap_err();
    assert_eq!(entries(conn).await.len(), before);
    assert_eq!(events(conn).await.iter().filter(|e| e.1 == "tags").count(), 0);
    // Reverse check: the same create without the failing hook is recorded.
    tags.create(&ctx, data(json!({"slug": "ok", "label": "fine"}))).await.unwrap();
    assert_eq!(events(conn).await.last().unwrap(), &ev("created", "tags", "ok"));
});

db_test!(a_failed_entry_rolls_the_write_and_its_receipt_back, |db: TestDb| async move {
    let fx = fixture(db).await;
    let conn = fx.db.conn();
    let notes = fx.resource("notes");
    let ctx = fx.ctx(admin()).with_idempotency_key(Some("k-1".into()));
    // A replay adds no entry.
    notes.create(&ctx, data(json!({"title": "Once"}))).await.unwrap();
    notes.create(&ctx, data(json!({"title": "Once"}))).await.unwrap();
    assert_eq!(entries(conn).await.len(), 1);

    conn.execute_unprepared("DROP TABLE audit_logs").await.unwrap();
    let keyed = fx.ctx(admin()).with_idempotency_key(Some("k-2".into()));
    notes.create(&keyed, data(json!({"title": "Lost"}))).await.unwrap_err();
    let count = |sql: &'static str| async move {
        let backend = conn.get_database_backend();
        let row = conn.query_one_raw(sea_orm::Statement::from_string(backend, sql)).await.unwrap().unwrap();
        row.try_get::<i64>("", "n").unwrap()
    };
    assert_eq!(count("SELECT COUNT(*) AS n FROM notes").await, 1, "the note rolled back");
    assert_eq!(count("SELECT COUNT(*) AS n FROM idempotency_keys").await, 1, "and its receipt");
});

/// `tags` under other names, to declare fields differently.
macro_rules! tag_resource {
    ($name:ident, $slug:literal, $meta:expr, $fields:expr, $form:expr) => {
        struct $name;

        #[async_trait::async_trait]
        impl Resource for $name {
            type Entity = tag::Entity;
            const SLUG: &'static str = $slug;

            fn meta() -> ResourceMeta {
                $meta
            }

            fn fields() -> Vec<Field> {
                $fields
            }

            fn form() -> Form {
                $form
            }
        }
    };
}

fn natural(label: &str) -> ResourceMeta {
    ResourceMeta::new(label).title("label").key(rustcms_admin::KeyStrategy::Natural)
}

fn tag_fields(label: Field) -> Vec<Field> {
    vec![Field::column(tag::Column::Slug).required(), label.required()]
}

fn tag_form(password: bool) -> Form {
    let label = if password { TextInput::make("label").password() } else { TextInput::make("label") };
    Form::new().schema(components![TextInput::make("slug"), label])
}

tag_resource!(
    Vault,
    "vault",
    natural("Vault"),
    tag_fields(Field::column(tag::Column::Label).audit_redact()),
    tag_form(false)
);
tag_resource!(Pins, "pins", natural("Pin"), tag_fields(Field::column(tag::Column::Label)), tag_form(true));
tag_resource!(
    Quiet,
    "quiet",
    natural("Quiet").audit(false),
    tag_fields(Field::column(tag::Column::Label)),
    tag_form(false)
);
tag_resource!(Plain, "plain", natural("Plain"), tag_fields(Field::column(tag::Column::Label)), tag_form(false));

/// Upper-cases the label after saving: the entry shows the final value. A
/// label `vanish` makes the hook delete the record.
struct Shouty;

#[async_trait::async_trait]
impl Resource for Shouty {
    type Entity = tag::Entity;
    const SLUG: &'static str = "shouty";

    fn meta() -> ResourceMeta {
        natural("Shouty")
    }

    fn fields() -> Vec<Field> {
        tag_fields(Field::column(tag::Column::Label))
    }

    async fn after_save(_ctx: &Ctx, txn: &DatabaseTransaction, record: &tag::Model) -> AppResult<()> {
        if record.label == "vanish" {
            record.clone().delete(txn).await?;
            return Ok(());
        }
        let label = record.label.to_uppercase();
        let mut active = record.clone().into_active_model();
        active.label = Set(label);
        active.update(txn).await?;
        Ok(())
    }
}

/// Notes binned, purged or touched by actions; `bin` also renames another note.
struct Bins;

impl Resource for Bins {
    type Entity = note::Entity;
    const SLUG: &'static str = "bins";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Bin").title("title").soft_deletes()
    }

    fn fields() -> Vec<Field> {
        vec![
            Field::column(note::Column::Title).required(),
            Field::column(note::Column::DeletedAt).guarded().readable(|_| true),
        ]
    }

    fn actions() -> Vec<Action<note::Entity>> {
        vec![
            Action::<note::Entity>::new("bin", |ctx, txn, models, _| {
                Box::pin(async move {
                    for model in models {
                        let mut active = model.into_active_model();
                        active.deleted_at = Set(Some(chrono::Utc::now()));
                        active.update(txn).await?;
                    }
                    note::Entity::update_many()
                        .col_expr(note::Column::Title, sea_orm::sea_query::Expr::value("renamed"))
                        .filter(note::Column::Title.eq("bystander"))
                        .exec(txn)
                        .await?;
                    // Writes beyond the selected records are recorded explicitly.
                    ctx.audit(txn, AuditEntry::new("renamed_bystander", "bins")).await
                })
            })
            .bulk(),
            Action::<note::Entity>::new("purge", |_, txn, models, _| {
                Box::pin(async move {
                    for model in models {
                        model.delete(txn).await?;
                    }
                    Ok(())
                })
            })
            .bulk(),
            Action::<note::Entity>::new("rename", |_, txn, models, _| {
                Box::pin(async move {
                    for model in models {
                        let title = format!("{}!", model.title);
                        let mut active = model.into_active_model();
                        active.title = Set(title);
                        active.update(txn).await?;
                    }
                    Ok(())
                })
            })
            .bulk(),
        ]
    }
}

async fn registry(db: &TestDb) -> (Arc<Registry>, Ctx) {
    db.migrate::<rustcms_migration::Migrator>().await;
    support::create_tables(db.conn()).await;
    let registry = Registry::new()
        .register::<Vault>()
        .register::<Pins>()
        .register::<Quiet>()
        .register::<Plain>()
        .register::<Shouty>()
        .register::<Bins>()
        .finish();
    let ctx = Ctx::new(db.conn().clone(), admin(), registry.clone());
    (registry, ctx)
}

db_test!(values_are_redacted_by_declaration_and_bounded, |db: TestDb| async move {
    let (registry, ctx) = registry(&db).await;
    let conn = db.conn();
    let create = |slug: &'static str, key: &'static str, label: String| {
        let resource = registry.get(slug).unwrap().clone();
        let ctx = ctx.clone();
        async move { resource.create(&ctx, data(json!({"slug": key, "label": label}))).await.unwrap() }
    };
    create("vault", "v", "vault-secret-1".into()).await;
    create("pins", "p", "pin-secret-2".into()).await;
    registry.get("vault").unwrap().update(&ctx, "v", data(json!({"label": "vault-secret-3"}))).await.unwrap();
    let all = entries(conn).await;
    assert_eq!(all[0].changes, Some(json!({"slug": [null, "v"], "label": [null, "[redacted]"]})));
    assert_eq!(all[1].changes, Some(json!({"slug": [null, "p"], "label": [null, "[redacted]"]})));
    assert_eq!(all[2].changes, Some(json!({"label": ["[redacted]", "[redacted]"]})), "a change, not its values");
    // Reverse check: the same field undeclared as secret is stored.
    create("plain", "x", "visible".into()).await;
    assert_eq!(entries(conn).await[3].changes.as_ref().unwrap()["label"], json!([null, "visible"]));

    // Long values are kept as their length and digest.
    create("plain", "long", "é".repeat(5_000)).await;
    let stored = entries(conn).await[4].changes.clone().unwrap();
    assert_eq!(
        (stored["label"][1]["truncated"].clone(), stored["label"][1]["length"].clone()),
        (json!(true), json!(5_000))
    );
    assert_eq!(stored["label"][1]["sha256"].as_str().unwrap().len(), 64);

    // `audit(false)` records nothing.
    create("quiet", "q", "q".into()).await;
    registry.get("quiet").unwrap().delete(&ctx, "q").await.unwrap();
    assert_eq!(entries(conn).await.len(), 5);

    // No secret reaches the table, in any column.
    let dump = format!("{:?}", entries(conn).await);
    for secret in ["vault-secret-1", "pin-secret-2", "vault-secret-3"] {
        assert!(!dump.contains(secret), "{secret} stored");
    }
});

db_test!(changes_reflect_the_record_after_its_hooks, |db: TestDb| async move {
    let (registry, ctx) = registry(&db).await;
    let shouty = registry.get("shouty").unwrap();
    shouty.create(&ctx, data(json!({"slug": "s", "label": "quiet"}))).await.unwrap();
    shouty.update(&ctx, "s", data(json!({"label": "again"}))).await.unwrap();
    let all = entries(db.conn()).await;
    assert_eq!(all[0].changes.as_ref().unwrap()["label"], json!([null, "QUIET"]));
    assert_eq!(all[1].changes, Some(json!({"label": ["QUIET", "AGAIN"]})));

    // A hook that deletes the record: the deletion is recorded too.
    shouty.update(&ctx, "s", data(json!({"label": "vanish"}))).await.unwrap();
    shouty.create(&ctx, data(json!({"slug": "t", "label": "vanish"}))).await.unwrap();
    let rest: Vec<_> = entries(db.conn()).await[2..].iter().map(|e| (e.event.clone(), e.subject_key.clone())).collect();
    let pair = |e: &str, k: &str| (e.to_owned(), Some(k.to_owned()));
    assert_eq!(rest, [pair("updated", "s"), pair("deleted", "s"), pair("created", "t"), pair("deleted", "t")]);
});

db_test!(actions_record_each_selected_record, |db: TestDb| async move {
    let (registry, ctx) = registry(&db).await;
    let conn = db.conn();
    let bins = registry.get("bins").unwrap();
    let mut keys = vec![];
    for title in ["a", "b", "c", "bystander"] {
        keys.push(bins.create(&ctx, data(json!({"title": title}))).await.unwrap().key);
    }
    let created = entries(conn).await.len();
    assert_eq!(created, 4);

    bins.run_action(&ctx, "rename", &keys[..1], Data::new()).await.unwrap();
    bins.run_action(&ctx, "bin", &keys[1..2], Data::new()).await.unwrap();
    bins.run_action(&ctx, "purge", &keys[2..3], Data::new()).await.unwrap();
    let all = &entries(conn).await[created..];
    let summary: Vec<_> = all.iter().map(|e| (e.event.as_str(), e.subject_key.as_deref(), e.meta.clone())).collect();
    assert_eq!(
        summary,
        [
            ("action:rename", Some(keys[0].as_str()), None),
            ("renamed_bystander", None, None),
            ("action:bin", Some(keys[1].as_str()), Some(json!({"result": "trashed"}))),
            ("action:purge", Some(keys[2].as_str()), Some(json!({"result": "deleted"}))),
        ]
    );
    assert_eq!(all[0].changes, Some(json!({"title": ["a", "a!"]})));
    assert_eq!(all[3].changes, None, "a deleted record's former values are not kept");
    // The trash marker is guarded: its change is shown redacted.
    assert_eq!(all[2].changes, Some(json!({"deleted_at": [null, "[redacted]"]})));
    // The record the action wrote besides the selected ones has no automatic entry.
    assert!(!all.iter().any(|e| e.subject_key.as_deref() == Some(keys[3].as_str())));
});

db_test!(a_large_action_records_in_batches, |db: TestDb| async move {
    let (registry, ctx) = registry(&db).await;
    let bins = registry.get("bins").unwrap();
    let rows: Vec<note::ActiveModel> =
        (0..500).map(|i| note::ActiveModel { title: Set(format!("n{i}")), ..Default::default() }).collect();
    note::Entity::insert_many(rows).exec_without_returning(db.conn()).await.unwrap();
    let keys: Vec<String> =
        note::Entity::find().all(db.conn()).await.unwrap().into_iter().map(|n| n.id.to_string()).collect();
    let started = std::time::Instant::now();
    let outcome = bins.run_action(&ctx, "rename", &keys, Data::new()).await.unwrap();
    eprintln!("action over 500 records: {:?}", started.elapsed());
    assert_eq!(outcome.processed.len(), 500);
    assert_eq!(entries(db.conn()).await.len(), 500);
});

/// A valid 1×1 PNG.
fn png() -> Vec<u8> {
    base64::engine::general_purpose::STANDARD
        .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=")
        .unwrap()
}

async fn upload(app: &TestApp, client: &TestClient<'_>, key: &str) -> TestResponse {
    let boundary = "rustcms-test-boundary";
    let mut body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"a.png\"\r\n\
         Content-Type: application/octet-stream\r\n\r\n"
    )
    .into_bytes();
    body.extend_from_slice(&png());
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    client.fetch_csrf().await;
    let request = Request::builder()
        .method(Method::POST)
        .uri(app.admin_api("media"))
        .header("content-type", format!("multipart/form-data; boundary={boundary}"))
        .header("content-length", body.len())
        .header("idempotency-key", key)
        .body(Body::from(body))
        .unwrap();
    client.request(request).await
}

/// `POST uri` with JSON and extra headers.
async fn post(client: &TestClient<'_>, uri: &str, body: &Value, headers: &[(&str, &str)]) -> TestResponse {
    client.fetch_csrf().await;
    let mut request = Request::builder().method(Method::POST).uri(uri).header("content-type", "application/json");
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    client.request(request.body(Body::from(body.to_string())).unwrap()).await
}

db_test!(http_writes_record_the_session_and_the_request, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    let conn = app.state().db();
    let client = app.acting_as(&users.editor).await;
    let notes = app.admin_api("resources/notes/records");
    let headers = [("x-request-id", "req-42"), ("user-agent", "Browser/2.0")];
    post(&client, &notes, &json!({"title": "From HTTP"}), &headers).await.assert_status(StatusCode::CREATED);
    let entry = entries(conn).await.into_iter().find(|e| e.subject_type == "notes").unwrap();
    assert_eq!(
        (entry.actor_kind.as_str(), entry.actor_user_id, entry.actor_guard.as_deref(), entry.credential.as_deref()),
        ("user", Some(users.editor.id), Some("admin"), Some("session"))
    );
    assert_eq!(
        (entry.ip.as_deref(), entry.user_agent.as_deref(), entry.request_id.as_deref()),
        (Some("127.0.0.1"), Some("Browser/2.0"), Some("req-42"))
    );

    // A request id that is not a plain token, or is sent twice, is replaced by
    // a generated one; a long user agent is cut.
    let (long_id, long_agent) = ("x".repeat(65), "A".repeat(400));
    let cases: [(&str, Vec<(&str, &str)>); 3] = [
        ("bad", vec![("x-request-id", "bad id!")]),
        ("twice", vec![("x-request-id", "one"), ("x-request-id", "two")]),
        ("long", vec![("x-request-id", &long_id), ("user-agent", &long_agent)]),
    ];
    for (title, headers) in &cases {
        post(&client, &notes, &json!({"title": title}), headers).await.assert_status(StatusCode::CREATED);
    }
    let rest: Vec<_> = entries(conn).await.into_iter().filter(|e| e.subject_type == "notes").skip(1).collect();
    assert_eq!(rest.len(), 3);
    for entry in &rest {
        let id = entry.request_id.as_deref().unwrap();
        assert!(uuid::Uuid::parse_str(id).is_ok(), "generated, got {id}");
    }
    assert_eq!(rest[2].user_agent.as_deref().map(str::len), Some(255));
});

db_test!(relation_creates_and_uploads_are_recorded, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    let conn = app.state().db();
    let client = app.acting_as(&users.admin).await;
    let category =
        post(&client, &app.admin_api("resources/categories/records"), &json!({"name": "A", "slug": "a"}), &[])
            .await
            .json()["data"]["key"]
            .as_str()
            .unwrap()
            .to_owned();
    let relation = app.admin_api(&format!("resources/categories/records/{category}/relations/notes"));
    let note = post(&client, &relation, &json!({"title": "Child"}), &[]).await;
    note.assert_status(StatusCode::CREATED);
    let key = note.json()["data"]["key"].as_str().unwrap().to_owned();
    let entry = entries(conn).await.into_iter().find(|e| e.subject_type == "notes").unwrap();
    assert_eq!(entry.subject_key.as_deref(), Some(key.as_str()));
    assert_eq!(entry.meta, Some(json!({"relation": "categories.notes", "parent": category})));
    assert_eq!(entry.changes.as_ref().unwrap()["category_id"], json!([null, category.parse::<i64>().unwrap()]));

    // An upload, then its replay: one entry.
    let first = upload(&app, &client, "up-1").await;
    first.assert_status(StatusCode::CREATED);
    upload(&app, &client, "up-1").await.assert_status(StatusCode::CREATED);
    let uploads: Vec<_> = entries(conn).await.into_iter().filter(|e| e.event == "uploaded").collect();
    assert_eq!(uploads.len(), 1);
    assert_eq!(
        (uploads[0].subject_type.as_str(), uploads[0].subject_key.as_deref()),
        ("media", first.json()["data"]["id"].as_str())
    );
    assert_eq!(uploads[0].meta, Some(json!({"mime": "image/png", "size": png().len()})));
});

/// A natural key cannot be redacted: it names the record in every entry.
struct RedactedKey;

impl Resource for RedactedKey {
    type Entity = tag::Entity;
    const SLUG: &'static str = "redacted_key";

    fn meta() -> ResourceMeta {
        natural("Redacted")
    }

    fn fields() -> Vec<Field> {
        vec![Field::column(tag::Column::Slug).audit_redact(), Field::column(tag::Column::Label)]
    }
}

#[test]
#[should_panic(expected = "cannot be `.audit_redact()`")]
fn a_redacted_primary_key_is_refused() {
    let _ = Registry::new().register::<RedactedKey>().finish();
}

/// Notes whose category is a secret: the parent of a relation create is redacted too.
struct SecretNotes;

impl Resource for SecretNotes {
    type Entity = note::Entity;
    const SLUG: &'static str = "notes";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Note").title("title")
    }

    fn fields() -> Vec<Field> {
        vec![
            Field::column(note::Column::Title).required(),
            Field::column(note::Column::CategoryId).belongs_to("categories").audit_redact(),
        ]
    }
}

fn secret_build(config: rustcms_core::Config) -> rustcms_core::AppBuilder {
    let mut app = rustcms_core::AppBuilder::new(config);
    app.provider(rustcms_auth::AuthProvider::new());
    app.provider(rustcms_admin::AdminProvider::new().resource::<support::CategoryResource>().resource::<SecretNotes>());
    app
}

db_test!(a_redacted_foreign_key_hides_the_relation_parent, |db: TestDb| async move {
    use rustcms_testing::{Factory, UserFactory};
    db.migrate::<rustcms_migration::Migrator>().await;
    support::create_tables(db.conn()).await;
    let app = TestApp::new(secret_build, db).await;
    let user = UserFactory::new().create(app.state().db()).await;
    app.make_super_admin(&user).await;
    let client = app.acting_as(&user).await;
    let category =
        post(&client, &app.admin_api("resources/categories/records"), &json!({"name": "S", "slug": "s"}), &[])
            .await
            .json()["data"]["key"]
            .as_str()
            .unwrap()
            .to_owned();
    let relation = app.admin_api(&format!("resources/categories/records/{category}/relations/notes"));
    post(&client, &relation, &json!({"title": "Child"}), &[]).await.assert_status(StatusCode::CREATED);
    let entry = entries(app.state().db()).await.into_iter().find(|e| e.subject_type == "notes").unwrap();
    assert_eq!(entry.meta, Some(json!({"relation": "categories.notes", "parent": "[redacted]"})));
    assert_eq!(entry.changes.as_ref().unwrap()["category_id"], json!([null, "[redacted]"]));
});

db_test!(an_engine_context_is_audited_as_its_principal, |db: TestDb| async move {
    let fx = fixture(db).await;
    fx.resource("notes")
        .create(&fx.ctx(support::editor("7").with(&["notes.create"])), data(json!({"title": "x"})))
        .await
        .unwrap();
    let entry = entries(fx.db.conn()).await.pop().unwrap();
    assert_eq!(
        (entry.actor_kind.as_str(), entry.actor_user_id, entry.actor_guard.as_deref()),
        ("user", Some(7), Some("admin"))
    );
});

db_test!(a_failed_entry_rolls_actions_and_uploads_back, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    let conn = app.state().db();
    let client = app.acting_as(&users.admin).await;
    let created = post(&client, &app.admin_api("resources/notes/records"), &json!({"title": "Keep"}), &[]).await;
    let key = created.json()["data"]["key"].as_str().unwrap().to_owned();
    conn.execute_unprepared("DROP TABLE audit_logs").await.unwrap();
    // The action and the upload fail with their entry; nothing they did remains.
    let action = app.admin_api("resources/notes/actions/touch");
    let response = post(&client, &action, &json!({"keys": [key]}), &[("idempotency-key", "act-1")]).await;
    response.assert_status(StatusCode::INTERNAL_SERVER_ERROR);
    upload(&app, &client, "up-9").await.assert_status(StatusCode::INTERNAL_SERVER_ERROR);
    let count = |table: &'static str| async move {
        let backend = conn.get_database_backend();
        let sql = format!("SELECT COUNT(*) AS n FROM {table}");
        let row = conn.query_one_raw(sea_orm::Statement::from_string(backend, sql)).await.unwrap().unwrap();
        row.try_get::<i64>("", "n").unwrap()
    };
    assert_eq!(count("media").await, 0);
    assert_eq!(count("idempotency_keys").await, 0);
});
