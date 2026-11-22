//! Media collections (vocabulary 23): declared by the panel, set by uploads
//! (`collection` part), changed on the record, filtered on the Media page and
//! in the picker (`~none`: no collection). Never a boundary of access.

mod support;

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use base64::Engine;
use rustcms_admin::media::entity as media;
use rustcms_admin::{AdminProvider, Ctx, Data, ListQuery, Registry, SchemaOperation};
use rustcms_core::config::Config;
use rustcms_core::{AppBuilder, AppError, Principal};
use rustcms_testing::{TestApp, TestClient, TestDb, TestResponse, db_test};
use sea_orm::{ActiveModelTrait, ActiveValue::Set, EntityTrait};
use serde_json::json;
use support::PrincipalExt;

/// A valid 1×1 PNG.
fn png() -> Vec<u8> {
    base64::engine::general_purpose::STANDARD
        .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=")
        .unwrap()
}

/// A multipart body with `parts` of `(name, filename, bytes)` (no filename: a text part).
fn multipart(parts: &[(&str, Option<&str>, &[u8])]) -> (String, Vec<u8>) {
    let boundary = "rustcms-test-boundary";
    let mut body = Vec::new();
    for (name, filename, bytes) in parts {
        let disposition = match filename {
            Some(file) => {
                format!("form-data; name=\"{name}\"; filename=\"{file}\"\r\nContent-Type: application/octet-stream")
            }
            None => format!("form-data; name=\"{name}\""),
        };
        body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: {disposition}\r\n\r\n").as_bytes());
        body.extend_from_slice(bytes);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

async fn upload(
    app: &TestApp,
    client: &TestClient<'_>,
    parts: &[(&str, Option<&str>, &[u8])],
    key: Option<&str>,
) -> TestResponse {
    client.fetch_csrf().await;
    let (content_type, body) = multipart(parts);
    let mut request = Request::builder()
        .method(Method::POST)
        .uri(app.admin_api("media"))
        .header("content-type", content_type)
        .header("content-length", body.len());
    if let Some(key) = key {
        request = request.header("idempotency-key", key);
    }
    client.request(request.body(Body::from(body)).unwrap()).await
}

fn declared(admin: AdminProvider) -> AdminProvider {
    admin.media_collection("covers", "Covers").media_collection("docs.files", "Documents")
}

fn build(config: Config) -> AppBuilder {
    support::app::build_extended(config, declared)
}

async fn collection_of(app: &TestApp, id: &str) -> Option<String> {
    let id = uuid::Uuid::parse_str(id).unwrap();
    media::Entity::find_by_id(id).one(app.state().db()).await.unwrap().unwrap().collection
}

db_test!(uploads_go_into_declared_collections, |db: TestDb| async move {
    let config = rustcms_testing::test_config(db.url());
    let (app, users) = support::app::setup_built(db, config, build).await;
    let admin = app.acting_as(&users.admin).await;
    let file = png();

    // Declared: stored and returned; the part may come before the file.
    let sent = upload(&app, &admin, &[("collection", None, b"covers"), ("file", Some("a.png"), &file)], None).await;
    sent.assert_status(StatusCode::CREATED);
    let id = sent.json()["data"]["id"].as_str().unwrap().to_owned();
    assert_eq!(sent.json()["data"]["collection"], json!("covers"));
    assert_eq!(collection_of(&app, &id).await.as_deref(), Some("covers"));
    let after =
        upload(&app, &admin, &[("file", Some("b.png"), &file), ("collection", None, b"docs.files")], None).await;
    let id = after.json()["data"]["id"].as_str().unwrap().to_owned();
    assert_eq!(collection_of(&app, &id).await.as_deref(), Some("docs.files"));

    // None sent, or an empty part: no collection.
    for parts in
        [&[("file", Some("c.png"), &file[..])][..], &[("file", Some("d.png"), &file), ("collection", None, b"")]]
    {
        let sent = upload(&app, &admin, parts, None).await;
        sent.assert_status(StatusCode::CREATED);
        assert!(sent.json()["data"].get("collection").is_none());
        let id = sent.json()["data"]["id"].as_str().unwrap().to_owned();
        assert_eq!(collection_of(&app, &id).await, None);
    }

    // Undeclared, too long, twice, or another part: refused, nothing stored.
    let before = media::Entity::find().all(app.state().db()).await.unwrap().len();
    for name in [&b"posters"[..], b"Covers", &[b'a'; 65]] {
        upload(&app, &admin, &[("file", Some("e.png"), &file), ("collection", None, name)], None)
            .await
            .assert_validation_error("collection");
    }
    upload(
        &app,
        &admin,
        &[("collection", None, b"covers"), ("collection", None, b"covers"), ("file", Some("f.png"), &file)],
        None,
    )
    .await
    .assert_validation_error("collection");
    upload(&app, &admin, &[("file", Some("g.png"), &file), ("other", None, b"x")], None)
        .await
        .assert_validation_error("file");
    upload(&app, &admin, &[("collection", None, b"covers")], None).await.assert_validation_error("file");
    assert_eq!(media::Entity::find().all(app.state().db()).await.unwrap().len(), before);

    // Idempotency: the collection is part of the request.
    let first =
        upload(&app, &admin, &[("file", Some("h.png"), &file), ("collection", None, b"covers")], Some("k-1")).await;
    first.assert_status(StatusCode::CREATED);
    let again =
        upload(&app, &admin, &[("file", Some("h.png"), &file), ("collection", None, b"covers")], Some("k-1")).await;
    assert_eq!(again.json()["data"]["id"], first.json()["data"]["id"], "a retry answers the same file");
    let other =
        upload(&app, &admin, &[("file", Some("h.png"), &file), ("collection", None, b"docs.files")], Some("k-1")).await;
    assert_ne!(other.status, StatusCode::CREATED, "the same key for another collection is another request");
});

db_test!(the_panel_lists_collections_and_the_media_schema_filters_them, |db: TestDb| async move {
    let config = rustcms_testing::test_config(db.url());
    let (app, users) = support::app::setup_built(db, config, build).await;
    let admin = app.acting_as(&users.admin).await;
    let panel = admin.get(&app.admin_api("panel")).await.json();
    assert_eq!(
        panel["mediaCollections"],
        json!([{"value": "covers", "label": "Covers"}, {"value": "docs.files", "label": "Documents"}])
    );
    // Users who may neither list nor upload files get none.
    let member = app.acting_as(&users.member).await;
    assert!(member.get(&app.admin_api("panel")).await.json().get("mediaCollections").is_none());

    let schema = admin.get(&app.admin_api("resources/media/schema?operation=view")).await.json();
    let filter = schema["table"]["filters"].as_array().unwrap().iter().find(|f| f["name"] == "collection").cloned();
    let filter = filter.expect("collection filter");
    assert_eq!(filter["none"], json!("Unassigned"));
    assert_eq!(filter["options"][0], json!({"value": "covers", "label": "Covers"}));
    let column = schema["table"]["columns"].as_array().unwrap().iter().find(|c| c["name"] == "collection").cloned();
    assert_eq!(column.expect("collection column")["options"][1]["label"], json!("Documents"));
});

db_test!(without_collections_the_panel_is_unchanged, |db: TestDb| async move {
    let (app, users) = support::app::setup(db).await;
    let admin = app.acting_as(&users.admin).await;
    assert!(admin.get(&app.admin_api("panel")).await.json().get("mediaCollections").is_none());
    let schema = admin.get(&app.admin_api("resources/media/schema?operation=view")).await.json();
    assert!(schema["table"]["columns"].is_array(), "{schema}");
    assert!(!schema.to_string().contains("\"collection\""), "no column, filter or input: {schema}");
    let edit = admin.get(&app.admin_api("resources/media/schema?operation=edit")).await.json();
    assert!(edit.to_string().contains("\"alt\""), "{edit}");
    assert!(!edit.to_string().contains("\"collection\""), "{edit}");
    // Any name is refused.
    admin.fetch_csrf().await;
    upload(&app, &admin, &[("file", Some("a.png"), &png()), ("collection", None, b"covers")], None)
        .await
        .assert_validation_error("collection");
});

/// A media row owned by `owner` in `collection`, inserted directly.
async fn stored(db: &sea_orm::DatabaseConnection, owner: i64, name: &str, collection: Option<&str>) -> String {
    let id = uuid::Uuid::now_v7();
    media::ActiveModel {
        id: Set(id),
        disk: Set("local".into()),
        key: Set(format!("media/{id}")),
        name: Set(name.into()),
        mime: Set("image/png".into()),
        size: Set(100),
        sha256: Set("0".repeat(64)),
        width: Set(Some(1)),
        height: Set(Some(1)),
        alt: Set(None),
        collection: Set(collection.map(str::to_owned)),
        owner_id: Set(owner),
        created_at: Set(chrono::Utc::now()),
        deleted_at: Set(None),
    }
    .insert(db)
    .await
    .unwrap();
    id.to_string()
}

async fn registry(db: &TestDb) -> Arc<Registry> {
    support::create_tables(db.conn()).await;
    Registry::new()
        .media_collection("covers", "Covers")
        .media_collection("docs", "Documents")
        .register::<rustcms_admin::media::MediaResource>()
        .finish()
}

fn user(db: &TestDb, registry: &Arc<Registry>, id: &str, permissions: &[&str]) -> Ctx {
    let principal = Principal { id: id.into(), guard: "admin".into(), ..Default::default() };
    Ctx::new(db.conn().clone(), principal.with(&["media.view_any", "media.view"]).with(permissions), registry.clone())
}

fn query(value: &str) -> ListQuery {
    ListQuery {
        filters: vec![rustcms_admin::Filter {
            field: "collection".into(),
            op: rustcms_admin::FilterOp::In,
            value: json!(value),
        }],
        from_query_string: true,
        ..Default::default()
    }
}

db_test!(lists_filter_by_collection_within_the_scope, |db: TestDb| async move {
    let registry = registry(&db).await;
    let library = registry.get("media").unwrap();
    stored(db.conn(), 7, "mine-cover.png", Some("covers")).await;
    stored(db.conn(), 7, "mine-doc.png", Some("docs")).await;
    stored(db.conn(), 7, "mine-loose.png", None).await;
    stored(db.conn(), 1, "their-cover.png", Some("covers")).await;
    stored(db.conn(), 1, "their-loose.png", None).await;
    let names = async |ctx: &Ctx, q: ListQuery| {
        let page = library.list(ctx, &q).await.unwrap();
        let mut names: Vec<String> = page.data.iter().map(|e| e.data["name"].as_str().unwrap().to_owned()).collect();
        names.sort();
        names
    };
    // Own files only: the collection never widens the scope.
    let own = user(&db, &registry, "7", &[]);
    assert_eq!(names(&own, query("covers")).await, ["mine-cover.png"]);
    assert_eq!(names(&own, query("~none")).await, ["mine-loose.png"]);
    assert_eq!(names(&own, query("covers,~none")).await, ["mine-cover.png", "mine-loose.png"]);
    assert_eq!(names(&own, query("docs,covers")).await, ["mine-cover.png", "mine-doc.png"]);
    let all = user(&db, &registry, "7", &["media.view_all"]);
    assert_eq!(names(&all, query("covers")).await, ["mine-cover.png", "their-cover.png"]);
    assert_eq!(names(&all, query("~none")).await, ["mine-loose.png", "their-loose.png"]);
    // JSON lists (Rust callers): the same token.
    let json_query = ListQuery {
        filters: vec![rustcms_admin::Filter {
            field: "collection".into(),
            op: rustcms_admin::FilterOp::In,
            value: json!(["~none", "docs"]),
        }],
        ..Default::default()
    };
    assert_eq!(names(&own, json_query).await, ["mine-doc.png", "mine-loose.png"]);
});

db_test!(owners_move_files_between_collections, |db: TestDb| async move {
    let registry = registry(&db).await;
    let library = registry.get("media").unwrap();
    let mine = stored(db.conn(), 7, "mine.png", None).await;
    let theirs = stored(db.conn(), 1, "theirs.png", Some("covers")).await;
    let old = stored(db.conn(), 7, "old.png", Some("retired")).await;
    let input = |value: serde_json::Value| -> Data { value.as_object().unwrap().clone() };
    let owner = user(&db, &registry, "7", &["media.update"]);

    let saved = library.update(&owner, &mine, input(json!({"collection": "covers"}))).await.unwrap();
    assert_eq!(saved.data["collection"], json!("covers"));
    let saved = library.update(&owner, &mine, input(json!({"collection": ""}))).await.unwrap();
    assert_eq!(saved.data["collection"], json!(null), "empty is none");
    library.update(&owner, &mine, input(json!({"collection": "docs"}))).await.unwrap();
    let saved = library.update(&owner, &mine, input(json!({"collection": null}))).await.unwrap();
    assert_eq!(saved.data["collection"], json!(null));
    let refused = library.update(&owner, &mine, input(json!({"collection": "posters"}))).await.unwrap_err();
    assert!(matches!(refused, AppError::Validation(ref v) if v.errors.contains_key("collection")), "{refused:?}");
    // A collection no longer declared stays while it is not changed.
    let kept = library.update(&owner, &old, input(json!({"collection": "retired", "alt": "Old"}))).await.unwrap();
    assert_eq!(kept.data["collection"], json!("retired"));

    // Another user's file: `media.view_all` sees it, only `media.manage_all` moves it.
    let viewer = user(&db, &registry, "7", &["media.update", "media.view_all"]);
    let refused = library.update(&viewer, &theirs, input(json!({"collection": "docs"}))).await;
    assert!(matches!(refused, Err(AppError::Forbidden)), "{refused:?}");
    let manager = user(&db, &registry, "7", &["media.update", "media.manage_all"]);
    library.update(&manager, &theirs, input(json!({"collection": "docs"}))).await.unwrap();

    // The edit form offers the declared collections, "Unassigned" when empty.
    let schema = library.schema(&owner, SchemaOperation::Edit, Some(&mine)).await.unwrap();
    let schema = serde_json::to_value(schema).unwrap();
    let input = schema["form"]["components"].as_array().unwrap().iter().find(|c| c["name"] == "collection").cloned();
    let input = input.expect("collection input");
    assert_eq!(input["placeholder"], json!("Unassigned"));
    assert_eq!(input["options"].as_array().unwrap().len(), 2);
});

mod boot {
    use super::*;

    pub mod album {
        use sea_orm::entity::prelude::*;

        #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
        #[sea_orm(table_name = "albums")]
        pub struct Model {
            #[sea_orm(primary_key)]
            pub id: i32,
            pub title: String,
            pub photo: Option<Uuid>,
        }

        #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
        pub enum Relation {}

        impl ActiveModelBehavior for ActiveModel {}
    }

    macro_rules! album {
        ($resource:ident, $photo:expr, $title:expr) => {
            pub struct $resource;

            impl rustcms_admin::Resource for $resource {
                type Entity = album::Entity;
                const SLUG: &'static str = "albums";

                fn meta() -> rustcms_admin::ResourceMeta {
                    rustcms_admin::ResourceMeta::new("Album")
                }

                fn fields() -> Vec<rustcms_admin::Field> {
                    vec![$title, $photo]
                }
            }
        };
    }

    use rustcms_admin::Field;
    album!(Good, Field::column(album::Column::Photo).media().collection("covers"), Field::column(album::Column::Title));
    album!(
        Undeclared,
        Field::column(album::Column::Photo).media().collection("posters"),
        Field::column(album::Column::Title)
    );
    album!(
        NotMedia,
        Field::column(album::Column::Photo).media(),
        Field::column(album::Column::Title).collection("covers")
    );

    #[test]
    fn collections_are_declared_and_used_by_media_fields() {
        let registry = Registry::new().media_collection("covers", "Covers").register::<Good>().finish();
        assert_eq!(registry.media_collections().len(), 1);
        let panics = |f: fn()| std::panic::catch_unwind(f).unwrap_err();
        let message = |e: Box<dyn std::any::Any + Send>| {
            e.downcast_ref::<String>().cloned().or_else(|| e.downcast_ref::<&str>().map(|s| (*s).to_owned())).unwrap()
        };
        let undeclared = message(panics(|| {
            Registry::new().media_collection("covers", "Covers").register::<Undeclared>().finish();
        }));
        assert!(undeclared.contains("`posters`, which is not declared"), "{undeclared}");
        let not_media = message(panics(|| {
            Registry::new().media_collection("covers", "Covers").register::<NotMedia>();
        }));
        assert!(not_media.contains("not a media or rich text field"), "{not_media}");
        for bad in ["Covers", "a b", "~none", ""] {
            let invalid = message(
                std::panic::catch_unwind(|| {
                    Registry::new().media_collection(bad, "X");
                })
                .unwrap_err(),
            );
            assert!(invalid.contains("names are 1 to 64"), "{bad}: {invalid}");
        }
        let twice = message(panics(|| {
            Registry::new().media_collection("covers", "Covers").media_collection("covers", "Again");
        }));
        assert!(twice.contains("declared twice"), "{twice}");
        let empty = message(panics(|| {
            Registry::new().media_collection("covers", " ");
        }));
        assert!(empty.contains("label is empty"), "{empty}");
        // Settings: only media settings, checked when declared.
        let setting = message(panics(|| {
            rustcms_admin::SettingField::text("title").collection("covers");
        }));
        assert!(setting.contains("only media settings"), "{setting}");
    }
}

/// What a media hook saw: (media id, event, collection, previous collection).
/// Each test reads its own file's entries: the database cases may run at once.
type Moved = (String, rustcms_admin::media::MediaEvent, Option<String>, Option<Option<String>>);

static MOVED: std::sync::Mutex<Vec<Moved>> = std::sync::Mutex::new(Vec::new());

struct MoveRecorder;

#[async_trait::async_trait]
impl rustcms_admin::media::MediaHook for MoveRecorder {
    async fn changed(
        &self,
        _: &Ctx,
        _: &sea_orm::DatabaseTransaction,
        change: &rustcms_admin::media::MediaChange<'_>,
    ) -> rustcms_core::AppResult<()> {
        if change.media.name == "hooked.png" {
            MOVED.lock().unwrap().push((
                change.media.id.to_string(),
                change.event,
                change.media.collection.clone(),
                change.previous.map(|p| p.collection.clone()),
            ));
        }
        Ok(())
    }
}

db_test!(media_hooks_see_a_file_move_between_collections, |db: TestDb| async move {
    support::create_tables(db.conn()).await;
    let registry = Registry::new()
        .media_collection("covers", "Covers")
        .media_hook(MoveRecorder)
        .register::<rustcms_admin::media::MediaResource>()
        .finish();
    let library = registry.get("media").unwrap();
    let file = stored(db.conn(), 7, "hooked.png", None).await;
    let owner = user(&db, &registry, "7", &["media.update"]);
    let input = |value: serde_json::Value| -> Data { value.as_object().unwrap().clone() };

    library.update(&owner, &file, input(json!({"collection": "covers"}))).await.unwrap();
    library.update(&owner, &file, input(json!({"collection": null}))).await.unwrap();

    let moved: Vec<_> = MOVED
        .lock()
        .unwrap()
        .iter()
        .filter(|m| m.0 == file)
        .map(|(_, event, collection, previous)| (*event, collection.clone(), previous.clone()))
        .collect();
    use rustcms_admin::media::MediaEvent::Updated;
    assert_eq!(moved, [(Updated, Some("covers".into()), Some(None)), (Updated, None, Some(Some("covers".into())))]);
});
