//! The dashboard: widgets shown to users with their permission who may also
//! list the resource and read its fields; counts as the list counts (scope,
//! policy per record, filters, trash); charts by UTC buckets; recent records
//! as the list shows them; checked at boot. SQLite, PostgreSQL, MySQL.

mod support;

use axum::http::StatusCode;
use chrono::{Duration, Utc};
use rustcms_auth::{ADMIN_GUARD, Actor, AuthState, RbacService};
use rustcms_core::models::user;
use rustcms_testing::{Factory, TestApp, TestDb, UserFactory, db_test};
use sea_orm::{ActiveModelTrait, ActiveValue::Set};
use serde_json::{Value, json};

use support::app::setup;
use support::{note, post};

async fn with_permissions(app: &TestApp, name: &str, permissions: &[&str]) -> (user::Model, i64) {
    let auth = app.state().service::<AuthState>().unwrap();
    let db = app.state().db();
    let rbac = RbacService::new(db, &auth.resolver);
    let user = UserFactory::new().name(name).create(db).await;
    let permissions: Vec<String> = permissions.iter().map(|p| (*p).to_owned()).collect();
    let role = rbac.create_role(Actor::System, ADMIN_GUARD, &name.to_lowercase(), &permissions).await.unwrap();
    rbac.assign_role(Actor::System, user.id, role.id).await.unwrap();
    (user, role.id)
}

async fn add_post(app: &TestApp, author: i64, title: &str, published_at: Option<chrono::DateTime<Utc>>) {
    post::ActiveModel {
        id: Set(uuid::Uuid::now_v7()),
        title: Set(title.into()),
        category_id: Set(None),
        author_id: Set(author),
        price: Set(None),
        published: Set(published_at.is_some()),
        published_at: Set(published_at),
        secret: Set(String::new()),
    }
    .insert(app.state().db())
    .await
    .unwrap();
}

fn names(view: &Value) -> Vec<String> {
    view["widgets"].as_array().unwrap().iter().map(|w| w["name"].as_str().unwrap().to_owned()).collect()
}

const DASHBOARD: [&str; 6] = [
    "dashboard.posts_total",
    "dashboard.published_posts",
    "dashboard.posts_per_day",
    "dashboard.latest_posts",
    "posts.view_any",
    "posts.view",
];

db_test!(widgets_count_what_the_list_shows, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    let (writer, _) = with_permissions(&app, "Writer", &DASHBOARD).await;
    let now = Utc::now();
    add_post(&app, writer.id, "Mine today", Some(now)).await;
    add_post(&app, writer.id, "Mine yesterday", Some(now - Duration::days(1))).await;
    add_post(&app, writer.id, "Mine draft", None).await;
    add_post(&app, users.admin.id, "Theirs", Some(now)).await;
    let client = app.acting_as(&writer).await;
    let widget = async |name: &str| client.get(&app.admin_api(&format!("dashboard/widgets/{name}"))).await;

    // Their own posts only (the scope), each checked by the policy.
    assert_eq!(widget("posts_total").await.json(), json!({"kind": "stat", "value": "3"}));
    let admin = app.acting_as(&users.admin).await;
    support::contract("dashboard.json", &admin.get(&app.admin_api("dashboard")).await.json());
    let all = admin.get(&app.admin_api("dashboard/widgets/posts_total")).await.json();
    assert_eq!(all["value"], json!("4"));
    // A fixed filter (published), compared with the 30 days before.
    assert_eq!(widget("published_posts").await.json(), json!({"kind": "stat", "value": "2", "previous": "0"}));
    // Per UTC day, oldest first.
    let chart = widget("posts_per_day").await.json();
    let counts: Vec<&str> = chart["buckets"].as_array().unwrap().iter().map(|b| b["count"].as_str().unwrap()).collect();
    assert_eq!(counts.len(), 7);
    assert_eq!(&counts[5..], ["1", "1"], "{chart}");
    assert_eq!(chart["buckets"][6]["start"], json!(now.format("%Y-%m-%d").to_string()));
    // The latest, as the list shows them.
    let recent = widget("latest_posts").await.json();
    let titles: Vec<&str> =
        recent["records"].as_array().unwrap().iter().map(|r| r["title"].as_str().unwrap()).collect();
    assert_eq!(titles, ["Mine draft", "Mine today", "Mine yesterday"]);
    assert_eq!(recent["columns"], json!([{"name": "published_at", "label": "Published at"}]));
    assert_eq!(recent["records"][0]["values"]["published_at"], json!("—"));
});

db_test!(widgets_need_their_permission_and_the_list, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    let member = app.acting_as(&users.member).await;
    assert_eq!(names(&member.get(&app.admin_api("dashboard")).await.json()), Vec::<String>::new());
    member.get(&app.admin_api("dashboard/widgets/posts_total")).await.assert_status(StatusCode::FORBIDDEN);
    member.get(&app.admin_api("dashboard/widgets/nope")).await.assert_status(StatusCode::NOT_FOUND);
    // The widget's permission without listing posts: still hidden.
    let (watcher, role) = with_permissions(&app, "Watcher", &["dashboard.posts_total"]).await;
    let client = app.acting_as(&watcher).await;
    assert!(names(&client.get(&app.admin_api("dashboard")).await.json()).is_empty());
    client.get(&app.admin_api("dashboard/widgets/posts_total")).await.assert_status(StatusCode::FORBIDDEN);
    let auth = app.state().service::<AuthState>().unwrap();
    let rbac = RbacService::new(app.state().db(), &auth.resolver);
    let granted: Vec<String> =
        ["dashboard.posts_total", "posts.view_any", "posts.view"].iter().map(|p| (*p).to_owned()).collect();
    rbac.set_role_permissions(Actor::System, role, &granted).await.unwrap();
    assert_eq!(names(&client.get(&app.admin_api("dashboard")).await.json()), ["posts_total"]);
    client.get(&app.admin_api("dashboard/widgets/posts_total")).await.assert_status(StatusCode::OK);
    // Revoked: at once (nothing cached).
    rbac.set_role_permissions(Actor::System, role, &["posts.view_any".to_owned()]).await.unwrap();
    client.get(&app.admin_api("dashboard/widgets/posts_total")).await.assert_status(StatusCode::FORBIDDEN);
});

db_test!(trashed_records_are_not_counted, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    let admin = app.acting_as(&users.admin).await;
    for title in ["Kept", "Trashed"] {
        admin
            .post_json(&app.admin_api("resources/notes/records"), &json!({"title": title}))
            .await
            .assert_status(StatusCode::CREATED);
    }
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
    note::Entity::update_many()
        .col_expr(note::Column::DeletedAt, sea_orm::sea_query::Expr::value(Utc::now()))
        .filter(note::Column::Title.eq("Trashed"))
        .exec(app.state().db())
        .await
        .unwrap();
    let notes = admin.get(&app.admin_api("dashboard/widgets/notes_total")).await.json();
    assert_eq!(notes, json!({"kind": "stat", "value": "1"}));
});

#[test]
fn bad_widgets_stop_the_boot() {
    use rustcms_admin::dashboard::{Dashboard, Widget};
    use rustcms_admin::resource::FilterOp;
    let cases = [
        Dashboard::new().widget(Widget::stat("x", "posts").filter("title", FilterOp::Eq, json!("a"))),
        Dashboard::new().widget(Widget::stat("x", "posts")).widget(Widget::stat("x", "posts")),
        Dashboard::new().widget(Widget::chart("x", "posts", "title")),
        Dashboard::new().widget(Widget::stat("x", "nope")),
        Dashboard::new().widget(Widget::recent("x", "posts").columns(&["nope"])),
        Dashboard::new().widget(Widget::chart("x", "posts", "published_at").last(61)),
    ];
    for (i, dashboard) in cases.into_iter().enumerate() {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            support::app::check_dashboard(dashboard);
        }));
        assert!(result.is_err(), "case {i} should be refused");
    }
}
