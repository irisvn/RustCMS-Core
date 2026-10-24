//! Media library over HTTP: uploads (content-typed, bounded, idempotent),
//! authorized downloads with safe headers, trash, and object cleanup.

mod support;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use base64::Engine;
use rustcms_admin::media::entity as media;
use rustcms_testing::{TestApp, TestClient, TestDb, TestResponse, db_test};
use sea_orm::{EntityTrait, PaginatorTrait};
use serde_json::json;
use support::app::{Users, setup};

/// A valid 1×1 PNG.
fn png() -> Vec<u8> {
    base64::engine::general_purpose::STANDARD
        .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=")
        .unwrap()
}

/// A multipart body with `parts` of `(name, filename, bytes)`.
fn multipart(parts: &[(&str, &str, &[u8])]) -> (String, Vec<u8>) {
    let boundary = "rustcms-test-boundary";
    let mut body = Vec::new();
    for (name, filename, bytes) in parts {
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"; filename=\"{filename}\"\r\n\
                 Content-Type: application/octet-stream\r\n\r\n"
            )
            .as_bytes(),
        );
        body.extend_from_slice(bytes);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

async fn upload(
    app: &TestApp,
    client: &TestClient<'_>,
    parts: &[(&str, &str, &[u8])],
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

async fn rows(app: &TestApp) -> u64 {
    media::Entity::find().count(app.state().db()).await.unwrap()
}

async fn admin_app(db: TestDb) -> (TestApp, Users) {
    setup(db).await
}

db_test!(uploads_are_typed_from_content_and_served_safely, |db: TestDb| async move {
    let (app, users) = admin_app(db).await;
    let admin = app.acting_as(&users.admin).await;
    let created = upload(&app, &admin, &[("file", "../../photo x.png", &png())], None).await;
    created.assert_status(StatusCode::CREATED);
    let item = created.json()["data"].clone();
    assert_eq!(
        (item["mime"].clone(), item["name"].clone(), item["inline"].clone()),
        (json!("image/png"), json!("photo x.png"), json!(true))
    );
    assert_eq!((item["width"].clone(), item["height"].clone()), (json!(1), json!(1)));
    assert!(item.get("key").is_none(), "the storage key is never sent");

    let id = item["id"].as_str().unwrap();
    let served = admin.get(&app.admin_api(&format!("media/{id}/file"))).await;
    served.assert_status(StatusCode::OK);
    assert_eq!(served.body.as_ref(), png().as_slice());
    let header = |name: &str| served.headers[name].to_str().unwrap().to_owned();
    assert_eq!(header("content-type"), "image/png");
    assert_eq!(header("x-content-type-options"), "nosniff");
    assert_eq!(header("content-security-policy"), "sandbox; default-src 'none'");
    assert!(header("content-disposition").starts_with("inline; filename=\"photo x.png\""));
    assert!(header("cache-control").contains("no-store"));

    // A PDF is accepted, but always downloaded.
    let pdf = upload(&app, &admin, &[("file", "doc.pdf", b"%PDF-1.7\n1 0 obj<<>>endobj\n%%EOF\n")], None).await;
    let pdf_id = pdf.json()["data"]["id"].as_str().unwrap().to_owned();
    let served = admin.get(&app.admin_api(&format!("media/{pdf_id}/file"))).await;
    assert!(served.headers["content-disposition"].to_str().unwrap().starts_with("attachment;"));
});

db_test!(uploads_refuse_what_is_not_accepted_and_store_nothing, |db: TestDb| async move {
    let (app, users) = admin_app(db).await;
    let admin = app.acting_as(&users.admin).await;
    for (name, bytes) in [
        ("x.svg", &b"<svg xmlns=\"http://www.w3.org/2000/svg\"><script>alert(1)</script></svg>"[..]),
        ("x.png", b"<!doctype html><script>alert(1)</script>"),
        ("x.png", b"plain text"),
        ("empty.png", b""),
    ] {
        upload(&app, &admin, &[("file", name, bytes)], None).await.assert_validation_error("file");
    }
    // A broken image with a PNG signature.
    upload(&app, &admin, &[("file", "x.png", b"\x89PNG\r\n\x1a\n\0\0\0\rIHDRbroken")], None)
        .await
        .assert_validation_error("file");
    // Exactly one part, named `file`.
    upload(&app, &admin, &[("file", "a.png", &png()), ("file", "b.png", &png())], None)
        .await
        .assert_validation_error("file");
    upload(&app, &admin, &[("other", "a.png", &png())], None).await.assert_validation_error("file");
    // Over the configured size (64 KiB in tests).
    let big = [png(), vec![0u8; 70 * 1024]].concat();
    upload(&app, &admin, &[("file", "big.png", &big)], None).await.assert_status(StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(rows(&app).await, 0);
});

db_test!(uploads_and_downloads_are_authorized, |db: TestDb| async move {
    let (app, users) = admin_app(db).await;
    let member = app.acting_as(&users.member).await;
    upload(&app, &member, &[("file", "a.png", &png())], None).await.assert_status(StatusCode::FORBIDDEN);
    // CSRF and Origin apply to multipart writes like any other.
    let foreign = app.acting_as(&users.admin).await.origin(Some("https://evil.example"));
    upload(&app, &foreign, &[("file", "a.png", &png())], None).await.assert_status(StatusCode::FORBIDDEN);
    assert_eq!(rows(&app).await, 0);

    let admin = app.acting_as(&users.admin).await;
    let id = upload(&app, &admin, &[("file", "a.png", &png())], None).await.json()["data"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let file = app.admin_api(&format!("media/{id}/file"));
    member.get(&file).await.assert_status(StatusCode::FORBIDDEN);
    app.client().get(&file).await.assert_status(StatusCode::UNAUTHORIZED);
    // Trashed media is not served.
    admin.send(Method::DELETE, &app.admin_api(&format!("resources/media/records/{id}")), None).await;
    admin.get(&file).await.assert_status(StatusCode::NOT_FOUND);
    // The generic endpoints never create or edit media.
    admin
        .post_json(&app.admin_api("resources/media/records"), &json!({"name": "x"}))
        .await
        .assert_status(StatusCode::FORBIDDEN);
});

db_test!(a_retried_upload_is_stored_once, |db: TestDb| async move {
    let (app, users) = admin_app(db).await;
    let admin = app.acting_as(&users.admin).await;
    let first = upload(&app, &admin, &[("file", "a.png", &png())], Some("up-1")).await;
    let again = upload(&app, &admin, &[("file", "a.png", &png())], Some("up-1")).await;
    first.assert_status(StatusCode::CREATED);
    again.assert_status(StatusCode::CREATED);
    assert_eq!(first.json()["data"]["id"], again.json()["data"]["id"]);
    assert_eq!(rows(&app).await, 1);
    // Another file with the same key: refused.
    let other = b"%PDF-1.7\n1 0 obj<<>>endobj\n%%EOF\n";
    upload(&app, &admin, &[("file", "a.pdf", other)], Some("up-1")).await.assert_validation_error("idempotency_key");
});

db_test!(deleted_media_files_are_removed_and_abandoned_uploads_cleaned, |db: TestDb| async move {
    use rustcms_core::storage::StorageKey;
    let (app, users) = admin_app(db).await;
    let admin = app.acting_as(&users.admin).await;
    let id = upload(&app, &admin, &[("file", "a.png", &png())], None).await.json()["data"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let row = media::Entity::find().one(app.state().db()).await.unwrap().unwrap();
    let storage = rustcms_core::storage::of(app.state()).unwrap();
    let key = StorageKey::parse(&row.key).unwrap();
    assert!(storage.exists(&key).await.unwrap());

    let record = app.admin_api(&format!("resources/media/records/{id}"));
    admin.send(Method::DELETE, &record, None).await.assert_status(StatusCode::NO_CONTENT);
    assert!(storage.exists(&key).await.unwrap(), "trash keeps the file");
    admin.send(Method::DELETE, &format!("{record}/force"), None).await.assert_status(StatusCode::NO_CONTENT);
    assert_eq!(rustcms_admin::media::prune(app.state().db(), storage.as_ref()).await.unwrap(), 1);
    assert!(!storage.exists(&key).await.unwrap());

    // An upload that crashed between storing the object and its row.
    let orphan = StorageKey::generate("png").unwrap();
    let staged = std::env::temp_dir().join(format!("orphan-{}", uuid::Uuid::now_v7()));
    std::fs::write(&staged, png()).unwrap();
    storage.put_file(&orphan, &staged, "image/png").await.unwrap();
    std::fs::remove_file(&staged).unwrap();
    insert_intent(&app, &orphan, chrono::Utc::now() - chrono::Duration::hours(2)).await;
    let recent = StorageKey::generate("png").unwrap();
    insert_intent(&app, &recent, chrono::Utc::now()).await;
    assert_eq!(rustcms_admin::media::prune(app.state().db(), storage.as_ref()).await.unwrap(), 1);
    assert!(!storage.exists(&orphan).await.unwrap());
});

async fn insert_intent(app: &TestApp, key: &rustcms_core::storage::StorageKey, at: chrono::DateTime<chrono::Utc>) {
    use sea_orm::ConnectionTrait;
    let insert = sea_orm::sea_query::Query::insert()
        .into_table(sea_orm::sea_query::Alias::new("media_uploads"))
        .columns(["id", "disk", "key", "created_at"].map(sea_orm::sea_query::Alias::new))
        .values_panic([uuid::Uuid::now_v7().into(), "local".into(), key.as_str().into(), at.into()])
        .to_owned();
    app.state().db().execute(&insert).await.unwrap();
}

db_test!(forged_or_truncated_files_are_refused, |db: TestDb| async move {
    let (app, users) = admin_app(db).await;
    let admin = app.acting_as(&users.admin).await;
    // A PNG header with plausible dimensions, and nothing else.
    let mut forged = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    forged.extend_from_slice(&[0, 0, 0, 1, 0, 0, 0, 1]);
    let truncated = png()[..png().len() - 8].to_vec();
    for bytes in [forged, truncated, b"%PDF-1.7\nno end marker".to_vec()] {
        upload(&app, &admin, &[("file", "x.png", &bytes)], None).await.assert_validation_error("file");
    }
    let long = format!("{}.png", "n".repeat(300));
    upload(&app, &admin, &[("file", &long, &png())], None).await.assert_validation_error("file");
    assert_eq!(rows(&app).await, 0);
});

db_test!(pruning_never_deletes_a_committed_file, |db: TestDb| async move {
    let (app, users) = admin_app(db).await;
    let admin = app.acting_as(&users.admin).await;
    upload(&app, &admin, &[("file", "a.png", &png())], None).await.assert_status(StatusCode::CREATED);
    // A committed row's object is never pruned, even if a stale intent names it.
    let row = media::Entity::find().one(app.state().db()).await.unwrap().unwrap();
    let key = rustcms_core::storage::StorageKey::parse(&row.key).unwrap();
    insert_intent(&app, &key, chrono::Utc::now() - chrono::Duration::hours(2)).await;
    let storage = rustcms_core::storage::of(app.state()).unwrap();
    rustcms_admin::media::prune(app.state().db(), storage.as_ref()).await.unwrap();
    assert!(storage.exists(&key).await.unwrap(), "used by a committed row");
});

/// A media row owned by `owner`, inserted directly.
async fn stored_media(db: &sea_orm::DatabaseConnection, owner: i64, name: &str) -> String {
    use sea_orm::{ActiveModelTrait, ActiveValue::Set};
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
        owner_id: Set(owner),
        created_at: Set(chrono::Utc::now()),
        deleted_at: Set(None),
    }
    .insert(db)
    .await
    .unwrap();
    id.to_string()
}

db_test!(view_all_shows_and_references_every_file_without_managing_others, |db: TestDb| async move {
    use rustcms_admin::{Data, ListQuery, Trashed};
    use rustcms_core::{AppError, Principal};
    use support::{Fixture, PrincipalExt, admin};
    let fx = Fixture::new(db).await;
    let library = fx.resource("media");
    let notes = fx.resource("notes");
    let theirs = stored_media(fx.db.conn(), 1, "theirs.png").await;
    let mine = stored_media(fx.db.conn(), 7, "mine.png").await;
    let user = |extra: &[&str]| {
        let base = ["media.view_any", "media.view", "media.delete", "media.restore", "media.force_delete"];
        let principal = Principal { id: "7".into(), guard: "admin".into(), ..Default::default() };
        fx.ctx(principal.with(&base).with(&["notes.view_any", "notes.view", "notes.create"]).with(extra))
    };
    let cover =
        |id: &str| -> Data { json!({"title": format!("cover {id}"), "cover": id}).as_object().unwrap().clone() };

    // Own files only, by default: others' are neither shown nor referenceable.
    let own = user(&[]);
    assert_eq!(library.list(&own, &ListQuery::default()).await.unwrap().meta.total, 1);
    assert!(matches!(library.find(&own, &theirs).await, Err(AppError::NotFound)));
    let refused = notes.create(&own, cover(&theirs)).await.unwrap_err();
    assert!(matches!(refused, AppError::Validation(ref v) if v.errors.contains_key("cover")), "{refused:?}");

    // `media.view_all`: every file is listed, read and referenced…
    let viewer = user(&["media.view_all"]);
    assert_eq!(library.list(&viewer, &ListQuery::default()).await.unwrap().meta.total, 2);
    let found = library.find(&viewer, &theirs).await.unwrap();
    assert!(!found.abilities["delete"], "others' files are not offered for trashing");
    notes.create(&viewer, cover(&theirs)).await.unwrap();
    // …but another user's file is never trashed, restored or deleted.
    assert!(matches!(library.delete(&viewer, &theirs).await, Err(AppError::Forbidden)));
    library.delete(&admin_ctx(&fx), &theirs).await.unwrap();
    assert!(matches!(library.restore(&viewer, &theirs).await, Err(AppError::Forbidden)));
    assert!(matches!(library.force_delete(&viewer, &theirs).await, Err(AppError::Forbidden)));
    let trash = library.list(&viewer, &ListQuery { trashed: Trashed::Only, ..Default::default() }).await.unwrap();
    assert_eq!(trash.meta.total, 0, "others' trashed files are not in the user's trash");
    // Own files: as before.
    library.delete(&viewer, &mine).await.unwrap();
    library.restore(&viewer, &mine).await.unwrap();

    // `media.manage_all` still manages them.
    let manager = user(&["media.manage_all"]);
    library.restore(&manager, &theirs).await.unwrap();
    library.delete(&manager, &theirs).await.unwrap();

    fn admin_ctx(fx: &Fixture) -> rustcms_admin::Ctx {
        fx.ctx(admin())
    }
});

db_test!(the_media_schema_says_who_may_upload, |db: TestDb| async move {
    use rustcms_admin::SchemaOperation;
    use rustcms_core::Principal;
    use support::{Fixture, PrincipalExt};
    let fx = Fixture::new(db).await;
    let library = fx.resource("media");
    let user = |extra: &[&str]| {
        let principal = Principal { id: "7".into(), guard: "admin".into(), ..Default::default() };
        fx.ctx(principal.with(&["media.view_any", "media.view"]).with(extra))
    };
    let schema = library.schema(&user(&["media.create"]), SchemaOperation::View, None).await.unwrap();
    assert_eq!(
        (schema.abilities["upload"], schema.abilities["create"]),
        (true, false),
        "uploads, never the generic create"
    );
    let schema = library.schema(&user(&[]), SchemaOperation::View, None).await.unwrap();
    assert!(!schema.abilities["upload"]);
    // Other resources have no such ability.
    let notes = fx.resource("notes").schema(&fx.ctx(support::admin()), SchemaOperation::View, None).await.unwrap();
    assert!(!notes.abilities.contains_key("upload"));
});

db_test!(owners_rename_files_and_set_their_alt_others_need_manage_all, |db: TestDb| async move {
    use rustcms_admin::Data;
    use rustcms_core::{AppError, Principal};
    use support::{Fixture, PrincipalExt};
    let fx = Fixture::new(db).await;
    let library = fx.resource("media");
    let theirs = stored_media(fx.db.conn(), 1, "theirs.png").await;
    let mine = stored_media(fx.db.conn(), 7, "mine.png").await;
    let user = |extra: &[&str]| {
        let principal = Principal { id: "7".into(), guard: "admin".into(), ..Default::default() };
        fx.ctx(principal.with(&["media.view_any", "media.view"]).with(extra))
    };
    let input = |value: serde_json::Value| -> Data { value.as_object().unwrap().clone() };

    // The owner, with `media.update`: renamed (cleaned like an upload) and described.
    let owner = user(&["media.update"]);
    let saved = library.update(&owner, &mine, input(json!({"name": "../x\"y.png", "alt": "A cat"}))).await.unwrap();
    assert_eq!((&saved.data["name"], &saved.data["alt"]), (&json!("xy.png"), &json!("A cat")));
    let found = library.find(&owner, &mine).await.unwrap();
    assert!(found.abilities["update"]);
    assert_eq!(found.data["alt"], json!("A cat"));
    // Cleared to `null`; bounded; required name.
    library.update(&owner, &mine, input(json!({"alt": null}))).await.unwrap();
    // Blank alternative text is none.
    let blank = library.update(&owner, &mine, input(json!({"alt": "  "}))).await.unwrap();
    assert_eq!(blank.data["alt"], json!(null));
    let long = library.update(&owner, &mine, input(json!({"alt": "x".repeat(501)}))).await.unwrap_err();
    assert!(matches!(long, AppError::Validation(ref v) if v.errors.contains_key("alt")), "{long:?}");
    for name in ["", " ", "../"] {
        let empty = library.update(&owner, &mine, input(json!({"name": name}))).await.unwrap_err();
        assert!(matches!(empty, AppError::Validation(ref v) if v.errors.contains_key("name")), "{name}: {empty:?}");
    }
    // Nothing else is editable.
    for field in ["mime", "size", "owner_id", "width"] {
        let refused = library.update(&owner, &mine, input(json!({field: "1"}))).await.unwrap_err();
        assert!(matches!(refused, AppError::Validation(ref v) if v.errors.contains_key(field)), "{field}: {refused:?}");
    }
    // Without `media.update`, not even one's own file.
    let reader = user(&[]);
    assert!(matches!(library.update(&reader, &mine, input(json!({"alt": "x"}))).await, Err(AppError::Forbidden)));
    assert!(!library.find(&reader, &mine).await.unwrap().abilities["update"]);
    // Another user's file: `media.view_all` shows it but may not edit it…
    let viewer = user(&["media.update", "media.view_all"]);
    assert!(!library.find(&viewer, &theirs).await.unwrap().abilities["update"]);
    let refused = library.update(&viewer, &theirs, input(json!({"name": "mine-now.png"}))).await.unwrap_err();
    assert!(matches!(refused, AppError::Forbidden), "{refused:?}");
    // …and `media.update` alone does not even see it.
    assert!(matches!(library.update(&owner, &theirs, input(json!({"alt": "x"}))).await, Err(AppError::NotFound)));
    // `media.manage_all` edits it.
    let manager = user(&["media.update", "media.manage_all"]);
    let saved =
        library.update(&manager, &theirs, input(json!({"name": "renamed.png", "alt": "Theirs"}))).await.unwrap();
    assert_eq!((&saved.data["name"], &saved.data["alt"]), (&json!("renamed.png"), &json!("Theirs")));
    // `media.manage_all` without `media.update`: no.
    let refused = library.update(&user(&["media.manage_all"]), &theirs, input(json!({"alt": "y"}))).await;
    assert!(matches!(refused, Err(AppError::Forbidden)));
    // Still never created through the generic endpoint.
    let created = library.create(&manager, input(json!({"name": "new.png"}))).await;
    assert!(matches!(created, Err(AppError::Forbidden)));
});

db_test!(a_renamed_file_downloads_under_its_new_name, |db: TestDb| async move {
    let (app, users) = admin_app(db).await;
    let admin = app.acting_as(&users.admin).await;
    let item = upload(&app, &admin, &[("file", "a.png", &png())], None).await.json()["data"].clone();
    assert!(item.get("alt").is_none(), "no alt yet");
    let id = item["id"].as_str().unwrap();
    let record = app.admin_api(&format!("resources/media/records/{id}"));
    admin
        .send(Method::PATCH, &record, Some(&json!({"name": "renamed.png", "alt": "Hi"})))
        .await
        .assert_status(StatusCode::OK);
    let served = admin.get(&app.admin_api(&format!("media/{id}/file"))).await;
    assert!(served.headers["content-disposition"].to_str().unwrap().starts_with("inline; filename=\"renamed.png\""));
    let record = admin.get(&record).await.json();
    assert_eq!(
        (&record["data"]["data"]["name"], &record["data"]["data"]["alt"]),
        (&json!("renamed.png"), &json!("Hi"))
    );
    let row = media::Entity::find().one(app.state().db()).await.unwrap().unwrap();
    assert_eq!(rustcms_admin::media::MediaItem::from(&row).alt.as_deref(), Some("Hi"));
});
