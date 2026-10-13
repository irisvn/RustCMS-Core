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
