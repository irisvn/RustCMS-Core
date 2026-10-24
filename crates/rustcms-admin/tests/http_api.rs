//! The admin API over HTTP, behind the real admin guard.

mod support;

use axum::http::{Method, StatusCode};
use rustcms_auth::{ADMIN_GUARD, AuthState};
use rustcms_testing::{TestApp, TestDb, db_test};
use serde_json::{Value, json};
use support::app::{Users, setup};

fn api(app: &TestApp, path: &str) -> String {
    app.admin_api(path)
}

db_test!(panel_lists_what_the_user_may_see, |db: TestDb| async move {
    let (app, users): (TestApp, Users) = setup(db).await;
    app.client().get(&api(&app, "panel")).await.assert_status(StatusCode::UNAUTHORIZED);

    let nav =
        |body: Value| body["navigation"].as_array().unwrap().iter().map(|r| r["slug"].clone()).collect::<Vec<_>>();
    let admin = app.acting_as(&users.admin).await.get(&api(&app, "panel")).await;
    admin.assert_status(StatusCode::OK).assert_json("/brand", json!("Test CMS"));
    // In registration order.
    assert_eq!(nav(admin.json()), [json!("media"), json!("categories"), json!("posts"), json!("tags"), json!("notes")]);
    assert_eq!(
        nav(app.acting_as(&users.editor).await.get(&api(&app, "panel")).await.json()),
        [json!("categories"), json!("posts"), json!("notes")]
    );
    assert!(nav(app.acting_as(&users.member).await.get(&api(&app, "panel")).await.json()).is_empty());
    // Upload limits for those who may upload (`[storage]`, 64 KiB in tests; 4 at a time by default).
    assert_eq!(admin.json()["uploads"], json!({"maxBytes": 64 * 1024, "maxConcurrent": 4}));
    let editor = app.acting_as(&users.editor).await.get(&api(&app, "panel")).await.json();
    assert!(editor.get("uploads").is_none(), "{editor}");

    // An API token never reaches the admin panel.
    app.with_token(&users.admin, &["*"]).await.get(&api(&app, "panel")).await.assert_status(StatusCode::UNAUTHORIZED);
});

db_test!(crud_over_http_with_query_string_filters, |db: TestDb| async move {
    let (app, users): (TestApp, Users) = setup(db).await;
    let admin = app.acting_as(&users.admin).await;
    let category =
        admin.post_json(&api(&app, "resources/categories/records"), &json!({"name": "News", "slug": "news"})).await;
    category.assert_status(StatusCode::CREATED);
    let category_id = category.json()["data"]["data"]["id"].clone();
    let category_key = category.json()["data"]["key"].as_str().unwrap().to_owned();
    assert_eq!(category_id, Value::Null, "the key is not part of the readable data");

    let editor = app.acting_as(&users.editor).await;
    let posts = api(&app, "resources/posts/records");
    for (title, category) in [
        ("Hello world", json!(category_key.parse::<i32>().unwrap())),
        ("100% Rust", Value::Null),
        ("Other", Value::Null),
    ] {
        editor
            .post_json(&posts, &json!({"title": title, "category_id": category}))
            .await
            .assert_status(StatusCode::CREATED);
    }

    let list = async |query: &str| editor.get(&format!("{posts}?{query}")).await;
    let titles = |r: rustcms_testing::TestResponse| {
        r.json()["data"].as_array().unwrap().iter().map(|e| e["data"]["title"].clone()).collect::<Vec<_>>()
    };
    assert_eq!(titles(list("sort=-title").await), [json!("Other"), json!("Hello world"), json!("100% Rust")]);
    assert_eq!(titles(list("search=HELLO").await), [json!("Hello world")], "case-insensitive");
    assert_eq!(titles(list("search=%25").await), [json!("100% Rust")], "% is literal");
    assert_eq!(titles(list(&format!("filter[category_id][in]={category_key},999")).await), [json!("Hello world")]);
    assert_eq!(list("filter[published][eq]=false").await.json()["meta"]["total"], json!(3));
    list("filter[published][near]=x").await.assert_validation_error("published");
    list("filter[title][eq]=x").await.assert_validation_error("title");
    list("per_page=abc").await.assert_validation_error("per_page");
    let page = list("per_page=2&page=2").await;
    page.assert_json("/meta/page", json!(2)).assert_json("/meta/lastPage", json!(2));

    // Read, update, validation, delete.
    let created = editor.post_json(&posts, &json!({"title": "Draft"})).await.json();
    let record = format!("{posts}/{}", created["data"]["key"].as_str().unwrap());
    editor.get(&record).await.assert_status(StatusCode::OK).assert_json("/data/data/title", json!("Draft"));
    editor
        .send(Method::PATCH, &record, Some(&json!({"title": "Final"})))
        .await
        .assert_json("/data/data/title", json!("Final"));
    editor
        .send(Method::PATCH, &record, Some(&json!({"title": "x".repeat(121)})))
        .await
        .assert_validation_error("title");
    editor.send(Method::PATCH, &record, Some(&json!({"published": true}))).await.assert_validation_error("published");
    editor.send(Method::DELETE, &record, None).await.assert_status(StatusCode::NO_CONTENT);
    editor.get(&record).await.assert_status(StatusCode::NOT_FOUND);
    editor.get(&api(&app, "resources/nope/records")).await.assert_status(StatusCode::NOT_FOUND);

    // CSRF still applies.
    editor.set_csrf(None);
    editor.post_json(&posts, &json!({"title": "x"})).await.assert_status(StatusCode::FORBIDDEN);
});

db_test!(schema_follows_field_permissions, |db: TestDb| async move {
    let (app, users): (TestApp, Users) = setup(db).await;
    let editor = app.acting_as(&users.editor).await;
    let schema = async |client: &rustcms_testing::TestClient<'_>, query: &str| {
        client.get(&format!("{}?{query}", api(&app, "resources/posts/schema"))).await
    };
    let inputs = |body: &Value| -> Vec<(String, bool)> {
        fn walk(components: &Value, out: &mut Vec<(String, bool)>) {
            for c in components.as_array().unwrap() {
                match c.get("components") {
                    Some(children) => walk(children, out),
                    None => out.push((c["name"].as_str().unwrap().to_owned(), c["disabled"].as_bool().unwrap())),
                }
            }
        }
        let mut out = Vec::new();
        walk(&body["form"]["components"], &mut out);
        out
    };

    // Create: only inputs the editor may set (no `published`, which needs posts.publish).
    let create = schema(&editor, "operation=create").await.json();
    assert_eq!(
        inputs(&create),
        [
            ("title".into(), false),
            ("price".into(), false),
            ("category_id".into(), false),
            ("published_at".into(), false)
        ]
    );
    let actions: Vec<_> = create["table"]["actions"].as_array().unwrap().iter().map(|a| a["name"].clone()).collect();
    assert_eq!(
        actions,
        [json!("touch"), json!("unpublish")],
        "publish needs posts.publish; touch and unpublish are decided per record"
    );

    // Edit: readable fields, `published` shown but disabled.
    let post = editor.post_json(&api(&app, "resources/posts/records"), &json!({"title": "A"})).await.json();
    let key = post["data"]["key"].as_str().unwrap();
    let edit = schema(&editor, &format!("operation=edit&key={key}")).await.json();
    assert!(inputs(&edit).contains(&("published".into(), true)));
    assert_eq!(edit["abilities"]["update"], json!(true));

    // Others' posts are out of scope; members may not see the schema at all.
    let admin = app.acting_as(&users.admin).await;
    let theirs = admin.post_json(&api(&app, "resources/posts/records"), &json!({"title": "B"})).await.json();
    schema(&editor, &format!("operation=edit&key={}", theirs["data"]["key"].as_str().unwrap()))
        .await
        .assert_status(StatusCode::NOT_FOUND);
    schema(&app.acting_as(&users.member).await, "operation=create").await.assert_status(StatusCode::FORBIDDEN);

    // Contract fixture shared with the admin SPA (fixtures/contract/posts.schema.json).
    let full = schema(&admin, "operation=create").await.json();
    support::contract("posts.schema.json", &full);
});

db_test!(options_respect_the_target_scope, |db: TestDb| async move {
    let (app, users): (TestApp, Users) = setup(db).await;
    let admin = app.acting_as(&users.admin).await;
    for (name, slug) in [("News", "news"), ("Hidden budget", "hidden"), ("Newsletter", "newsletter")] {
        admin.post_json(&api(&app, "resources/categories/records"), &json!({"name": name, "slug": slug})).await;
    }
    let options = async |client: &rustcms_testing::TestClient<'_>, query: &str| {
        client.get(&format!("{}?{query}", api(&app, "resources/posts/fields/category_id/options"))).await
    };
    let labels = |body: Value| body["data"].as_array().unwrap().iter().map(|o| o["label"].clone()).collect::<Vec<_>>();

    let editor = app.acting_as(&users.editor).await;
    assert_eq!(labels(options(&editor, "").await.json()), [json!("News"), json!("Newsletter")], "hidden excluded");
    assert_eq!(labels(options(&editor, "search=letter").await.json()), [json!("Newsletter")]);
    let all = options(&admin, "").await.json();
    assert_eq!(labels(all.clone()).len(), 3);
    assert!(all["data"][0]["value"].is_number(), "i32 keys keep their wire type");

    options(&app.acting_as(&users.member).await, "").await.assert_status(StatusCode::FORBIDDEN);
    editor.get(&api(&app, "resources/posts/fields/title/options")).await.assert_status(StatusCode::NOT_FOUND);
});

db_test!(actions_over_http, |db: TestDb| async move {
    let (app, users): (TestApp, Users) = setup(db).await;
    let admin = app.acting_as(&users.admin).await;
    let created = admin.post_json(&api(&app, "resources/posts/records"), &json!({"title": "Go"})).await.json();
    let key = created["data"]["key"].as_str().unwrap().to_owned();
    assert_eq!(created["data"]["abilities"]["publish"], json!(true));

    let run = admin.post_json(&api(&app, "resources/posts/actions/publish"), &json!({"keys": [key, "missing"]})).await;
    run.assert_status(StatusCode::OK)
        .assert_json("/processed", json!([key]))
        .assert_json("/missing", json!(["missing"]));
    admin
        .post_json(&api(&app, "resources/posts/actions/nope"), &json!({"keys": []}))
        .await
        .assert_status(StatusCode::NOT_FOUND);

    let editor = app.acting_as(&users.editor).await;
    let mine = editor.post_json(&api(&app, "resources/posts/records"), &json!({"title": "Mine"})).await.json();
    let refused =
        editor.post_json(&api(&app, "resources/posts/actions/publish"), &json!({"keys": [mine["data"]["key"]]})).await;
    refused.assert_status(StatusCode::FORBIDDEN);
});

db_test!(resource_permissions_are_declared_for_sync, |db: TestDb| async move {
    let (app, _users): (TestApp, Users) = setup(db).await;
    let declared = app.state().service::<AuthState>().unwrap().permissions.all();
    for name in ["posts.view_any", "posts.publish", "posts.archive", "categories.delete", "tags.update"] {
        assert!(declared.contains_key(&(ADMIN_GUARD.to_owned(), name.to_owned())), "{name}");
    }
});

db_test!(create_permission_is_enough_to_load_choices, |db: TestDb| async move {
    use rustcms_auth::{Actor, RbacService};
    let (app, users): (TestApp, Users) = setup(db).await;
    let auth = app.state().service::<AuthState>().unwrap();
    let rbac = RbacService::new(app.state().db(), &auth.resolver);
    let permissions: Vec<String> =
        ["posts.create", "categories.view_any", "categories.view"].map(String::from).to_vec();
    let writer = rbac.create_role(Actor::System, ADMIN_GUARD, "writer", &permissions).await.unwrap();
    rbac.assign_role(Actor::System, users.member.id, writer.id).await.unwrap();
    app.acting_as(&users.admin)
        .await
        .post_json(&api(&app, "resources/categories/records"), &json!({"name": "News", "slug": "news"}))
        .await;

    let member = app.acting_as(&users.member).await;
    member
        .get(&format!("{}?operation=create", api(&app, "resources/posts/schema")))
        .await
        .assert_status(StatusCode::OK);
    member.get(&api(&app, "resources/posts/records")).await.assert_status(StatusCode::FORBIDDEN);
    let options = member.get(&api(&app, "resources/posts/fields/category_id/options")).await;
    options.assert_status(StatusCode::OK).assert_json("/data/0/label", json!("News"));
});

db_test!(record_policies_decide_options_for_the_edited_record, |db: TestDb| async move {
    use rustcms_auth::{Actor, RbacService};
    use sea_orm::EntityTrait;
    let (app, users): (TestApp, Users) = setup(db).await;
    let auth = app.state().service::<AuthState>().unwrap();
    let rbac = RbacService::new(app.state().db(), &auth.resolver);
    // May update their own posts only; no list, create or general update permission.
    let permissions: Vec<String> =
        ["posts.update_own", "categories.view_any", "categories.view"].map(String::from).to_vec();
    let owner = rbac.create_role(Actor::System, ADMIN_GUARD, "owner", &permissions).await.unwrap();
    rbac.assign_role(Actor::System, users.member.id, owner.id).await.unwrap();

    let admin = app.acting_as(&users.admin).await;
    admin.post_json(&api(&app, "resources/categories/records"), &json!({"name": "News", "slug": "news"})).await;
    // A post owned by the member (inserted as the admin, then reassigned).
    let own = admin.post_json(&api(&app, "resources/posts/records"), &json!({"title": "Own"})).await.json();
    let own = own["data"]["key"].as_str().unwrap().to_owned();
    support::post::Entity::update_many()
        .col_expr(support::post::Column::AuthorId, sea_orm::sea_query::Expr::value(users.member.id))
        .exec(app.state().db())
        .await
        .unwrap();
    let theirs = admin.post_json(&api(&app, "resources/posts/records"), &json!({"title": "Theirs"})).await.json();
    let theirs = theirs["data"]["key"].as_str().unwrap().to_owned();

    let member = app.acting_as(&users.member).await;
    member
        .get(&format!("{}?operation=edit&key={own}", api(&app, "resources/posts/schema")))
        .await
        .assert_status(StatusCode::OK);
    let options = format!("{}?record=", api(&app, "resources/posts/fields/category_id/options"));
    member
        .get(&format!("{options}{own}"))
        .await
        .assert_status(StatusCode::OK)
        .assert_json("/data/0/label", json!("News"));
    member.get(&format!("{options}{theirs}")).await.assert_status(StatusCode::NOT_FOUND);
    member.get(&api(&app, "resources/posts/fields/category_id/options")).await.assert_status(StatusCode::FORBIDDEN);

    // Owner-only action: offered, runs on the owner's post, not on others'.
    let schema = member.get(&format!("{}?operation=edit&key={own}", api(&app, "resources/posts/schema"))).await.json();
    let actions: Vec<_> = schema["table"]["actions"].as_array().unwrap().iter().map(|a| a["name"].clone()).collect();
    assert_eq!(
        actions,
        [json!("touch"), json!("unpublish")],
        "publish needs posts.publish; touch and unpublish are decided per record"
    );
    let record = member.get(&format!("{}/{own}", api(&app, "resources/posts/records"))).await.json();
    assert_eq!(record["data"]["abilities"]["touch"], json!(true));
    let run = member.post_json(&api(&app, "resources/posts/actions/touch"), &json!({"keys": [own, theirs]})).await;
    run.assert_status(StatusCode::OK).assert_json("/processed", json!([own])).assert_json("/missing", json!([theirs]));
    member
        .post_json(&api(&app, "resources/posts/actions/publish"), &json!({"keys": [own]}))
        .await
        .assert_status(StatusCode::FORBIDDEN);
});

db_test!(lists_never_show_records_the_policy_hides, |db: TestDb| async move {
    let (app, users): (TestApp, Users) = setup(db).await;
    let admin = app.acting_as(&users.admin).await;
    let categories = api(&app, "resources/categories/records");
    for (name, slug) in [("Budget", "secret-budget"), ("News", "news"), ("Art", "art")] {
        admin.post_json(&categories, &json!({"name": name, "slug": slug})).await.assert_status(StatusCode::CREATED);
    }
    let names = |r: &rustcms_testing::TestResponse| {
        r.json()["data"].as_array().unwrap().iter().map(|e| e["data"]["name"].clone()).collect::<Vec<_>>()
    };

    let editor = app.acting_as(&users.editor).await;
    let all = editor.get(&format!("{categories}?sort=name")).await;
    assert_eq!(names(&all), [json!("Art"), json!("News")]);
    all.assert_json("/meta/total", json!(2));
    let search = editor.get(&format!("{categories}?search=budget")).await;
    assert!(names(&search).is_empty());
    search.assert_json("/meta/total", json!(0));
    let filtered = editor.get(&format!("{categories}?filter[slug][eq]=secret-budget")).await;
    filtered.assert_json("/meta/total", json!(0));
    // Pagination counts only visible records.
    let page = editor.get(&format!("{categories}?sort=name&per_page=1&page=2")).await;
    assert_eq!(names(&page), [json!("News")]);
    page.assert_json("/meta/lastPage", json!(2));

    let everything = admin.get(&categories).await;
    everything.assert_json("/meta/total", json!(3));
});

db_test!(view_any_without_view_lists_nothing, |db: TestDb| async move {
    use rustcms_auth::{Actor, RbacService};
    let (app, users): (TestApp, Users) = setup(db).await;
    let auth = app.state().service::<AuthState>().unwrap();
    let rbac = RbacService::new(app.state().db(), &auth.resolver);
    let role = rbac.create_role(Actor::System, ADMIN_GUARD, "browser", &["tags.view_any".to_owned()]).await.unwrap();
    rbac.assign_role(Actor::System, users.member.id, role.id).await.unwrap();
    let admin = app.acting_as(&users.admin).await;
    admin
        .post_json(&api(&app, "resources/tags/records"), &json!({"slug": "rust", "label": "Rust"}))
        .await
        .assert_status(StatusCode::CREATED);

    let member = app.acting_as(&users.member).await;
    let list = member.get(&api(&app, "resources/tags/records")).await;
    list.assert_status(StatusCode::OK).assert_json("/meta/total", json!(0)).assert_json("/data", json!([]));
    member.get(&api(&app, "resources/tags/records/rust")).await.assert_status(StatusCode::FORBIDDEN);
    admin.get(&api(&app, "resources/tags/records")).await.assert_json("/meta/total", json!(1));
});

db_test!(writes_return_no_data_without_view, |db: TestDb| async move {
    use rustcms_auth::{Actor, RbacService};
    let (app, users): (TestApp, Users) = setup(db).await;
    let auth = app.state().service::<AuthState>().unwrap();
    let rbac = RbacService::new(app.state().db(), &auth.resolver);
    let role = rbac
        .create_role(Actor::System, ADMIN_GUARD, "blind", &["tags.update".to_owned(), "tags.create".to_owned()])
        .await
        .unwrap();
    rbac.assign_role(Actor::System, users.member.id, role.id).await.unwrap();
    let admin = app.acting_as(&users.admin).await;
    admin.post_json(&api(&app, "resources/tags/records"), &json!({"slug": "rust", "label": "Secret label"})).await;

    let member = app.acting_as(&users.member).await;
    let record = api(&app, "resources/tags/records/rust");
    member.get(&record).await.assert_status(StatusCode::FORBIDDEN);
    for body in [json!({}), json!({"label": "Changed"})] {
        let patched = member.send(Method::PATCH, &record, Some(&body)).await;
        patched
            .assert_status(StatusCode::OK)
            .assert_json("/data/data", json!({}))
            .assert_json("/data/key", json!("rust"));
        assert!(patched.json()["data"].get("title").is_none(), "no title either");
    }
    let created = member.post_json(&api(&app, "resources/tags/records"), &json!({"slug": "go", "label": "Go"})).await;
    created.assert_status(StatusCode::CREATED).assert_json("/data/data", json!({}));
    admin.get(&record).await.assert_json("/data/data/label", json!("Changed"));
});

db_test!(extreme_pages_are_rejected, |db: TestDb| async move {
    let (app, users): (TestApp, Users) = setup(db).await;
    let admin = app.acting_as(&users.admin).await;
    for resource in ["tags", "posts"] {
        let url =
            format!("{}?page=9223372036854775809&per_page=2", api(&app, &format!("resources/{resource}/records")));
        admin.get(&url).await.assert_validation_error("page");
        let url = format!("{}?page=18446744073709551615", api(&app, &format!("resources/{resource}/records")));
        admin.get(&url).await.assert_validation_error("page");
    }
});

/// Tags whose create commits late (2.5 s hook) and whose policy then stalls.
struct LateWrite;

struct StallAfterWrite;

#[async_trait::async_trait]
impl rustcms_admin::Policy for StallAfterWrite {
    async fn allows(&self, _ctx: &rustcms_admin::Ctx, ability: &str, record: Option<&rustcms_admin::Data>) -> bool {
        if ability == "view" && record.is_some() {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        }
        true
    }
}

#[async_trait::async_trait]
impl rustcms_admin::Resource for LateWrite {
    type Entity = support::tag::Entity;
    const SLUG: &'static str = "late";

    fn meta() -> rustcms_admin::ResourceMeta {
        rustcms_admin::ResourceMeta::new("Late").key(rustcms_admin::KeyStrategy::Natural)
    }

    fn fields() -> Vec<rustcms_admin::Field> {
        vec![rustcms_admin::Field::make("slug"), rustcms_admin::Field::make("label")]
    }

    fn policy() -> std::sync::Arc<dyn rustcms_admin::Policy> {
        std::sync::Arc::new(StallAfterWrite)
    }

    async fn mutate_before_create(
        _ctx: &rustcms_admin::Ctx,
        _txn: &sea_orm::DatabaseTransaction,
        data: rustcms_admin::Data,
    ) -> rustcms_core::AppResult<rustcms_admin::Data> {
        tokio::time::sleep(std::time::Duration::from_millis(2500)).await;
        Ok(data)
    }
}

fn build_late(config: rustcms_core::Config) -> rustcms_core::AppBuilder {
    let mut app = rustcms_core::AppBuilder::new(config);
    app.provider(rustcms_auth::AuthProvider::new());
    app.provider(rustcms_admin::AdminProvider::new().resource::<LateWrite>());
    app
}

db_test!(committed_write_beats_the_request_timeout, |db: TestDb| async move {
    use rustcms_testing::{Factory, UserFactory};
    use sea_orm::{EntityTrait, PaginatorTrait};
    db.migrate::<rustcms_migration::Migrator>().await;
    support::create_tables(db.conn()).await;
    let config = rustcms_testing::test_config(db.url()).with("http.timeout_secs", 3).unwrap();
    let app = TestApp::with_config(build_late, config, db).await;
    let admin = UserFactory::new().create(app.state().db()).await;
    let role = rustcms_auth::entities::role::super_admin_id(app.state().db(), ADMIN_GUARD).await.unwrap().unwrap();
    let auth = app.state().service::<AuthState>().unwrap();
    rustcms_auth::RbacService::new(app.state().db(), &auth.resolver)
        .assign_role(rustcms_auth::Actor::System, admin.id, role)
        .await
        .unwrap();

    let client = app.acting_as(&admin).await;
    let created = client.post_json(&api(&app, "resources/late/records"), &json!({"slug": "s", "label": "l"})).await;
    created.assert_status(StatusCode::CREATED).assert_json("/data/key", json!("s"));
    assert_eq!(support::tag::Entity::find().count(app.state().db()).await.unwrap(), 1);
});

/// Tags whose `view` check (run after a create commits) has another transaction
/// lock the sessions table for 10 s, as a concurrent request might.
struct LocksSessions;

struct LockSessionsPolicy;

#[async_trait::async_trait]
impl rustcms_admin::Policy for LockSessionsPolicy {
    async fn allows(&self, ctx: &rustcms_admin::Ctx, ability: &str, record: Option<&rustcms_admin::Data>) -> bool {
        use sea_orm::{ConnectionTrait, DbBackend, TransactionTrait};
        if ability == "view" && record.is_some() {
            let (locked, ready) = tokio::sync::oneshot::channel();
            let db = ctx.db.clone();
            tokio::spawn(async move {
                let txn = if db.get_database_backend() != DbBackend::Sqlite {
                    let txn = db.begin().await.unwrap();
                    txn.execute_unprepared("SELECT id FROM sessions FOR UPDATE").await.unwrap();
                    txn
                } else {
                    rustcms_core::db::begin_write(&db).await.unwrap()
                };
                locked.send(()).ok();
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                txn.rollback().await.ok();
            });
            ready.await.ok();
        }
        true
    }
}

impl rustcms_admin::Resource for LocksSessions {
    type Entity = support::tag::Entity;
    const SLUG: &'static str = "locks";

    fn meta() -> rustcms_admin::ResourceMeta {
        rustcms_admin::ResourceMeta::new("Locks").key(rustcms_admin::KeyStrategy::Natural)
    }

    fn fields() -> Vec<rustcms_admin::Field> {
        vec![rustcms_admin::Field::make("slug"), rustcms_admin::Field::make("label")]
    }

    fn policy() -> std::sync::Arc<dyn rustcms_admin::Policy> {
        std::sync::Arc::new(LockSessionsPolicy)
    }
}

fn build_locks(config: rustcms_core::Config) -> rustcms_core::AppBuilder {
    let mut app = rustcms_core::AppBuilder::new(config);
    app.provider(rustcms_auth::AuthProvider::new());
    app.provider(rustcms_admin::AdminProvider::new().resource::<LocksSessions>());
    app
}

db_test!(blocked_session_renewal_does_not_hide_a_committed_write, |db: TestDb| async move {
    use chrono::{Duration, Utc};
    use rustcms_testing::{Factory, UserFactory};
    use sea_orm::{EntityTrait, PaginatorTrait};
    db.migrate::<rustcms_migration::Migrator>().await;
    support::create_tables(db.conn()).await;
    let config = rustcms_testing::test_config(db.url()).with("http.timeout_secs", 3).unwrap();
    let app = TestApp::with_config(build_locks, config, db).await;
    let admin = UserFactory::new().create(app.state().db()).await;
    let role = rustcms_auth::entities::role::super_admin_id(app.state().db(), ADMIN_GUARD).await.unwrap().unwrap();
    let auth = app.state().service::<AuthState>().unwrap();
    rustcms_auth::RbacService::new(app.state().db(), &auth.resolver)
        .assign_role(rustcms_auth::Actor::System, admin.id, role)
        .await
        .unwrap();
    let client = app.acting_as(&admin).await;
    // Due for sliding renewal on this request.
    rustcms_auth::entities::session::Entity::update_many()
        .col_expr(
            rustcms_auth::entities::session::Column::ExpiresAt,
            sea_orm::sea_query::Expr::value(Utc::now() + Duration::minutes(10)),
        )
        .exec(app.state().db())
        .await
        .unwrap();

    let created = client.post_json(&api(&app, "resources/locks/records"), &json!({"slug": "s", "label": "l"})).await;
    created.assert_status(StatusCode::CREATED).assert_json("/data/key", json!("s"));
    assert_eq!(support::tag::Entity::find().count(app.state().db()).await.unwrap(), 1);
});

db_test!(panel_pages_are_served_with_strict_headers, |db: TestDb| async move {
    let (app, users): (TestApp, Users) = setup(db).await;
    let anonymous = app.client();
    for page in ["/admin", "/admin/", "/admin/r/posts", "/admin/r/posts/records/create%20me/edit"] {
        let response = anonymous.get(page).await;
        response.assert_status(StatusCode::OK);
        assert_eq!(response.header("content-type").as_deref(), Some("text/html; charset=utf-8"), "{page}");
        assert_eq!(response.header("cache-control").as_deref(), Some("no-store"));
        assert_eq!(
            response.header("content-security-policy").as_deref(),
            Some(rustcms_admin::spa::content_security_policy(&["http://localhost:8000".to_owned()]).as_str())
        );
        assert_eq!(response.header("x-frame-options").as_deref(), Some("DENY"));
        assert_eq!(response.header("x-content-type-options").as_deref(), Some("nosniff"));
        let html = response.text();
        assert!(
            html.contains(r#"<base href="/admin/">"#) || html.contains("not built"),
            "the shell carries the base path: {html}"
        );
    }

    // The API never falls back to HTML, signed in or not, whatever the method.
    let admin = app.acting_as(&users.admin).await;
    for client in [&anonymous, &admin] {
        for path in ["/admin/api", "/admin/api/", "/admin/api/nope", "/admin/api/resources/posts/nope/deeper"] {
            let response = client.get(path).await;
            response.assert_status(StatusCode::NOT_FOUND).assert_json("/code", json!("not_found"));
            assert_eq!(response.header("cache-control").as_deref(), Some("private, no-store"), "{path}");
        }
    }
    admin.post_json("/admin/api/nope", &json!({})).await.assert_status(StatusCode::NOT_FOUND);

    // Assets never load or renew the session: no cookie on a public, cacheable response.
    let asset = admin.get("/admin/assets/missing-0000.js").await;
    assert_eq!(asset.header("set-cookie"), None);
    // Pages that set the cookie (here: a new anonymous session) are never stored.
    let first = app.client().get(&api(&app, "auth/csrf")).await;
    assert!(first.header("set-cookie").is_some());
    assert_eq!(first.header("cache-control").as_deref(), Some("private, no-store"));

    // Missing assets are 404, not the shell; pages only answer GET/HEAD.
    anonymous.get("/admin/assets/missing-0000.js").await.assert_status(StatusCode::NOT_FOUND);
    admin.post_json("/admin/r/posts", &json!({})).await.assert_status(StatusCode::METHOD_NOT_ALLOWED);
    admin.post_json("/admin", &json!({})).await.assert_status(StatusCode::METHOD_NOT_ALLOWED);
    // Outside the admin path nothing changes.
    anonymous.get("/administrator").await.assert_status(StatusCode::NOT_FOUND);
});
