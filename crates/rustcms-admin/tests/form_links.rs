//! Belongs-to-many links chosen in the record's own form (posts ↔ tags with a
//! `BelongsToManySelect`, ADR 0035): the abilities of the relation manager,
//! admitted before anything is read and decided on the record as stored;
//! targets the user may view, locked; hidden links never touched; the
//! relation hook in the write's transaction. SQLite, PostgreSQL, MySQL.

mod support;

use axum::http::{Method, StatusCode};
use rustcms_auth::{ADMIN_GUARD, Actor, AuthState, RbacService};
use rustcms_core::models::{audit_log, user};
use rustcms_testing::{Factory, TestApp, TestClient, TestDb, TestResponse, UserFactory, db_test};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, QueryOrder, Set};
use serde_json::{Value, json};

use support::app::setup;
use support::{post, post_tag, tag};

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

/// May create and update own posts (scope), and link tags both ways.
const WRITER: [&str; 8] = [
    "posts.view_any",
    "posts.view",
    "posts.create",
    "posts.update",
    "posts.attach_tags",
    "posts.detach_tags",
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

async fn links(app: &TestApp, post: &str) -> Vec<String> {
    let id: uuid::Uuid = post.parse().unwrap();
    let mut rows: Vec<String> = post_tag::Entity::find()
        .filter(post_tag::Column::PostId.eq(id))
        .all(app.state().db())
        .await
        .unwrap()
        .into_iter()
        .map(|r| r.tag_slug)
        .collect();
    rows.sort();
    rows
}

async fn create(client: &TestClient<'_>, app: &TestApp, body: Value) -> TestResponse {
    client.post_json(&app.admin_api("resources/posts/records"), &body).await
}

async fn update(client: &TestClient<'_>, app: &TestApp, post: &str, body: Value) -> TestResponse {
    client.send(Method::PATCH, &app.admin_api(&format!("resources/posts/records/{post}")), Some(&body)).await
}

fn key(response: &TestResponse) -> String {
    response.assert_status(StatusCode::CREATED);
    response.json()["data"]["key"].as_str().unwrap().to_owned()
}

async fn posts(app: &TestApp) -> usize {
    post::Entity::find().all(app.state().db()).await.unwrap().len()
}

db_test!(links_are_written_with_the_record, |db: TestDb| async move {
    let (app, _) = setup(db).await;
    tags(&app, &["rust", "sql", "go"]).await;
    let writer = user_with(&app, "Writer", &WRITER).await;
    let client = app.acting_as(&writer).await;

    // Create: the record, then its links, the hook and the audit entries (the record's first).
    let post =
        key(&create(&client, &app, json!({"title": "Linked", "tags": {"attach": ["sql", "rust", "rust"]}})).await);
    assert_eq!(links(&app, &post).await, ["rust", "sql"]);
    let seen = support::post_links(&post);
    assert_eq!(seen.len(), 1);
    assert_eq!(
        (seen[0].change.relation, &seen[0].change.attached),
        ("tags", &vec!["rust".to_owned(), "sql".to_owned()])
    );
    assert_eq!(seen[0].linked, ["rust", "sql"], "the hook sees the links in the write's transaction");
    let events: Vec<String> = audit_log::Entity::find()
        .filter(audit_log::Column::SubjectKey.eq(post.clone()))
        .order_by_asc(audit_log::Column::Id)
        .all(app.state().db())
        .await
        .unwrap()
        .into_iter()
        .map(|e| e.event)
        .collect();
    assert_eq!(events, ["created", "attached"]);

    // Update: attach and detach in one write, with or without field changes.
    update(&client, &app, &post, json!({"tags": {"attach": ["go"], "detach": ["rust"]}}))
        .await
        .assert_status(StatusCode::OK);
    assert_eq!(links(&app, &post).await, ["go", "sql"]);
    let last = support::post_links(&post).pop().unwrap();
    assert_eq!((last.change.attached, last.change.detached), (vec!["go".to_owned()], vec!["rust".to_owned()]));
    // Converges: what is already linked or unlinked changes nothing, and calls no hook.
    let calls = support::post_links(&post).len();
    let same =
        update(&client, &app, &post, json!({"title": "Renamed", "tags": {"attach": ["go"], "detach": ["rust"]}})).await;
    same.assert_status(StatusCode::OK);
    assert_eq!(links(&app, &post).await, ["go", "sql"]);
    assert_eq!(support::post_links(&post).len(), calls);
    // An empty change is no change.
    update(&client, &app, &post, json!({"tags": {}})).await.assert_status(StatusCode::OK);
    update(&client, &app, &post, json!({"tags": {"attach": [], "detach": []}})).await.assert_status(StatusCode::OK);

    // A failing relation hook rolls the whole create back.
    let before = posts(&app).await;
    let failed = create(&client, &app, json!({"title": "stuck-links", "tags": {"attach": ["go"]}})).await;
    failed.assert_status(StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(posts(&app).await, before);
});

db_test!(form_links_need_the_relation_abilities_and_visible_targets, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    tags(&app, &["rust", "sql", "hidden-x"]).await;
    let admin = app.acting_as(&users.admin).await;

    // Without `attach_tags`: 403 before anything else, whatever the keys.
    let editor = app.acting_as(&users.editor).await;
    let before = posts(&app).await;
    for value in [json!({"attach": ["rust"]}), json!({"attach": ["nope", 5]})] {
        create(&editor, &app, json!({"title": "No", "tags": value})).await.assert_status(StatusCode::FORBIDDEN);
    }
    assert_eq!(posts(&app).await, before);
    // Without `detach_tags`: attaching works, detaching is refused.
    let attacher =
        user_with(&app, "Attacher", &WRITER[..5].iter().chain(&WRITER[6..]).copied().collect::<Vec<_>>()).await;
    let attacher = app.acting_as(&attacher).await;
    let post = key(&create(&attacher, &app, json!({"title": "Mine", "tags": {"attach": ["rust"]}})).await);
    update(&attacher, &app, &post, json!({"tags": {"detach": ["rust"]}})).await.assert_status(StatusCode::FORBIDDEN);
    assert_eq!(links(&app, &post).await, ["rust"]);

    let writer = user_with(&app, "Writer", &WRITER).await;
    let client = app.acting_as(&writer).await;
    // The values' shape (422 on the relation's name); detaching on create is refused.
    for value in [
        json!(["rust"]),
        json!({"attach": "rust"}),
        json!({"attach": [1]}),
        json!({"add": ["rust"]}),
        json!({"attach": ["rust"], "detach": ["rust"]}),
        json!({"attach": (0..101).map(|i| format!("t{i}")).collect::<Vec<_>>()}),
    ] {
        create(&client, &app, json!({"title": "Shape", "tags": value})).await.assert_validation_error("tags");
    }
    create(&client, &app, json!({"title": "Shape", "tags": {"detach": ["rust"]}}))
        .await
        .assert_validation_error("tags");
    // A target the user may not view (scope) or that does not exist: 422, nothing written.
    for slug in ["hidden-x", "nope"] {
        let refused = create(&client, &app, json!({"title": "Hidden", "tags": {"attach": [slug]}})).await;
        refused.assert_validation_error("tags");
    }
    assert_eq!(posts(&app).await, before + 1);

    // A link the user cannot see is kept when they detach it with others.
    let post = key(&create(&client, &app, json!({"title": "Shared", "tags": {"attach": ["rust", "sql"]}})).await);
    admin
        .post_json(
            &app.admin_api(&format!("resources/posts/records/{post}/relations/tags/attach")),
            &json!({"keys": ["hidden-x"]}),
        )
        .await
        .assert_status(StatusCode::OK);
    update(&client, &app, &post, json!({"tags": {"detach": ["hidden-x", "sql"]}})).await.assert_status(StatusCode::OK);
    assert_eq!(links(&app, &post).await, ["hidden-x", "rust"]);

    // The policy decides on the record as stored: `locked*` posts take no new links
    // (here because the same write renames it), and the rename rolls back with them.
    let refused = update(&client, &app, &post, json!({"title": "locked now", "tags": {"attach": ["sql"]}})).await;
    refused.assert_status(StatusCode::FORBIDDEN);
    let stored = post::Entity::find_by_id(post.parse::<uuid::Uuid>().unwrap()).one(app.state().db()).await.unwrap();
    assert_eq!(stored.unwrap().title, "Shared");
    // Others' posts are out of scope: 404, as for any update.
    let theirs = key(&create(&admin, &app, json!({"title": "Theirs"})).await);
    update(&client, &app, &theirs, json!({"tags": {"attach": ["rust"]}})).await.assert_status(StatusCode::NOT_FOUND);
    assert!(links(&app, &theirs).await.is_empty());
    // A relation not in the form (or not a relation) is not a field.
    update(&client, &app, &post, json!({"notes": {"attach": ["1"]}})).await.assert_validation_error("notes");
});

db_test!(form_links_in_schemas_and_create_options, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    tags(&app, &["rust", "hidden-x"]).await;
    let writer = user_with(&app, "Writer", &WRITER).await;
    let client = app.acting_as(&writer).await;
    let input = |schema: &Value| -> Option<Value> {
        fn walk(components: &Value, out: &mut Option<Value>) {
            for c in components.as_array().into_iter().flatten() {
                if c["name"] == json!("tags") {
                    *out = Some(c.clone());
                }
                walk(&c["components"], out);
            }
        }
        let mut out = None;
        walk(&schema["form"]["components"], &mut out);
        out
    };
    let created = input(&schema(&app, &client, "operation=create".into()).await).expect("tags on create");
    assert_eq!(
        (&created["type"], &created["valueType"], &created["relationship"], &created["links"]),
        (
            &json!("belongs_to_many"),
            &json!("links"),
            &json!({"resource": "tags"}),
            &json!({"attach": true, "detach": false})
        )
    );
    let post = key(&create(&client, &app, json!({"title": "Mine"})).await);
    let edit = input(&schema(&app, &client, format!("operation=edit&key={post}")).await).expect("tags on edit");
    assert_eq!(edit["links"], json!({"attach": true, "detach": true}));
    assert!(
        input(&schema(&app, &client, format!("operation=view&key={post}")).await).is_none(),
        "the relation manager shows them"
    );
    // Without the abilities, the input is left out.
    let editor = app.acting_as(&users.editor).await;
    assert!(input(&schema(&app, &editor, "operation=create".into()).await).is_none());

    // The create form's options: the target's lookup (scope), for users who may create and attach.
    let options = app.admin_api("resources/posts/relations/tags/options");
    let found = client.get(&options).await;
    found.assert_status(StatusCode::OK);
    assert_eq!(found.json()["data"], json!([{"value": "rust", "label": "RUST", "attached": false}]));
    editor.get(&options).await.assert_status(StatusCode::FORBIDDEN);
    client.get(&app.admin_api("resources/posts/relations/nope/options")).await.assert_status(StatusCode::NOT_FOUND);
});

async fn schema(app: &TestApp, client: &TestClient<'_>, query: String) -> Value {
    client.get(&app.admin_api(&format!("resources/posts/schema?{query}"))).await.json()
}

#[test]
#[should_panic(expected = "is not a belongs-to-many relation")]
fn a_links_input_names_a_belongs_to_many_relation() {
    struct Broken;
    impl rustcms_admin::Resource for Broken {
        type Entity = post::Entity;
        const SLUG: &'static str = "broken_posts";
        fn meta() -> rustcms_admin::ResourceMeta {
            rustcms_admin::ResourceMeta::new("Post")
        }
        fn fields() -> Vec<rustcms_admin::Field> {
            vec![rustcms_admin::Field::make("title")]
        }
        fn form() -> rustcms_admin::Form {
            rustcms_admin::Form::new()
                .schema(rustcms_admin::components![rustcms_admin::BelongsToManySelect::make("tags")])
        }
    }
    let _ = rustcms_admin::Registry::new().register::<Broken>().finish();
}

db_test!(form_link_targets_lock_in_the_registry_order, |db: TestDb| async move {
    // Parents before their belongs-to-many targets, as every other writer
    // locks them; resources outside the lock graph after, by slug.
    let fx = support::Fixture::new(db).await;
    let registry = fx.ctx(support::admin()).registry;
    assert!(registry.lock_rank("posts") < registry.lock_rank("tags"));
    assert!(registry.lock_rank("tags") < registry.lock_rank("categories"));
    assert!(registry.lock_rank("categories") < registry.lock_rank("notes"));
});

#[test]
#[should_panic(expected = "cannot link the media library")]
fn a_links_input_never_targets_the_media_library() {
    struct Files;
    impl rustcms_admin::Resource for Files {
        type Entity = post::Entity;
        const SLUG: &'static str = "file_posts";
        fn meta() -> rustcms_admin::ResourceMeta {
            rustcms_admin::ResourceMeta::new("Post")
        }
        fn fields() -> Vec<rustcms_admin::Field> {
            vec![rustcms_admin::Field::make("title")]
        }
        fn relations() -> Vec<rustcms_admin::Relation> {
            vec![rustcms_admin::Relation::belongs_to_many(
                "files",
                rustcms_admin::media::SLUG,
                post_tag::Column::PostId,
                post_tag::Column::TagSlug,
            )]
        }
        fn form() -> rustcms_admin::Form {
            rustcms_admin::Form::new()
                .schema(rustcms_admin::components![rustcms_admin::BelongsToManySelect::make("files")])
        }
    }
    let _ = rustcms_admin::Registry::new().register::<Files>().finish();
}
