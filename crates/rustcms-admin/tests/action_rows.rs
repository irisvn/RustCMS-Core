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
