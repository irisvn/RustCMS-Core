//! Row actions: offered per record (`visible_when`, decided by the server and
//! checked again when the action runs) and placed (`primary`, `destructive`).

mod support;

use rustcms_admin::{ListQuery, SchemaOperation};
use rustcms_testing::{TestDb, db_test};
use serde_json::json;
use support::{Fixture, admin};

db_test!(hidden_actions_are_flagged_per_row_and_refused_when_run, |db: TestDb| async move {
    let fx = Fixture::new(db).await;
    let posts = fx.resource("posts");
    let ctx = fx.ctx(admin());
    let create = async |title: &str, published: bool| {
        let input = json!({"title": title, "published": published});
        posts.create(&ctx, input.as_object().unwrap().clone()).await.unwrap().key
    };
    let draft = create("draft", false).await;
    let live = create("live", true).await;

    // The schema places them: `touch` labelled, `unpublish` destructive.
    let schema = serde_json::to_value(posts.schema(&ctx, SchemaOperation::View, None).await.unwrap()).unwrap();
    let action =
        |name: &str| schema["table"]["actions"].as_array().unwrap().iter().find(|a| a["name"] == name).cloned();
    assert_eq!(action("touch").unwrap()["primary"], json!(true));
    assert_eq!(
        (action("unpublish").unwrap()["color"].clone(), action("unpublish").unwrap().get("primary")),
        (json!("danger"), None)
    );

    // Each row says whether it is offered there.
    let page = posts.list(&ctx, &ListQuery::default()).await.unwrap();
    let row = |key: &str| page.data.iter().find(|e| e.key == key).unwrap().abilities.clone();
    assert_eq!((row(&draft)["unpublish"], row(&live)["unpublish"]), (false, true));
    assert!(row(&draft)["touch"] && row(&live)["touch"]);
    assert!(!posts.find(&ctx, &draft).await.unwrap().abilities["unpublish"]);

    // A bulk run on both: the hidden one is skipped (denied), never handed to the handler.
    let outcome =
        posts.run_action(&ctx, "unpublish", &[draft.clone(), live.clone()], Default::default()).await.unwrap();
    assert_eq!((outcome.processed, outcome.denied), (vec![live.clone()], vec![draft.clone()]));
    let live_now = posts.find(&ctx, &live).await.unwrap();
    assert_eq!(live_now.data["published"], json!(false));
    // Now hidden on that record too: a run on it alone is refused, nothing runs.
    assert!(!live_now.abilities["unpublish"]);
    let refused = posts.run_action(&ctx, "unpublish", std::slice::from_ref(&live), Default::default()).await;
    assert!(matches!(refused, Err(rustcms_core::AppError::Forbidden)), "{refused:?}");
    // The same key twice is still a run on one record.
    let refused = posts.run_action(&ctx, "unpublish", &[draft.clone(), draft.clone()], Default::default()).await;
    assert!(matches!(refused, Err(rustcms_core::AppError::Forbidden)), "{refused:?}");
});

db_test!(hidden_actions_are_refused_over_http, |db: TestDb| async move {
    let (app, users) = support::app::setup(db).await;
    let admin = app.acting_as(&users.admin).await;
    let created = admin
        .post_json(&app.admin_api("resources/posts/records"), &json!({"title": "draft", "published": false}))
        .await
        .json();
    let key = created["data"]["key"].as_str().unwrap().to_owned();
    let list = admin.get(&app.admin_api("resources/posts/records")).await.json();
    assert_eq!(list["data"][0]["abilities"]["unpublish"], json!(false));
    admin
        .post_json(&app.admin_api("resources/posts/actions/unpublish"), &json!({"keys": [key]}))
        .await
        .assert_status(axum::http::StatusCode::FORBIDDEN);
});

db_test!(handlers_return_a_message_and_a_url_to_open, |db: TestDb| async move {
    let fx = Fixture::new(db).await;
    let notes = fx.resource("notes");
    let ctx = fx.ctx(admin());
    let note = notes.create(&ctx, json!({"title": "n"}).as_object().unwrap().clone()).await.unwrap().key;
    let open = async |url: &str| {
        let data = json!({"url": url}).as_object().unwrap().clone();
        notes.run_action(&ctx, "open", std::slice::from_ref(&note), data).await
    };
    let outcome = open("https://example.com/report?id=1#top").await.unwrap();
    assert_eq!(
        (outcome.open.as_deref(), outcome.message.as_deref(), outcome.processed.clone()),
        (Some("https://example.com/report?id=1#top"), Some("Opening the page"), vec![note.clone()])
    );
    let wire = serde_json::to_value(&outcome).unwrap();
    support::contract("action-outcome.json", &wire);
    assert_eq!(
        (&wire["open"], &wire["message"]),
        (&json!("https://example.com/report?id=1#top"), &json!("Opening the page"))
    );
    for ok in ["https://example.com:8443/x", "https://[::1]:8443/x", "https://127.0.0.1/x", "https://EXAMPLE.com"] {
        assert_eq!(open(ok).await.unwrap().open.as_deref(), Some(ok));
    }
    // A path of this site: the panel's or the application's.
    assert_eq!(open("/admin/r/notes?page=2").await.unwrap().open.as_deref(), Some("/admin/r/notes?page=2"));
    assert_eq!(open("/blog/n").await.unwrap().open.as_deref(), Some("/blog/n"));
    // Anything else fails the action, and its writes are rolled back.
    notes.update(&ctx, &note, json!({"title": "kept"}).as_object().unwrap().clone()).await.unwrap();
    let long = format!("https://example.com/{}", "x".repeat(2048));
    for url in [
        "http://example.com/",
        "javascript:alert(1)",
        "data:text/html,x",
        "//evil.example/x",
        "https://user:pw@evil.example/",
        "https://",
        "https://evil.example\\@good/",
        "/admin/../evil",
        "/admin/%2E%2E/evil",
        "relative/path",
        long.as_str(),
        "https://exa mple.com/",
        "https://example.com:abc/report",
        "https://example.com:99999/",
        "https://example.com:0/",
        "https://[]/",
        "https://[/",
        "https://:",
        "https://a..b/",
        "https://-a.example/",
        "",
    ] {
        let refused = open(url).await.unwrap_err();
        assert!(matches!(refused, rustcms_core::AppError::Internal(_)), "{url}: {refused:?}");
    }
    assert_eq!(notes.find(&ctx, &note).await.unwrap().data["title"], json!("kept"), "rolled back");
});

db_test!(a_page_to_open_is_refused_on_runs_on_several_records, |db: TestDb| async move {
    let fx = Fixture::new(db).await;
    let notes = fx.resource("notes");
    let ctx = fx.ctx(admin());
    let mut keys = Vec::new();
    for title in ["a", "b"] {
        keys.push(notes.create(&ctx, json!({"title": title}).as_object().unwrap().clone()).await.unwrap().key);
    }
    let data = json!({"url": "https://example.com/x"}).as_object().unwrap().clone();
    let refused = notes.run_action(&ctx, "open", &keys, data).await.unwrap_err();
    assert!(
        matches!(&refused, rustcms_core::AppError::Validation(v) if v.form_errors == ["Run this action on one record at a time."]),
        "{refused:?}"
    );
    for (key, title) in keys.iter().zip(["a", "b"]) {
        assert_eq!(notes.find(&ctx, key).await.unwrap().data["title"], json!(title), "rolled back");
    }
});

db_test!(handlers_returning_nothing_report_no_effects, |db: TestDb| async move {
    let fx = Fixture::new(db).await;
    let notes = fx.resource("notes");
    let ctx = fx.ctx(admin());
    let note = notes.create(&ctx, json!({"title": "n"}).as_object().unwrap().clone()).await.unwrap().key;
    let outcome = notes.run_action(&ctx, "touch", &[note], Default::default()).await.unwrap();
    let wire = serde_json::to_value(&outcome).unwrap();
    assert!(wire.get("open").is_none() && wire.get("message").is_none(), "{wire}");
});

db_test!(outcomes_are_replayed_with_their_url, |db: TestDb| async move {
    let (app, users) = support::app::setup(db).await;
    let admin = app.acting_as(&users.admin).await;
    let created = admin.post_json(&app.admin_api("resources/notes/records"), &json!({"title": "n"})).await.json();
    let key = created["data"]["key"].as_str().unwrap().to_owned();
    let request = || {
        axum::http::Request::builder()
            .method(axum::http::Method::POST)
            .uri(app.admin_api("resources/notes/actions/open"))
            .header("content-type", "application/json")
            .header("idempotency-key", "open-1")
            .body(axum::body::Body::from(json!({"keys": [key], "data": {"url": "https://example.com/x"}}).to_string()))
            .unwrap()
    };
    admin.fetch_csrf().await;
    let first = admin.request(request()).await.json();
    assert_eq!((&first["open"], &first["message"]), (&json!("https://example.com/x"), &json!("Opening the page")));
    let again = admin.request(request()).await.json();
    assert_eq!((&again["open"], &again["replayed"]), (&json!("https://example.com/x"), &json!(true)));
    // Once the record is out of the user's sight, a replay keeps its message and page to itself.
    admin.send(axum::http::Method::DELETE, &app.admin_api(&format!("resources/notes/records/{key}")), None).await;
    let hidden = admin.request(request()).await.json();
    assert_eq!(hidden["replayed"], json!(true));
    assert!(hidden.get("open").is_none() && hidden.get("message").is_none(), "{hidden}");
    admin.send(axum::http::Method::POST, &app.admin_api(&format!("resources/notes/records/{key}/restore")), None).await;
    // Refused URLs: a server error, nothing written.
    let refused = admin
        .post_json(
            &app.admin_api("resources/notes/actions/open"),
            &json!({"keys": [key], "data": {"url": "http://example.com/"}}),
        )
        .await;
    refused.assert_status(axum::http::StatusCode::INTERNAL_SERVER_ERROR);
});
