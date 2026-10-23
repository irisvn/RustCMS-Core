//! Belongs-to-many relations over HTTP (posts ↔ tags through `post_tags`):
//! lists through the pivot, attach, detach and pivot updates; admission before
//! the body, scope and policy on the parent and the targets, hidden targets
//! never touched, pivot columns by their field rules, history. SQLite,
//! PostgreSQL, MySQL.

mod support;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use rustcms_admin::{Field, Registry, Resource, ResourceMeta};
use rustcms_auth::{ADMIN_GUARD, Actor, AuthState, RbacService};
use rustcms_core::models::user;
use rustcms_testing::{Factory, TestApp, TestClient, TestDb, TestResponse, UserFactory, db_test};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use serde_json::{Value, json};

use support::app::setup;
use support::{post, post_tag, tag};

/// A user holding `permissions` (a role of their own).
async fn user_with(app: &TestApp, name: &str, permissions: &[&str]) -> user::Model {
    let auth = app.state().service::<AuthState>().unwrap();
    let db = app.state().db();
    let rbac = RbacService::new(db, &auth.resolver);
    let user = UserFactory::new().name(name).create(db).await;
    let permissions: Vec<String> = permissions.iter().map(|p| (*p).to_owned()).collect();
    let role = rbac.create_role(Actor::System, ADMIN_GUARD, &name.to_lowercase(), &permissions).await.unwrap();
    rbac.assign_role(Actor::System, user.id, role.id).await.unwrap();
    user
}

const LINKER: [&str; 8] = [
    "posts.view_any",
    "posts.view",
    "posts.create",
    "posts.attach_tags",
    "posts.detach_tags",
    "posts.update_pivot_tags",
    "tags.view_any",
    "tags.view",
];

async fn tags(app: &TestApp, slugs: &[&str]) {
    for slug in slugs {
        tag::ActiveModel { slug: Set((*slug).into()), label: Set(slug.to_uppercase()) }
            .insert(app.state().db())
            .await
            .unwrap();
    }
}

async fn post_of(client: &TestClient<'_>, app: &TestApp, title: &str) -> String {
    let created = client.post_json(&app.admin_api("resources/posts/records"), &json!({"title": title})).await;
    created.assert_status(StatusCode::CREATED);
    created.json()["data"]["key"].as_str().unwrap().to_owned()
}

fn rel(app: &TestApp, post: &str, rest: &str) -> String {
    app.admin_api(&format!("resources/posts/records/{post}/relations/tags{rest}"))
}

/// Pivot rows of `post`: tag → (position, note).
async fn links(app: &TestApp, post: &str) -> Vec<(String, i32, Option<String>)> {
    let id: uuid::Uuid = post.parse().unwrap();
    let mut rows: Vec<_> = post_tag::Entity::find()
        .filter(post_tag::Column::PostId.eq(id))
        .all(app.state().db())
        .await
        .unwrap()
        .into_iter()
        .map(|r| (r.tag_slug, r.position, r.note))
        .collect();
    rows.sort();
    rows
}

fn keys(response: &TestResponse) -> Vec<String> {
    response.assert_status(StatusCode::OK);
    let mut keys: Vec<String> =
        response.json()["data"].as_array().unwrap().iter().map(|r| r["key"].as_str().unwrap().to_owned()).collect();
    keys.sort();
    keys
}

async fn raw(client: &TestClient<'_>, method: Method, uri: &str, body: &str) -> TestResponse {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_owned()))
        .unwrap();
    client.request(request).await
}

db_test!(attach_list_detach_and_update_links, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    tags(&app, &["rust", "sql", "hidden-x"]).await;
    let linker = user_with(&app, "Linker", &LINKER).await;
    let client = app.acting_as(&linker).await;
    let post = post_of(&client, &app, "Mine").await;

    // Hidden (scope) and unknown targets are missing, never linked.
    let attached = client
        .post_json(&rel(&app, &post, "/attach"), &json!({"keys": ["sql", "rust", "hidden-x", "nope", "rust"]}))
        .await;
    attached.assert_status(StatusCode::OK);
    assert_eq!(attached.json(), json!({"processed": ["rust", "sql"], "missing": ["hidden-x", "nope"]}));
    assert_eq!(links(&app, &post).await, [("rust".into(), 0, None), ("sql".into(), 0, None)]);

    // The list: targets in scope, readable pivot columns only (`note` needs
    // `posts.read_notes`; `secret` is never declared).
    let list = client.get(&rel(&app, &post, "")).await;
    assert_eq!(keys(&list), ["rust", "sql"]);
    assert_eq!(list.json()["pivot"], json!({"rust": {"position": 0}, "sql": {"position": 0}}));
    assert_eq!(keys(&client.get(&format!("{}?search=RUS", rel(&app, &post, ""))).await), ["rust"]);

    // Attaching again keeps the link and its pivot values.
    let again =
        client.post_json(&rel(&app, &post, "/attach"), &json!({"keys": ["rust"], "pivot": {"position": 5}})).await;
    assert_eq!(again.json(), json!({"processed": ["rust"], "missing": []}));
    assert_eq!(links(&app, &post).await[0], ("rust".into(), 0, None));

    // A link to a target the linker cannot see stays when they detach it.
    let admin = app.acting_as(&users.admin).await;
    admin.post_json(&rel(&app, &post, "/attach"), &json!({"keys": ["hidden-x"]})).await.assert_status(StatusCode::OK);
    assert_eq!(keys(&client.get(&rel(&app, &post, "")).await), ["rust", "sql"]);
    let detached = client.post_json(&rel(&app, &post, "/detach"), &json!({"keys": ["rust", "hidden-x"]})).await;
    assert_eq!(detached.json(), json!({"processed": ["rust"], "missing": ["hidden-x"]}));
    let left: Vec<String> = links(&app, &post).await.into_iter().map(|(t, _, _)| t).collect();
    assert_eq!(left, ["hidden-x", "sql"]);

    // Pivot updates: declared, writable columns only; `note` is write-only here.
    let update = |target: &str| rel(&app, &post, &format!("/links/{target}"));
    let patch = |target: String, body: Value| {
        let client = &client;
        async move { client.send(Method::PATCH, &target, Some(&body)).await }
    };
    patch(update("sql"), json!({"position": 3, "note": "first"})).await.assert_status(StatusCode::OK);
    assert_eq!(links(&app, &post).await[1], ("sql".into(), 3, Some("first".into())));
    patch(update("sql"), json!({"secret": "x"})).await.assert_validation_error("secret");
    patch(update("sql"), json!({"position": "abc"})).await.assert_validation_error("position");
    patch(update("sql"), json!({"note": "x".repeat(21)})).await.assert_validation_error("note");
    // Not linked (any more), or not visible: 404.
    patch(update("rust"), json!({"position": 1})).await.assert_status(StatusCode::NOT_FOUND);
    patch(update("hidden-x"), json!({"position": 1})).await.assert_status(StatusCode::NOT_FOUND);
    assert_eq!(list_pivot(&admin, &app, &post).await["sql"], json!({"position": 3, "note": "first"}));

    // Recorded on the post: the targets actually changed.
    let history = admin.get(&app.admin_api(&format!("resources/posts/records/{post}/history"))).await;
    let events: Vec<(Value, Value)> = history.json()["data"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["meta"]["relation"] == json!("tags"))
        .map(|e| (e["event"].clone(), e["meta"]["targets"].clone()))
        .collect();
    assert_eq!(
        events,
        [
            (json!("pivot_updated"), json!(["sql"])),
            (json!("detached"), json!(["rust"])),
            (json!("attached"), json!(["hidden-x"])),
            (json!("attached"), json!(["rust", "sql"])),
        ]
    );
});

db_test!(link_writes_call_after_relation_change_in_their_transaction, |db: TestDb| async move {
    use support::post_links;
    let (app, users) = setup(db).await;
    tags(&app, &["rust", "sql"]).await;
    let admin = app.acting_as(&users.admin).await;
    let post = post_of(&admin, &app, "Hooked").await;
    // (attached, detached, pivot_updated, linked) per call.
    type Seen = (Vec<String>, Vec<String>, Vec<String>, Vec<String>);
    let summary = |post: &str| -> Vec<Seen> {
        post_links(post)
            .into_iter()
            .map(|l| {
                assert_eq!(l.change.relation, "tags");
                (l.change.attached, l.change.detached, l.change.pivot_updated, l.linked)
            })
            .collect()
    };
    let v = |items: &[&str]| items.iter().map(|s| (*s).to_owned()).collect::<Vec<String>>();

    admin
        .post_json(&rel(&app, &post, "/attach"), &json!({"keys": ["rust", "sql"]}))
        .await
        .assert_status(StatusCode::OK);
    // Already linked: nothing changes, no call.
    admin.post_json(&rel(&app, &post, "/attach"), &json!({"keys": ["rust"]})).await.assert_status(StatusCode::OK);
    admin
        .send(Method::PATCH, &rel(&app, &post, "/links/rust"), Some(&json!({"position": 4})))
        .await
        .assert_status(StatusCode::OK);
    admin
        .send(Method::PATCH, &rel(&app, &post, "/links/rust"), Some(&json!({"position": 4})))
        .await
        .assert_status(StatusCode::OK);
    admin
        .post_json(&rel(&app, &post, "/detach"), &json!({"keys": ["sql", "nope"]}))
        .await
        .assert_status(StatusCode::OK);
    assert_eq!(
        summary(&post),
        [
            (v(&["rust", "sql"]), v(&[]), v(&[]), v(&["rust", "sql"])),
            (v(&[]), v(&[]), v(&["rust"]), v(&["rust", "sql"])),
            (v(&[]), v(&["sql"]), v(&[]), v(&["rust"])),
        ],
        "once per write that changed links, with the pivot rows of the write"
    );

    // A failing hook rolls the link write back.
    let stuck = post_of(&admin, &app, "stuck-links").await;
    let refused = admin.post_json(&rel(&app, &stuck, "/attach"), &json!({"keys": ["rust"]})).await;
    refused.assert_status(StatusCode::INTERNAL_SERVER_ERROR);
    assert!(links(&app, &stuck).await.is_empty());
});

async fn list_pivot(client: &TestClient<'_>, app: &TestApp, post: &str) -> Value {
    client.get(&rel(app, post, "")).await.json()["pivot"].clone()
}

db_test!(link_writes_are_refused_before_the_body_and_the_records, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    tags(&app, &["rust"]).await;
    let linker = user_with(&app, "Linker", &LINKER).await;
    let client = app.acting_as(&linker).await;
    let mine = post_of(&client, &app, "Mine").await;
    let locked = post_of(&client, &app, "locked one").await;
    let admin = app.acting_as(&users.admin).await;
    let theirs = post_of(&admin, &app, "Theirs").await;

    // No admission: 403 whatever the body, the post or the relation's targets.
    let member = app.acting_as(&users.member).await;
    let editor = app.acting_as(&users.editor).await; // may update posts, not attach
    for outsider in [&member, &editor] {
        for post in [mine.as_str(), "00000000-0000-0000-0000-000000000000", "garbage"] {
            for (method, path, body) in [
                (Method::POST, "/attach", "not json"),
                (Method::POST, "/attach", r#"{"keys": 5}"#),
                (Method::POST, "/detach", "{}"),
                (Method::PATCH, "/links/rust", "[]"),
            ] {
                raw(outsider, method, &rel(&app, post, path), body).await.assert_status(StatusCode::FORBIDDEN);
            }
            outsider.get(&rel(&app, post, "/options")).await.assert_status(StatusCode::FORBIDDEN);
        }
    }
    member.get(&rel(&app, &mine, "")).await.assert_status(StatusCode::FORBIDDEN);
    // Unknown relation: 404; the has-many create route does not apply: 405.
    raw(&client, Method::POST, &app.admin_api(&format!("resources/posts/records/{mine}/relations/nope/attach")), "{}")
        .await
        .assert_status(StatusCode::NOT_FOUND);
    client.post_json(&rel(&app, &mine, ""), &json!({})).await.assert_status(StatusCode::METHOD_NOT_ALLOWED);

    // Admitted: the body is checked, then the post (scope, then policy).
    raw(&client, Method::POST, &rel(&app, &mine, "/attach"), "not json")
        .await
        .assert_status(StatusCode::UNPROCESSABLE_ENTITY);
    raw(&client, Method::POST, &rel(&app, &mine, "/attach"), r#"{"keys": [], "x": 1}"#)
        .await
        .assert_status(StatusCode::UNPROCESSABLE_ENTITY);
    client.post_json(&rel(&app, &mine, "/attach"), &json!({"keys": []})).await.assert_validation_error("keys");
    let many: Vec<String> = (0..101).map(|i| format!("t{i}")).collect();
    client.post_json(&rel(&app, &mine, "/attach"), &json!({"keys": many})).await.assert_validation_error("keys");
    client
        .post_json(&rel(&app, &mine, "/attach"), &json!({"keys": ["rust"], "pivot": {"secret": "x"}}))
        .await
        .assert_validation_error("secret");
    client
        .post_json(&rel(&app, &mine, "/detach"), &json!({"keys": ["rust"], "pivot": {"position": 1}}))
        .await
        .assert_validation_error("pivot");
    for post in [theirs.as_str(), "00000000-0000-0000-0000-000000000000"] {
        client
            .post_json(&rel(&app, post, "/attach"), &json!({"keys": ["rust"]}))
            .await
            .assert_status(StatusCode::NOT_FOUND);
        client.get(&rel(&app, post, "/options")).await.assert_status(StatusCode::NOT_FOUND);
    }
    // The policy refuses attaching to `locked*` posts (on the record).
    client
        .post_json(&rel(&app, &locked, "/attach"), &json!({"keys": ["rust"]}))
        .await
        .assert_status(StatusCode::FORBIDDEN);
    client.get(&rel(&app, &locked, "/options")).await.assert_status(StatusCode::FORBIDDEN);
    assert!(links(&app, &locked).await.is_empty() && links(&app, &theirs).await.is_empty());
});

db_test!(schema_and_options_follow_the_user, |db: TestDb| async move {
    let (app, _users) = setup(db).await;
    tags(&app, &["rust", "ruby", "hidden-r"]).await;
    let linker = user_with(&app, "Linker", &LINKER).await;
    let client = app.acting_as(&linker).await;
    let post = post_of(&client, &app, "Mine").await;
    let locked = post_of(&client, &app, "locked one").await;
    client.post_json(&rel(&app, &post, "/attach"), &json!({"keys": ["rust"]})).await.assert_status(StatusCode::OK);

    let options = client.get(&format!("{}?search=r", rel(&app, &post, "/options"))).await;
    options.assert_status(StatusCode::OK);
    assert_eq!(
        options.json()["data"],
        json!([
            {"value": "ruby", "label": "RUBY", "attached": false},
            {"value": "rust", "label": "RUST", "attached": true},
        ])
    );
    let selected = client.get(&format!("{}?keys=rust,hidden-r", rel(&app, &post, "/options"))).await;
    assert_eq!(selected.json()["data"], json!([{"value": "rust", "label": "RUST", "attached": true}]));

    let schema = |key: String| {
        let client = &client;
        let app = &app;
        async move {
            let schema = client.get(&app.admin_api(&format!("resources/posts/schema?operation=view&key={key}"))).await;
            schema.json()["relations"][0].clone()
        }
    };
    let view = client.get(&app.admin_api(&format!("resources/posts/schema?operation=view&key={post}"))).await;
    support::contract("posts.schema.view.json", &view.json());
    let relation = schema(post.clone()).await;
    assert_eq!(relation["kind"], json!("belongsToMany"));
    assert_eq!((relation["attach"].clone(), relation["detach"].clone()), (json!(true), json!(true)));
    let names = |form: &Value| -> Vec<(Value, Value)> {
        form["components"].as_array().unwrap().iter().map(|i| (i["name"].clone(), i["writeOnly"].clone())).collect()
    };
    assert_eq!(relation["pivot"].as_array().unwrap().len(), 1, "readable: position");
    assert_eq!(names(&relation["attachForm"]), [(json!("position"), Value::Null), (json!("note"), json!(true))]);
    assert_eq!(names(&relation["pivotForm"]), [(json!("position"), Value::Null), (json!("note"), json!(true))]);
    let refused = schema(locked).await;
    assert_eq!(refused.get("attach"), None, "the policy refuses attaching to this post");
    assert!(refused.get("attachForm").is_none());
});

db_test!(concurrent_link_writes_stay_consistent, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    let linker = user_with(&app, "Linker", &LINKER).await;
    let one = app.acting_as(&linker).await;
    let two = app.acting_as(&linker).await;
    let admin = app.acting_as(&users.admin).await;
    // PostgreSQL and MySQL wait for each other's locks (one lock order: never a
    // deadlock, so never a 409); SQLite serializes writers and may answer busy.
    let waits = app.state().db().get_database_backend() != sea_orm::DbBackend::Sqlite;
    let ok = |response: &TestResponse, expected: &[StatusCode]| {
        let allowed = expected.contains(&response.status) || (!waits && response.status == StatusCode::CONFLICT);
        assert!(allowed, "{} {}", response.status, response.text());
    };
    for round in 0..4 {
        let (rust, sql) = (format!("rust-{round}"), format!("sql-{round}"));
        tags(&app, &[&rust, &sql]).await;
        let post = post_of(&one, &app, &format!("Mine {round}")).await;
        let attach = rel(&app, &post, "/attach");
        // The same links twice at once: one row each.
        let body = json!({"keys": [rust, sql]});
        let (a, b) = tokio::join!(one.post_json(&attach, &body), two.post_json(&attach, &body));
        ok(&a, &[StatusCode::OK]);
        ok(&b, &[StatusCode::OK]);
        assert!(links(&app, &post).await.len() <= 2);
        // Attaching while the target is deleted: linked then removed with it, or missing.
        one.post_json(&rel(&app, &post, "/detach"), &json!({"keys": [sql]})).await.assert_status(StatusCode::OK);
        let tag_url = app.admin_api(&format!("resources/tags/records/{sql}"));
        let only_sql = json!({"keys": [sql]});
        let (attached, deleted) =
            tokio::join!(one.post_json(&attach, &only_sql), admin.send(Method::DELETE, &tag_url, None));
        ok(&attached, &[StatusCode::OK]);
        ok(&deleted, &[StatusCode::NO_CONTENT]);
        let gone = tag::Entity::find_by_id(sql.clone()).one(app.state().db()).await.unwrap().is_none();
        let linked = links(&app, &post).await.iter().any(|(t, _, _)| *t == sql);
        assert!(!(gone && linked), "no link to a deleted tag");
        // Attaching while the post is deleted: done first, or the post is gone (404).
        let post_url = app.admin_api(&format!("resources/posts/records/{post}"));
        let only_rust = json!({"keys": [rust]});
        let (attached, deleted) =
            tokio::join!(two.post_json(&attach, &only_rust), admin.send(Method::DELETE, &post_url, None));
        ok(&attached, &[StatusCode::OK, StatusCode::NOT_FOUND]);
        ok(&deleted, &[StatusCode::NO_CONTENT]);
        assert!(links(&app, &post).await.is_empty(), "links go with the post");
    }
});

db_test!(history_shows_links_as_the_reader_may_see_them, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    tags(&app, &["rust"]).await;
    let admin = app.acting_as(&users.admin).await;
    // The reader's post; they may not list tags (nor attach them).
    let reader = user_with(&app, "Reader", &["audit.view", "posts.view_any", "posts.view", "posts.create"]).await;
    let reader = app.acting_as(&reader).await;
    let post = post_of(&reader, &app, "Theirs").await;
    admin
        .post_json(&rel(&app, &post, "/attach"), &json!({"keys": ["rust"], "pivot": {"position": 2, "note": "n"}}))
        .await
        .assert_status(StatusCode::OK);
    // Stored with the note redacted (it has a `readable` rule).
    let row = rustcms_core::models::audit_log::Entity::find()
        .filter(rustcms_core::models::audit_log::Column::Event.eq("attached"))
        .one(app.state().db())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.meta.unwrap()["pivot"], json!({"position": 2, "note": "[redacted]"}));
    // A reader who may not list tags sees the relation and a count.
    let history = reader.get(&app.admin_api(&format!("resources/posts/records/{post}/history"))).await;
    let entry = history.json()["data"].as_array().unwrap().iter().find(|e| e["event"] == json!("attached")).cloned();
    assert_eq!(entry.unwrap()["meta"], json!({"relation": "tags", "count": 1}), "no targets, no pivot values");
    let admin_view = admin.get(&app.admin_api(&format!("resources/posts/records/{post}/history"))).await;
    let entry = admin_view.json()["data"].as_array().unwrap().iter().find(|e| e["event"] == json!("attached")).cloned();
    assert_eq!(
        entry.unwrap()["meta"],
        json!({"relation": "tags", "count": 1, "targets": ["rust"], "pivot": {"position": 2, "note": "[redacted]"}})
    );
});

/// Configurations outside the one lock order are refused at boot.
#[test]
fn misdeclared_pivots_stop_the_boot() {
    struct Again;
    impl Resource for Again {
        type Entity = post::Entity;
        const SLUG: &'static str = "again";
        fn meta() -> ResourceMeta {
            ResourceMeta::new("Again")
        }
        fn fields() -> Vec<Field> {
            vec![Field::column(post::Column::Title)]
        }
        fn relations() -> Vec<rustcms_admin::Relation> {
            vec![rustcms_admin::Relation::belongs_to_many(
                "tags",
                "tags",
                post_tag::Column::PostId,
                post_tag::Column::TagSlug,
            )]
        }
    }
    struct KeyAsField;
    impl Resource for KeyAsField {
        type Entity = post::Entity;
        const SLUG: &'static str = "key_as_field";
        fn meta() -> ResourceMeta {
            ResourceMeta::new("K")
        }
        fn fields() -> Vec<Field> {
            vec![Field::column(post::Column::Title)]
        }
        fn relations() -> Vec<rustcms_admin::Relation> {
            vec![
                rustcms_admin::Relation::belongs_to_many(
                    "tags",
                    "tags",
                    post_tag::Column::PostId,
                    post_tag::Column::TagSlug,
                )
                .pivot(vec![Field::column(post_tag::Column::TagSlug)]),
            ]
        }
    }
    struct WrongKey;
    impl Resource for WrongKey {
        type Entity = support::category::Entity;
        const SLUG: &'static str = "wrong_key";
        fn meta() -> ResourceMeta {
            ResourceMeta::new("W")
        }
        fn fields() -> Vec<Field> {
            vec![Field::column(support::category::Column::Name)]
        }
        fn relations() -> Vec<rustcms_admin::Relation> {
            vec![rustcms_admin::Relation::belongs_to_many(
                "tags",
                "tags",
                post_tag::Column::PostId,
                post_tag::Column::TagSlug,
            )]
        }
    }
    let boot = |registry: fn() -> Registry| std::panic::catch_unwind(move || registry().finish()).is_err();
    /// Tags linking back to posts (another pivot): posts → tags → posts.
    struct TagsBack;
    impl Resource for TagsBack {
        type Entity = tag::Entity;
        const SLUG: &'static str = "tags";
        fn meta() -> ResourceMeta {
            ResourceMeta::new("Tag").title("label")
        }
        fn fields() -> Vec<Field> {
            vec![Field::column(tag::Column::Label)]
        }
        fn relations() -> Vec<rustcms_admin::Relation> {
            vec![rustcms_admin::Relation::belongs_to_many(
                "posts",
                "posts",
                support::tag_post::Column::TagSlug,
                support::tag_post::Column::PostId,
            )]
        }
    }
    let cycle = || {
        Registry::new()
            .register::<TagsBack>()
            .register::<support::PostResource>()
            .register::<support::CategoryResource>()
            .register::<support::NoteResource>()
    };
    assert!(boot(cycle), "a lock order cycle");
    fn base() -> Registry {
        Registry::new().register::<support::TagResource>()
    }
    assert!(
        boot(|| base()
            .register::<support::PostResource>()
            .register::<Again>()
            .register::<support::CategoryResource>()
            .register::<support::NoteResource>()),
        "one relation per pivot"
    );
    assert!(boot(|| base().register::<KeyAsField>()), "a key column is not a pivot field");
    assert!(boot(|| base().register::<WrongKey>()), "the pivot's key types");
    let fine = || {
        base()
            .register::<support::PostResource>()
            .register::<support::CategoryResource>()
            .register::<support::NoteResource>()
    };
    assert!(!boot(fine), "the test resources boot");
}

db_test!(permissions_name_their_matrix_cell, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    let admin = app.acting_as(&users.admin).await;
    let listed = admin.get(&app.admin_api("system/permissions")).await;
    listed.assert_status(StatusCode::OK);
    let cell = |name: &str| {
        let all = listed.json();
        let found = all.as_array().unwrap().iter().find(|p| p["name"] == json!(name)).cloned();
        let found = found.unwrap_or_else(|| panic!("{name} listed"));
        (found["resource"].clone(), found["ability"].clone())
    };
    assert_eq!(cell("posts.update"), (json!("posts"), json!("update")));
    assert_eq!(cell("posts.attach_tags"), (json!("posts"), json!("attach_tags")));
    assert_eq!(cell("posts.update_pivot_tags"), (json!("posts"), json!("update_pivot_tags")));
    assert_eq!(cell("posts.move"), (json!("posts"), json!("move")));
    // An explicit admission permission (`archive` checks `update` per record).
    assert_eq!(cell("posts.archive"), (Value::Null, Value::Null));
    // Not a resource's ability: outside the matrix.
    assert_eq!(cell("settings.general.view"), (Value::Null, Value::Null));
    assert_eq!(cell("roles.view_any"), (Value::Null, Value::Null));
});
