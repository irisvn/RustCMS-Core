//! Labels in the user's language: schemas, navigation and permission groups.
//! Data, identifiers and validation messages (translated by the panel from
//! their code) are never changed.

mod support;

use axum::http::StatusCode;
use rustcms_testing::{TestDb, db_test};
use serde_json::{Value, json};
use support::app::setup;

/// The input named `name` anywhere in a form.
fn input<'a>(components: &'a Value, name: &str) -> Option<&'a Value> {
    components.as_array()?.iter().find_map(|c| if c["name"] == name { Some(c) } else { input(&c["components"], name) })
}

db_test!(labels_follow_the_language, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    let posts = app.admin_api("resources/posts/schema?operation=view");
    let vi = app.acting_as(&users.admin).await.languages("vi-VN,vi;q=0.9");
    let en = app.acting_as(&users.admin).await.languages("en");

    let schema = vi.get(&posts).await;
    schema.assert_status(StatusCode::OK);
    let schema = schema.json();
    assert_eq!(
        (schema["resource"]["label"].clone(), schema["resource"]["pluralLabel"].clone()),
        (json!("Bài viết"), json!("Bài viết"))
    );
    // Scoped first ([posts] "Content"), then global ("Title"), then the framework ("Published").
    let content = &schema["form"]["components"][0];
    assert_eq!(content["label"], json!("Nội dung bài"));
    assert_eq!(input(&schema["form"]["components"], "title").unwrap()["label"], json!("Tiêu đề"));
    assert_eq!(input(&schema["form"]["components"], "title").unwrap()["placeholder"], json!("Tiêu đề"));
    assert_eq!(input(&schema["form"]["components"], "published").unwrap()["label"], json!("Đã xuất bản"));
    assert_eq!(schema["table"]["actions"][0]["label"], json!("Xuất bản"));
    assert_eq!(schema["table"]["columns"][0]["label"], json!("Tiêu đề"));
    // Identifiers stay.
    assert_eq!(schema["resource"]["slug"], json!("posts"));
    assert_eq!(input(&schema["form"]["components"], "title").unwrap()["name"], json!("title"));
    // Untranslated text stays as written; English is unchanged.
    assert_eq!(input(&schema["form"]["components"], "price").unwrap()["label"], json!("Price"));
    let english = en.get(&posts).await.json();
    assert_eq!(english["resource"]["label"], json!("Post"));
    assert_eq!(english["form"]["components"][0]["label"], json!("Content"));

    // Navigation: labels and groups; the media library uses the framework's catalog.
    let panel = vi.get(&app.admin_api("panel")).await.json();
    let nav: Vec<(String, String)> = panel["navigation"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| (r["slug"].as_str().unwrap().to_owned(), r["pluralLabel"].as_str().unwrap().to_owned()))
        .collect();
    assert!(nav.contains(&("posts".into(), "Bài viết".into())), "{nav:?}");
    assert!(nav.contains(&("media".into(), "Thư viện media".into())), "{nav:?}");

    // Data is never translated.
    let created = vi.post_json(&app.admin_api("resources/posts/records"), &json!({"title": "Title"})).await;
    created.assert_status(StatusCode::CREATED).assert_json("/data/data/title", json!("Title"));
});

db_test!(the_saved_choice_wins_over_the_browser, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    let client = app.acting_as(&users.admin).await.languages("en");
    client.post_json(&app.admin_api("auth/locale"), &json!({"locale": "vi"})).await.assert_status(StatusCode::OK);
    let schema = client.get(&app.admin_api("resources/posts/schema?operation=view")).await.json();
    assert_eq!(schema["resource"]["label"], json!("Bài viết"));
});

db_test!(permission_groups_have_translated_labels, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    let vi = app.acting_as(&users.admin).await.languages("vi");
    let permissions = vi.get(&app.admin_api("system/permissions")).await.json();
    let posts = permissions.as_array().unwrap().iter().find(|p| p["name"] == "posts.view_any").unwrap();
    assert_eq!((posts["group"].clone(), posts["groupLabel"].clone()), (json!("posts"), json!("Bài viết")));
});

db_test!(validation_messages_stay_english_with_codes, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    let vi = app.acting_as(&users.admin).await.languages("vi");
    let refused = vi.post_json(&app.admin_api("resources/posts/records"), &json!({"title": "x".repeat(121)})).await;
    refused.assert_validation_error("title");
    let body = refused.json();
    assert_eq!(body["errors"]["title"], json!(["may not be longer than 120 characters"]));
    assert_eq!(body["issues"]["title"], json!([{"code": "max_length", "params": {"max": "120"}}]));
});

#[test]
#[should_panic(expected = "must use the placeholders")]
fn catalogs_keep_placeholders() {
    let _ = rustcms_admin::AdminProvider::new().translations("vi", "\"at most {max}\" = \"tối đa {n}\"");
}

#[test]
#[should_panic(expected = "registered twice")]
fn a_language_is_registered_once() {
    let _ = rustcms_admin::AdminProvider::new().translations("vi", "").translations("vi", "");
}

#[test]
fn missing_labels_are_looked_up_as_they_are_shown() {
    use rustcms_admin::AdminPanel;
    use rustcms_admin::registry::Registry;
    use rustcms_core::i18n::Translator;
    // The media library sits in the "Content" group, which navigation translates
    // globally: a translation scoped to the resource does not count.
    let registry = Registry::new().register::<rustcms_admin::media::MediaResource>().finish();
    let mut translator = Translator::default();
    translator.add("vi", "[media]\n\"Content\" = \"Nội dung\"\n\"Media\" = \"Thư viện\"").unwrap();
    let panel = AdminPanel {
        brand: "x".into(),
        registry,
        translator: std::sync::Arc::new(translator),
        settings: Default::default(),
        custom: std::sync::Arc::new(rustcms_admin::custom::CustomPanel::empty("/admin")),
    };
    let missing = panel.missing_translations("vi", &["users".into(), "media".into()]);
    assert!(missing.contains(&"Content".to_owned()), "{missing:?}");
    assert!(!missing.contains(&"Media".to_owned()), "{missing:?}");
    // Permission groups that are not resources are shown in words.
    assert!(missing.contains(&"Users".to_owned()), "{missing:?}");
    assert!(panel.missing_translations("en", &["users".into()]).is_empty());
}

db_test!(system_permission_groups_are_translated, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    let vi = app.acting_as(&users.admin).await.languages("vi");
    let permissions = vi.get(&app.admin_api("system/permissions")).await.json();
    let label =
        |name: &str| permissions.as_array().unwrap().iter().find(|p| p["name"] == name).unwrap()["groupLabel"].clone();
    assert_eq!(label("users.view_any"), json!("Người dùng"));
    assert_eq!(label("roles.view_any"), json!("Vai trò"));
    let en = app.acting_as(&users.admin).await.get(&app.admin_api("system/permissions")).await.json();
    assert_eq!(
        en.as_array().unwrap().iter().find(|p| p["name"] == "users.view_any").unwrap()["groupLabel"],
        json!("Users")
    );
});

db_test!(labels_follow_the_language_the_panel_shows, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    // The tab shows English while the account's saved choice is Vietnamese
    // (another tab just switched): the answer matches what this tab caches.
    let client = app.acting_as(&users.admin).await;
    client.post_json(&app.admin_api("auth/locale"), &json!({"locale": "vi"})).await.assert_status(StatusCode::OK);
    let posts = app.admin_api("resources/posts/schema?operation=view");
    let shown = async |locale: &str| {
        let request = axum::http::Request::get(&posts)
            .header("x-rustcms-locale", locale)
            .body(axum::body::Body::empty())
            .unwrap();
        client.request(request).await.json()["resource"]["label"].clone()
    };
    assert_eq!(shown("en").await, json!("Post"));
    assert_eq!(shown("vi").await, json!("Bài viết"));
    // A language not offered is ignored: the saved choice applies.
    assert_eq!(shown("fr").await, json!("Bài viết"));
});

db_test!(an_empty_filter_operator_keeps_its_code, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    let client = app.acting_as(&users.admin).await;
    let refused = client.get(&app.admin_api("resources/posts/records?filter%5Btitle%5D%5B%5D=x")).await;
    refused.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
    let issues = refused.json()["issues"].clone();
    assert_eq!(issues["title"][0]["code"], json!("unknown_operator"), "{issues}");
});
