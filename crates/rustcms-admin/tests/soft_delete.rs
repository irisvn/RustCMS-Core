//! Soft deletes: trashed records are hidden on every path; restore and force
//! delete have their own abilities. On SQLite and PostgreSQL.

mod support;

use std::sync::Arc;

use rustcms_admin::{Data, ListQuery, Trashed};
use rustcms_core::AppError;
use rustcms_testing::{TestDb, db_test};
use serde_json::{Value, json};
use support::{Fixture, PrincipalExt, admin, editor, note};

fn data(value: Value) -> Data {
    value.as_object().expect("object").clone()
}

fn trashed(mode: Trashed) -> ListQuery {
    ListQuery { trashed: mode, ..Default::default() }
}

fn field_errors(err: AppError) -> Vec<String> {
    match err {
        AppError::Validation(v) => v.errors.into_keys().collect(),
        other => panic!("expected validation error, got {other:?}"),
    }
}

db_test!(trashed_records_are_hidden_on_every_path, |db: TestDb| async move {
    use sea_orm::{EntityTrait, PaginatorTrait};
    let fx = Fixture::new(db).await;
    let ctx = fx.ctx(admin());
    let notes = fx.resource("notes");
    let gone = notes.create(&ctx, data(json!({"title": "Gone"}))).await.unwrap();
    let kept = notes.create(&ctx, data(json!({"title": "Kept"}))).await.unwrap();
    notes.delete(&ctx, &gone.key).await.unwrap();
    assert_eq!(note::Entity::find().count(fx.db.conn()).await.unwrap(), 2, "the row is kept");

    let key = gone.key.as_str();
    assert!(matches!(notes.find(&ctx, key).await, Err(AppError::NotFound)));
    assert!(matches!(notes.update(&ctx, key, data(json!({"title": "x"}))).await, Err(AppError::NotFound)));
    assert!(matches!(notes.delete(&ctx, key).await, Err(AppError::NotFound)), "deleting twice");
    let page = notes.list(&ctx, &ListQuery::default()).await.unwrap();
    assert_eq!((page.meta.total, page.data[0].key.as_str()), (1, kept.key.as_str()));
    assert_eq!(notes.lookup(&ctx, Some("Gone"), &[]).await.unwrap(), vec![]);
    assert_eq!(notes.lookup(&ctx, None, std::slice::from_ref(&gone.key)).await.unwrap(), vec![]);
    assert!(notes.titles(&ctx, std::slice::from_ref(&gone.key)).await.unwrap().is_empty(), "labels show `#key`");
    let txn = rustcms_core::db::begin_write(fx.db.conn()).await.unwrap();
    assert!(
        notes.keys_in_scope(&ctx, &txn, std::slice::from_ref(&gone.key)).await.unwrap().is_empty(),
        "not referenceable"
    );
    txn.rollback().await.unwrap();
    let outcome = notes.run_action(&ctx, "touch", &[gone.key.clone(), kept.key.clone()], Data::new()).await.unwrap();
    assert_eq!((outcome.processed, outcome.missing), (vec![kept.key.clone()], vec![gone.key.clone()]));

    // The trash lists it, with only restore and force delete.
    let trash = notes.list(&ctx, &trashed(Trashed::Only)).await.unwrap();
    assert_eq!(trash.meta.total, 1);
    let row = &trash.data[0];
    assert!(row.trashed);
    for (ability, expected) in [
        ("view", true),
        ("update", false),
        ("delete", false),
        ("touch", false),
        ("restore", true),
        ("force_delete", true),
    ] {
        assert_eq!(row.abilities[ability], expected, "{ability}");
    }
    assert_eq!(notes.list(&ctx, &trashed(Trashed::With)).await.unwrap().meta.total, 2);
    let active = &notes.list(&ctx, &ListQuery::default()).await.unwrap().data[0];
    assert!(!active.trashed && !active.abilities["restore"] && !active.abilities["force_delete"]);

    // Restore brings it back; force delete only applies to trashed records.
    assert!(matches!(notes.force_delete(&ctx, &kept.key).await, Err(AppError::NotFound)));
    assert!(matches!(notes.restore(&ctx, &kept.key).await, Err(AppError::NotFound)));
    let restored = notes.restore(&ctx, key).await.unwrap();
    assert_eq!(restored.data["title"], json!("Gone"));
    assert!(restored.data["deleted_at"].is_null() && !restored.trashed);
    notes.delete(&ctx, key).await.unwrap();
    notes.force_delete(&ctx, key).await.unwrap();
    assert_eq!(note::Entity::find().count(fx.db.conn()).await.unwrap(), 1);
    assert!(matches!(notes.force_delete(&ctx, key).await, Err(AppError::NotFound)));
});

db_test!(trash_restore_and_force_delete_hooks_run_in_their_transactions, |db: TestDb| async move {
    use support::NOTE_HOOKS;
    let fx = Fixture::new(db).await;
    let ctx = fx.ctx(admin());
    let notes = fx.resource("notes");
    let seen = |entry: &str| NOTE_HOOKS.lock().unwrap().iter().any(|e| e == entry);
    let note = notes.create(&ctx, data(json!({"title": "Hooked"}))).await.unwrap();

    // Trashing calls `after_trash` with the record as stored (trashed).
    notes.delete(&ctx, &note.key).await.unwrap();
    assert!(seen("trash:Hooked:true"));
    // Restoring calls `after_restore` with the active record.
    notes.restore(&ctx, &note.key).await.unwrap();
    assert!(seen("restore:Hooked:false"));
    // Force deleting calls `after_force_delete` after the delete, with the record as it was.
    notes.delete(&ctx, &note.key).await.unwrap();
    notes.force_delete(&ctx, &note.key).await.unwrap();
    assert!(seen("force:Hooked:true"));

    // A failing hook rolls its write back.
    let stuck = notes.create(&ctx, data(json!({"title": "stuck-trash"}))).await.unwrap();
    assert!(notes.delete(&ctx, &stuck.key).await.is_err());
    assert!(!notes.find(&ctx, &stuck.key).await.unwrap().trashed, "still active");
    let stuck = notes.create(&ctx, data(json!({"title": "stuck-restore"}))).await.unwrap();
    notes.delete(&ctx, &stuck.key).await.unwrap();
    assert!(notes.restore(&ctx, &stuck.key).await.is_err());
    assert!(matches!(notes.find(&ctx, &stuck.key).await, Err(AppError::NotFound)), "still trashed");
    let stuck = notes.create(&ctx, data(json!({"title": "stuck-force"}))).await.unwrap();
    notes.delete(&ctx, &stuck.key).await.unwrap();
    assert!(notes.force_delete(&ctx, &stuck.key).await.is_err());
    let trash = notes.list(&ctx, &trashed(Trashed::Only)).await.unwrap();
    assert!(trash.data.iter().any(|e| e.key == stuck.key), "still in the trash");
});

/// Notes with an `after_save` and the default `after_restore`.
struct SavedNotes;

/// What `SavedNotes::after_save` saw: `"{title}:{trashed}"`.
static SAVED_NOTES: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

#[async_trait::async_trait]
impl rustcms_admin::Resource for SavedNotes {
    type Entity = note::Entity;
    const SLUG: &'static str = "saved_notes";

    fn meta() -> rustcms_admin::ResourceMeta {
        rustcms_admin::ResourceMeta::new("Note").soft_deletes()
    }

    fn fields() -> Vec<rustcms_admin::Field> {
        vec![rustcms_admin::Field::make("title")]
    }

    async fn after_save(
        _ctx: &rustcms_admin::Ctx,
        _txn: &sea_orm::DatabaseTransaction,
        record: &note::Model,
    ) -> rustcms_core::AppResult<()> {
        SAVED_NOTES.lock().unwrap().push(format!("{}:{}", record.title, record.deleted_at.is_some()));
        if record.title.starts_with("refuse") { Err(AppError::internal("refused")) } else { Ok(()) }
    }
}

db_test!(the_default_after_restore_calls_after_save, |db: TestDb| async move {
    use sea_orm::{ActiveModelTrait, ActiveValue::Set};
    let fx = Fixture::new(db).await;
    let tag = format!("{:?}", fx.db.backend());
    let registry = rustcms_admin::Registry::new().register::<SavedNotes>().finish();
    let ctx = rustcms_admin::Ctx::new(fx.db.conn().clone(), admin(), registry.clone());
    let notes = registry.get("saved_notes").unwrap();
    let title = format!("Kept {tag}");
    let note = notes.create(&ctx, data(json!({"title": title}))).await.unwrap();
    notes.delete(&ctx, &note.key).await.unwrap();
    let saved = |entry: &str| SAVED_NOTES.lock().unwrap().iter().filter(|e| *e == entry).count();
    assert_eq!(saved(&format!("{title}:false")), 1, "the create");
    // Restoring calls `after_save` (the default `after_restore`), with the active record.
    notes.restore(&ctx, &note.key).await.unwrap();
    assert_eq!(saved(&format!("{title}:false")), 2);

    // Its error rolls the restore back (a trashed note stored directly: no hook ran).
    let refused = note::ActiveModel {
        title: Set(format!("refuse {tag}")),
        deleted_at: Set(Some(chrono::Utc::now())),
        ..Default::default()
    }
    .insert(fx.db.conn())
    .await
    .unwrap();
    let key = refused.id.to_string();
    assert!(notes.restore(&ctx, &key).await.is_err());
    assert!(matches!(notes.find(&ctx, &key).await, Err(AppError::NotFound)), "still trashed");

    // An override replaces the call: `NoteResource` restores without `after_save`.
    let notes = fx.resource("notes");
    let admin_ctx = fx.ctx(admin());
    let title = format!("Overridden {tag}");
    let note = notes.create(&admin_ctx, data(json!({"title": title}))).await.unwrap();
    notes.delete(&admin_ctx, &note.key).await.unwrap();
    notes.restore(&admin_ctx, &note.key).await.unwrap();
    let hooks = support::NOTE_HOOKS.lock().unwrap().clone();
    assert!(hooks.contains(&format!("restore:{title}:false")));
    assert_eq!(hooks.iter().filter(|h| **h == format!("save:{title}:false")).count(), 1, "only the create's");
});

db_test!(the_trash_needs_its_own_abilities, |db: TestDb| async move {
    let fx = Fixture::new(db).await;
    let notes = fx.resource("notes");
    let admin_ctx = fx.ctx(admin());
    let note = notes.create(&admin_ctx, data(json!({"title": "Draft"}))).await.unwrap();
    notes.delete(&admin_ctx, &note.key).await.unwrap();

    let deleter = fx.ctx(editor("7").with(&["notes.view_any", "notes.view", "notes.delete"]));
    assert!(matches!(notes.list(&deleter, &trashed(Trashed::Only)).await, Err(AppError::Forbidden)));
    for key in [note.key.as_str(), "999"] {
        assert!(matches!(notes.restore(&deleter, key).await, Err(AppError::Forbidden)), "restore {key}");
        assert!(matches!(notes.force_delete(&deleter, key).await, Err(AppError::Forbidden)), "force {key}");
    }

    // Reverse check: with `notes.restore`, the trash and restore open; force delete stays closed.
    let restorer = fx.ctx(editor("7").with(&["notes.view_any", "notes.view", "notes.restore"]));
    assert_eq!(notes.list(&restorer, &trashed(Trashed::Only)).await.unwrap().meta.total, 1);
    assert!(matches!(notes.force_delete(&restorer, &note.key).await, Err(AppError::Forbidden)));
    assert!(matches!(notes.restore(&restorer, "999").await, Err(AppError::NotFound)));
    notes.restore(&restorer, &note.key).await.unwrap();

    // Resources without soft deletes have no trash.
    let err = fx.resource("posts").list(&admin_ctx, &trashed(Trashed::Only)).await.unwrap_err();
    assert_eq!(field_errors(err), ["trashed"]);
    assert!(matches!(fx.resource("posts").restore(&admin_ctx, "x").await, Err(AppError::NotFound)));
});

/// Notes whose policy lets everyone view and delete, but never restore: its
/// `admits` is the default (true), so only the per-record rule hides the trash.
struct ViewOnlyNotes;

struct ViewOnlyPolicy;

#[async_trait::async_trait]
impl rustcms_admin::Policy for ViewOnlyPolicy {
    async fn allows(&self, _ctx: &rustcms_admin::Ctx, ability: &str, _record: Option<&Data>) -> bool {
        ["view_any", "view", "create", "delete"].contains(&ability)
    }
}

impl rustcms_admin::Resource for ViewOnlyNotes {
    type Entity = note::Entity;
    const SLUG: &'static str = "view_only_notes";

    fn meta() -> rustcms_admin::ResourceMeta {
        rustcms_admin::ResourceMeta::new("Note").soft_deletes()
    }

    fn fields() -> Vec<rustcms_admin::Field> {
        vec![rustcms_admin::Field::make("title")]
    }

    fn policy() -> Arc<dyn rustcms_admin::Policy> {
        Arc::new(ViewOnlyPolicy)
    }
}

db_test!(trashed_records_need_restore_or_force_delete_per_record, |db: TestDb| async move {
    let fx = Fixture::new(db).await;
    let registry = rustcms_admin::Registry::new().register::<ViewOnlyNotes>().finish();
    let ctx = rustcms_admin::Ctx::new(fx.db.conn().clone(), editor("7").without_all(), registry.clone());
    let notes = registry.get("view_only_notes").unwrap();
    let note = notes.create(&ctx, data(json!({"title": "Secret"}))).await.unwrap();
    notes.delete(&ctx, &note.key).await.unwrap();
    let page = notes.list(&ctx, &trashed(Trashed::Only)).await.unwrap();
    assert_eq!((page.meta.total, page.data.len()), (0, 0), "viewing alone does not open the trash");
    assert_eq!(notes.list(&ctx, &trashed(Trashed::With)).await.unwrap().meta.total, 0);
    assert!(matches!(notes.restore(&ctx, &note.key).await, Err(AppError::Forbidden)));
});

db_test!(restore_requires_visible_relationships, |db: TestDb| async move {
    let fx = Fixture::new(db).await;
    let admin_ctx = fx.ctx(admin());
    let category = fx.seed_category("Plain").await;
    let notes = fx.resource("notes");
    let note = notes
        .create(&admin_ctx, data(json!({"title": "Linked", "category_id": category.parse::<i32>().unwrap()})))
        .await;
    let note = note.unwrap();
    notes.delete(&admin_ctx, &note.key).await.unwrap();
    // The category is now hidden from editors (`Hidden…` needs `categories.view_hidden`).
    fx.resource("categories").update(&admin_ctx, &category, data(json!({"name": "Hidden plain"}))).await.unwrap();

    let restorer = fx.ctx(editor("7").with(&["notes.view_any", "notes.view", "notes.restore"]));
    assert_eq!(field_errors(notes.restore(&restorer, &note.key).await.unwrap_err()), ["category_id"]);
    assert_eq!(notes.list(&admin_ctx, &trashed(Trashed::Only)).await.unwrap().meta.total, 1, "still trashed");
    // Reverse check: whoever sees the category restores it.
    notes.restore(&admin_ctx, &note.key).await.unwrap();
});

db_test!(trashed_records_keep_their_unique_values, |db: TestDb| async move {
    let fx = Fixture::new(db).await;
    let ctx = fx.ctx(admin());
    let notes = fx.resource("notes");
    let note = notes.create(&ctx, data(json!({"title": "Once"}))).await.unwrap();
    notes.delete(&ctx, &note.key).await.unwrap();
    let err = notes.create(&ctx, data(json!({"title": "Once"}))).await.unwrap_err();
    assert_eq!(field_errors(err), ["title"]);
});

db_test!(soft_delete_permissions_are_declared, |db: TestDb| async move {
    let fx = Fixture::new(db).await;
    let permissions = fx.resource("notes").permissions();
    assert!(
        permissions.contains(&"notes.restore".to_owned()) && permissions.contains(&"notes.force_delete".to_owned())
    );
    let posts = fx.resource("posts").permissions();
    assert!(!posts.iter().any(|p| p.ends_with(".restore") || p.ends_with(".force_delete")));
});

/// A hook that writes the marker directly.
struct SneakyNotes;

#[async_trait::async_trait]
impl rustcms_admin::Resource for SneakyNotes {
    type Entity = note::Entity;
    const SLUG: &'static str = "sneaky_notes";

    fn meta() -> rustcms_admin::ResourceMeta {
        rustcms_admin::ResourceMeta::new("Note").soft_deletes()
    }

    fn fields() -> Vec<rustcms_admin::Field> {
        vec![rustcms_admin::Field::make("title")]
    }

    async fn mutate_before_create(
        _ctx: &rustcms_admin::Ctx,
        _txn: &sea_orm::DatabaseTransaction,
        mut data: Data,
    ) -> rustcms_core::AppResult<Data> {
        data.insert("deleted_at".into(), json!("2026-01-01T00:00:00Z"));
        Ok(data)
    }
}

db_test!(hooks_cannot_write_the_soft_delete_marker, |db: TestDb| async move {
    let fx = Fixture::new(db).await;
    let registry = rustcms_admin::Registry::new().register::<SneakyNotes>().finish();
    let ctx = rustcms_admin::Ctx::new(fx.db.conn().clone(), admin(), registry.clone());
    let err = registry.get("sneaky_notes").unwrap().create(&ctx, data(json!({"title": "x"}))).await.unwrap_err();
    assert!(matches!(err, AppError::Internal(_)), "{err:?}");
});

struct WritableMarker;

impl rustcms_admin::Resource for WritableMarker {
    type Entity = note::Entity;
    const SLUG: &'static str = "writable_marker";

    fn meta() -> rustcms_admin::ResourceMeta {
        rustcms_admin::ResourceMeta::new("Note").soft_deletes()
    }

    fn fields() -> Vec<rustcms_admin::Field> {
        vec![rustcms_admin::Field::make("title"), rustcms_admin::Field::make("deleted_at")]
    }
}

#[test]
#[should_panic(expected = "may only be a `guarded()` field")]
fn the_marker_cannot_be_a_writable_field() {
    let _ = rustcms_admin::Registry::new().register::<WritableMarker>();
}

struct TextMarker;

impl rustcms_admin::Resource for TextMarker {
    type Entity = note::Entity;
    const SLUG: &'static str = "text_marker";

    fn meta() -> rustcms_admin::ResourceMeta {
        rustcms_admin::ResourceMeta::new("Note").soft_deletes_column("title")
    }

    fn fields() -> Vec<rustcms_admin::Field> {
        vec![rustcms_admin::Field::make("category_id")]
    }
}

#[test]
#[should_panic(expected = "must be nullable")]
fn the_marker_must_be_a_nullable_timestamp() {
    let _ = rustcms_admin::Registry::new().register::<TextMarker>();
}

pub mod memo {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "memos")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i32,
        pub title: String,
        pub removed_at: Option<DateTime>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

struct Memos;

impl rustcms_admin::Resource for Memos {
    type Entity = memo::Entity;
    const SLUG: &'static str = "memos";

    fn meta() -> rustcms_admin::ResourceMeta {
        rustcms_admin::ResourceMeta::new("Memo").soft_deletes_column("removed_at")
    }

    fn fields() -> Vec<rustcms_admin::Field> {
        vec![rustcms_admin::Field::make("title")]
    }
}

db_test!(a_custom_local_datetime_marker_works, |db: TestDb| async move {
    use sea_orm::{EntityTrait, Schema};
    let backend = db.conn().get_database_backend();
    rustcms_testing::create_table(db.conn(), &Schema::new(backend).create_table_from_entity(memo::Entity)).await;
    rustcms_testing::create_table(
        db.conn(),
        &Schema::new(backend)
            .create_table_from_entity(rustcms_core::models::audit_log::Entity)
            .if_not_exists()
            .to_owned(),
    )
    .await;
    let registry = rustcms_admin::Registry::new().register::<Memos>().finish();
    let ctx = rustcms_admin::Ctx::new(db.conn().clone(), admin(), registry.clone());
    let memos = registry.get("memos").unwrap();
    let memo = memos.create(&ctx, data(json!({"title": "m"}))).await.unwrap();
    memos.delete(&ctx, &memo.key).await.unwrap();
    let stored = memo::Entity::find().one(db.conn()).await.unwrap().unwrap();
    assert!(stored.removed_at.is_some());
    assert_eq!(memos.list(&ctx, &ListQuery::default()).await.unwrap().meta.total, 0);
    memos.restore(&ctx, &memo.key).await.unwrap();
    assert_eq!(memos.list(&ctx, &ListQuery::default()).await.unwrap().meta.total, 1);
});

/// Inserts a media row as if uploaded by `owner` (no file needed for reference checks).
async fn media(fx: &Fixture, owner: i64, mime: &str, size: i64) -> String {
    use rustcms_admin::media::entity;
    use sea_orm::{ActiveModelTrait, ActiveValue::Set};
    let id = uuid::Uuid::now_v7();
    entity::ActiveModel {
        id: Set(id),
        disk: Set("local".into()),
        key: Set(format!("2026/10/{id}.bin")),
        name: Set("f".into()),
        mime: Set(mime.into()),
        size: Set(size),
        sha256: Set("0".repeat(64)),
        width: Set(None),
        height: Set(None),
        owner_id: Set(owner),
        created_at: Set(chrono::Utc::now()),
        deleted_at: Set(None),
    }
    .insert(fx.db.conn())
    .await
    .unwrap();
    id.to_string()
}

db_test!(media_references_are_checked_on_every_write, |db: TestDb| async move {
    let fx = Fixture::new(db).await;
    let notes = fx.resource("notes");
    let writer = fx.ctx(editor("7").with(&["notes.create", "notes.view", "media.view_any", "media.view"]));
    let mine = media(&fx, 7, "image/png", 100).await;
    let theirs = media(&fx, 8, "image/png", 100).await;
    let pdf = media(&fx, 7, "application/pdf", 100).await;
    let big = media(&fx, 7, "image/png", 20_000).await;
    let missing = uuid::Uuid::now_v7().to_string();

    let created = notes.create(&writer, data(json!({"title": "A", "cover": mine, "attachments": [pdf, mine]}))).await;
    assert_eq!(created.unwrap().data["cover"], json!(mine));
    for (field, value) in [
        ("cover", json!(theirs)),
        ("cover", json!(missing)),
        ("cover", json!(pdf)),
        ("cover", json!(big)),
        ("cover", json!("not-a-uuid")),
        ("attachments", json!([mine, mine])),
        ("attachments", json!([mine, pdf, big])),
        ("attachments", json!("not-a-list")),
    ] {
        let err = notes.create(&writer, data(json!({"title": format!("{field}-{value}"), field: value}))).await;
        assert_eq!(field_errors(err.unwrap_err()), [field], "{field} = {value}");
    }
    // Without `media.view`, not even one's own uploads can be referenced.
    let blind = fx.ctx(editor("7").with(&["notes.create"]));
    assert_eq!(
        field_errors(notes.create(&blind, data(json!({"title": "B", "cover": mine}))).await.unwrap_err()),
        ["cover"]
    );

    // A trashed media item cannot be referenced, nor a record restored onto it.
    let admin_ctx = fx.ctx(admin());
    let note = notes.create(&writer, data(json!({"title": "C", "cover": mine}))).await.unwrap();
    notes.delete(&admin_ctx, &note.key).await.unwrap();
    fx.resource("media").delete(&admin_ctx, &mine).await.unwrap();
    assert_eq!(
        field_errors(notes.create(&writer, data(json!({"title": "D", "cover": mine}))).await.unwrap_err()),
        ["cover"]
    );
    assert_eq!(field_errors(notes.restore(&admin_ctx, &note.key).await.unwrap_err()), ["cover"]);
});

db_test!(rich_text_is_a_validated_document_with_visible_images, |db: TestDb| async move {
    let fx = Fixture::new(db).await;
    let notes = fx.resource("notes");
    let writer = fx.ctx(editor("7").with(&["notes.create", "notes.view", "media.view_any", "media.view"]));
    let image = media(&fx, 7, "image/png", 10).await;
    let pdf = media(&fx, 7, "application/pdf", 10).await;
    let theirs = media(&fx, 8, "image/png", 10).await;
    let body = |media: &str| {
        json!({"type": "doc", "version": 1, "content": [
            {"type": "paragraph", "content": [{"type": "text", "text": "Hi"}]},
            {"type": "image", "attrs": {"media": media, "alt": "x"}}
        ]})
    };
    let created = notes.create(&writer, data(json!({"title": "Doc", "body": body(&image)}))).await.unwrap();
    assert_eq!(created.data["body"], body(&image));
    for (title, value) in [
        ("pdf", body(&pdf)),
        ("theirs", body(&theirs)),
        ("html", json!("<p>Hi</p>")),
        ("script", json!({"type": "doc", "version": 1, "content": [{"type": "script"}]})),
    ] {
        let err = notes.create(&writer, data(json!({"title": title, "body": value}))).await.unwrap_err();
        assert_eq!(field_errors(err), ["body"], "{title}");
    }

    // Version 2: captions, widths, tables, embeds and callouts; its images follow the same rules.
    let v2 = |media: &str, provider: &str, id: &str| {
        let cell = |kind: &str| json!({"type": kind, "content": [{"type": "paragraph", "content": [{"type": "text", "text": "c"}]}]});
        json!({"type": "doc", "version": 2, "content": [
            {"type": "image", "attrs": {"media": media, "alt": "x", "caption": "Cap", "width": "full"}},
            {"type": "table", "content": [
                {"type": "tableRow", "content": [cell("tableHeader"), cell("tableHeader")]},
                {"type": "tableRow", "content": [cell("tableCell"), cell("tableCell")]}
            ]},
            {"type": "embed", "attrs": {"provider": provider, "id": id}},
            {"type": "callout", "attrs": {"tone": "info"}, "content": [{"type": "paragraph"}]}
        ]})
    };
    let created = notes
        .create(&writer, data(json!({"title": "V2", "body": v2(&image, "youtube", "dQw4w9WgXcQ")})))
        .await
        .unwrap();
    assert_eq!(created.data["body"], v2(&image, "youtube", "dQw4w9WgXcQ"));
    for (title, value) in [
        ("v2 theirs", v2(&theirs, "youtube", "dQw4w9WgXcQ")),
        ("v2 pdf", v2(&pdf, "vimeo", "1")),
        ("v2 provider", v2(&image, "evil", "dQw4w9WgXcQ")),
        ("v2 x", v2(&image, "x", "1460323737035677698")),
        ("v2 id", v2(&image, "youtube", "javascript:x")),
        ("v1 table", {
            let mut doc = v2(&image, "youtube", "dQw4w9WgXcQ");
            doc["version"] = json!(1);
            doc
        }),
    ] {
        let err = notes.create(&writer, data(json!({"title": title, "body": value}))).await.unwrap_err();
        assert_eq!(field_errors(err), ["body"], "{title}");
    }
});
