//! Has-many relations over HTTP: children of one visible parent, created with
//! the parent's key set by the server. Both databases.

mod support;

use axum::http::{Method, StatusCode};
use rustcms_testing::{TestApp, TestClient, TestDb, TestResponse, db_test};
use serde_json::{Value, json};
use support::app::setup;

async fn category(app: &TestApp, admin: &TestClient<'_>, name: &str) -> String {
    let body = json!({"name": name, "slug": name.to_lowercase().replace(' ', "-")});
    let created = admin.post_json(&app.admin_api("resources/categories/records"), &body).await;
    created.json()["data"]["key"].as_str().unwrap().to_owned()
}

fn relation(app: &TestApp, parent: &str) -> String {
    app.admin_api(&format!("resources/categories/records/{parent}/relations/notes"))
}

fn titles(response: &TestResponse) -> Vec<Value> {
    response.assert_status(StatusCode::OK);
    let mut titles: Vec<Value> =
        response.json()["data"].as_array().unwrap().iter().map(|r| r["data"]["title"].clone()).collect();
    titles.sort_by_key(|v| v.to_string());
    titles
}

db_test!(relation_lists_show_the_children_of_one_visible_parent, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    let admin = app.acting_as(&users.admin).await;
    let (a, b) = (category(&app, &admin, "Alpha").await, category(&app, &admin, "Beta").await);
    for (title, parent) in [("a1", &a), ("a2", &a), ("b1", &b)] {
        let body = json!({"title": title, "category_id": parent.parse::<i32>().unwrap()});
        admin.post_json(&app.admin_api("resources/notes/records"), &body).await.assert_status(StatusCode::CREATED);
    }
    let editor = app.acting_as(&users.editor).await;
    assert_eq!(titles(&editor.get(&relation(&app, &a)).await), [json!("a1"), json!("a2")]);
    assert_eq!(titles(&editor.get(&format!("{}?search=a2", relation(&app, &a))).await), [json!("a2")]);
    assert_eq!(titles(&editor.get(&relation(&app, &b)).await), [json!("b1")]);
    let page = editor.get(&format!("{}?per_page=1", relation(&app, &a))).await.json();
    assert_eq!((page["meta"]["total"].clone(), page["data"].as_array().unwrap().len()), (json!(2), 1));

    // Hidden (scope) and missing parents are alike: 404.
    let hidden = category(&app, &admin, "Hidden gem").await;
    editor.get(&relation(&app, &hidden)).await.assert_status(StatusCode::NOT_FOUND);
    editor.get(&relation(&app, "999999")).await.assert_status(StatusCode::NOT_FOUND);
    let unknown = app.admin_api(&format!("resources/categories/records/{a}/relations/nope"));
    editor.get(&unknown).await.assert_status(StatusCode::NOT_FOUND);
    // Without the abilities, the same 403 whether the parent exists or not.
    let member = app.acting_as(&users.member).await;
    member.get(&relation(&app, &a)).await.assert_status(StatusCode::FORBIDDEN);
    member.get(&relation(&app, "999999")).await.assert_status(StatusCode::FORBIDDEN);

    // The parent's view schema announces the relation.
    let schema = editor.get(&app.admin_api(&format!("resources/categories/schema?operation=view&key={a}"))).await;
    assert_eq!(
        schema.json()["relations"],
        json!([{"name": "notes", "label": "Notes", "resource": "notes", "field": "category_id", "create": true}])
    );
});

db_test!(relation_creates_set_the_parent_key_on_the_server, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    let admin = app.acting_as(&users.admin).await;
    let a = category(&app, &admin, "Alpha").await;
    let b = category(&app, &admin, "Beta").await;
    let editor = app.acting_as(&users.editor).await;
    let created = editor.post_json(&relation(&app, &a), &json!({"title": "child"})).await;
    created.assert_status(StatusCode::CREATED);
    assert_eq!(created.json()["data"]["data"]["category_id"], json!(a.parse::<i32>().unwrap()));
    // The client never chooses the parent.
    let other = json!({"title": "x", "category_id": b.parse::<i32>().unwrap()});
    editor.post_json(&relation(&app, &a), &other).await.assert_validation_error("category_id");
    // A hidden parent is not found.
    let hidden = category(&app, &admin, "Hidden gem").await;
    editor.post_json(&relation(&app, &hidden), &json!({"title": "x"})).await.assert_status(StatusCode::NOT_FOUND);
    // Without the target's create: 403, whatever the parent.
    let member = app.acting_as(&users.member).await;
    member.post_json(&relation(&app, &a), &json!({"title": "y"})).await.assert_status(StatusCode::FORBIDDEN);
    member.post_json(&relation(&app, "999999"), &json!({"title": "y"})).await.assert_status(StatusCode::FORBIDDEN);
});

db_test!(a_retried_relation_create_is_answered_after_the_parent_is_hidden, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    let admin = app.acting_as(&users.admin).await;
    let a = category(&app, &admin, "Alpha").await;
    let editor = app.acting_as(&users.editor).await;
    editor.fetch_csrf().await;
    let send = |body: Value| {
        let request = axum::http::Request::builder()
            .method(Method::POST)
            .uri(relation(&app, &a))
            .header("idempotency-key", "rel-1")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(body.to_string()))
            .unwrap();
        editor.request(request)
    };
    let first = send(json!({"title": "once"})).await;
    first.assert_status(StatusCode::CREATED);
    let rename = app.admin_api(&format!("resources/categories/records/{a}"));
    admin.send(Method::PATCH, &rename, Some(&json!({"name": "Hidden now"}))).await.assert_status(StatusCode::OK);
    let again = send(json!({"title": "once"})).await;
    again.assert_status(StatusCode::CREATED);
    assert_eq!(first.json()["data"]["key"], again.json()["data"]["key"]);
    // A new create under the now hidden parent is refused.
    editor.post_json(&relation(&app, &a), &json!({"title": "new"})).await.assert_status(StatusCode::NOT_FOUND);
});

struct Orphans;

impl rustcms_admin::Resource for Orphans {
    type Entity = support::category::Entity;
    const SLUG: &'static str = "orphans";

    fn meta() -> rustcms_admin::ResourceMeta {
        rustcms_admin::ResourceMeta::new("Orphan")
    }

    fn fields() -> Vec<rustcms_admin::Field> {
        vec![rustcms_admin::Field::make("name")]
    }

    fn relations() -> Vec<rustcms_admin::Relation> {
        // `notes.category_id` belongs to `categories`, not to `orphans`.
        vec![rustcms_admin::Relation::has_many("notes", "notes", "category_id")]
    }
}

#[test]
#[should_panic(expected = "belongs_to(\"orphans\")")]
fn relations_need_a_foreign_key_to_the_parent() {
    let _ = rustcms_admin::Registry::new()
        .register::<support::CategoryResource>()
        .register::<support::NoteResource>()
        .register::<Orphans>()
        .finish();
}

/// A second app for the cases the shared one cannot express: a child with a
/// custom scope, a per-record policy and a hook, a guarded foreign key, and
/// users with narrow permissions.
mod narrow {
    use std::sync::Arc;

    use rustcms_admin::*;
    use rustcms_auth::{ADMIN_GUARD, Actor, AuthProvider, AuthState, RbacService};
    use rustcms_core::models::user;
    use rustcms_core::{AppBuilder, AppResult, Config};
    use rustcms_testing::{Factory, TestApp, TestDb, UserFactory};
    use sea_orm::{ColumnTrait, DatabaseTransaction, QueryFilter, Select};
    use serde_json::Value;

    use super::support::{category, note};

    pub struct Folders;

    impl Resource for Folders {
        type Entity = category::Entity;
        const SLUG: &'static str = "folders";

        fn meta() -> ResourceMeta {
            ResourceMeta::new("Folder").title("name")
        }

        fn fields() -> Vec<Field> {
            vec![Field::column(category::Column::Name).required(), Field::column(category::Column::Slug).required()]
        }

        fn relations() -> Vec<Relation> {
            vec![
                Relation::has_many("memos", "memos", "category_id"),
                Relation::has_many("locked", "locked_memos", "category_id"),
            ]
        }
    }

    /// Hidden by scope: `private…`; hidden by policy: `secret…`.
    pub struct Memos;

    struct MemoPolicy;

    #[async_trait::async_trait]
    impl Policy for MemoPolicy {
        async fn allows(&self, ctx: &Ctx, ability: &str, record: Option<&Data>) -> bool {
            let secret = record.is_some_and(|r| r["title"].as_str().unwrap_or_default().starts_with("secret"));
            ctx.can(&format!("memos.{ability}")) && !(ability == "view" && secret)
        }

        async fn admits(&self, ctx: &Ctx, ability: &str) -> bool {
            ctx.can(&format!("memos.{ability}"))
        }
    }

    #[async_trait::async_trait]
    impl Resource for Memos {
        type Entity = note::Entity;
        const SLUG: &'static str = "memos";

        fn meta() -> ResourceMeta {
            ResourceMeta::new("Memo").title("title").soft_deletes()
        }

        fn fields() -> Vec<Field> {
            vec![
                Field::column(note::Column::Title).required().searchable(),
                Field::column(note::Column::CategoryId)
                    .belongs_to("folders")
                    .required()
                    .readable(|ctx| ctx.can("memos.fk"))
                    .creatable(|ctx| ctx.can("memos.fk")),
                Field::column(note::Column::DeletedAt).guarded().readable(|_| true),
            ]
        }

        fn policy() -> Arc<dyn Policy> {
            Arc::new(MemoPolicy)
        }

        fn scope(query: Select<note::Entity>, _ctx: &Ctx) -> Select<note::Entity> {
            query.filter(note::Column::Title.not_like("private%"))
        }

        async fn mutate_before_create(_ctx: &Ctx, _txn: &DatabaseTransaction, mut data: Data) -> AppResult<Data> {
            if data["title"] == "swap" {
                data.insert("category_id".into(), Value::Null);
            }
            Ok(data)
        }
    }

    /// The same table, default policy, and a foreign key no user may write.
    pub struct LockedMemos;

    impl Resource for LockedMemos {
        type Entity = note::Entity;
        const SLUG: &'static str = "locked_memos";

        fn meta() -> ResourceMeta {
            ResourceMeta::new("Locked memo").title("title")
        }

        fn fields() -> Vec<Field> {
            vec![
                Field::column(note::Column::Title).required(),
                Field::column(note::Column::CategoryId).belongs_to("folders").guarded().readable(|_| true),
            ]
        }
    }

    fn build(config: Config) -> AppBuilder {
        let mut app = AppBuilder::new(config);
        app.provider(AuthProvider::new().permission(ADMIN_GUARD, "memos.fk", "memos"));
        app.provider(AdminProvider::new().resource::<Folders>().resource::<Memos>().resource::<LockedMemos>());
        app
    }

    pub async fn setup(db: TestDb) -> (TestApp, user::Model) {
        db.migrate::<rustcms_migration::Migrator>().await;
        super::support::create_tables(db.conn()).await;
        let app = TestApp::new(build, db).await;
        let auth = app.state().service::<AuthState>().unwrap();
        let rbac = RbacService::new(app.state().db(), &auth.resolver);
        rbac.sync_permissions(&auth.permissions.all()).await.unwrap();
        let admin = UserFactory::new().name("Admin").create(app.state().db()).await;
        let super_admin =
            rustcms_auth::entities::role::super_admin_id(app.state().db(), ADMIN_GUARD).await.unwrap().unwrap();
        rbac.assign_role(Actor::System, admin.id, super_admin).await.unwrap();
        (app, admin)
    }

    /// A user with exactly `permissions` (through a role named after them).
    pub async fn user_with(app: &TestApp, name: &str, permissions: &[&str]) -> user::Model {
        let auth = app.state().service::<AuthState>().unwrap();
        let rbac = RbacService::new(app.state().db(), &auth.resolver);
        let user = UserFactory::new().name(name).create(app.state().db()).await;
        let permissions: Vec<String> = permissions.iter().map(|p| (*p).to_owned()).collect();
        let role = rbac.create_role(Actor::System, ADMIN_GUARD, name, &permissions).await.unwrap();
        rbac.assign_role(Actor::System, user.id, role.id).await.unwrap();
        user
    }
}

const FOLDERS: [&str; 2] = ["folders.view_any", "folders.view"];
const MEMOS: [&str; 5] = ["memos.view_any", "memos.view", "memos.create", "memos.fk", "memos.restore"];

async fn folder(app: &TestApp, admin: &TestClient<'_>, name: &str) -> String {
    let created =
        admin.post_json(&app.admin_api("resources/folders/records"), &json!({"name": name, "slug": name})).await;
    created.assert_status(StatusCode::CREATED);
    created.json()["data"]["key"].as_str().unwrap().to_owned()
}

fn memos(app: &TestApp, parent: &str, relation: &str) -> String {
    app.admin_api(&format!("resources/folders/records/{parent}/relations/{relation}"))
}

db_test!(relation_lists_keep_the_child_scope_and_policy_in_both_list_paths, |db: TestDb| async move {
    let (app, admin) = narrow::setup(db).await;
    let admin = app.acting_as(&admin).await;
    let (a, b) = (folder(&app, &admin, "a").await, folder(&app, &admin, "b").await);
    for (title, parent) in [("a1", &a), ("secret a2", &a), ("private a3", &a), ("b1", &b)] {
        let body = json!({"title": title, "category_id": parent.parse::<i32>().unwrap()});
        admin.post_json(&app.admin_api("resources/memos/records"), &body).await.assert_status(StatusCode::CREATED);
    }
    let editor = narrow::user_with(
        &app,
        "editor",
        &[&FOLDERS[..], &MEMOS[..], &["locked_memos.view_any", "locked_memos.view"]].concat(),
    )
    .await;
    let editor = app.acting_as(&editor).await;
    // Custom policy (records checked one by one): scope and policy both apply, and the count follows.
    let page = editor.get(&memos(&app, &a, "memos")).await;
    assert_eq!(titles(&page), [json!("a1")]);
    assert_eq!(page.json()["meta"]["total"], json!(1));
    // Default policy (counted in SQL): only the parent condition narrows the list.
    let page = editor.get(&format!("{}?per_page=2", memos(&app, &a, "locked"))).await;
    assert_eq!(page.json()["meta"]["total"], json!(3));
    assert_eq!(page.json()["data"].as_array().unwrap().len(), 2);
    // Trashed children are listed only when asked for, with `restore`.
    let a1 = editor.get(&memos(&app, &a, "memos")).await.json()["data"][0]["key"].as_str().unwrap().to_owned();
    admin
        .send(Method::DELETE, &app.admin_api(&format!("resources/memos/records/{a1}")), None)
        .await
        .assert_status(StatusCode::NO_CONTENT);
    assert_eq!(titles(&editor.get(&memos(&app, &a, "memos")).await), Vec::<Value>::new());
    assert_eq!(titles(&editor.get(&format!("{}?trashed=only", memos(&app, &a, "memos"))).await), [json!("a1")]);
});

db_test!(relation_denials_never_depend_on_the_parent_key, |db: TestDb| async move {
    let (app, admin) = narrow::setup(db).await;
    let admin = app.acting_as(&admin).await;
    let a = folder(&app, &admin, "a").await;
    // Without reading the foreign key, without `restore`, without writing the foreign key:
    // the same 403 for an existing and a missing parent.
    let no_fk =
        narrow::user_with(&app, "no_fk", &[&FOLDERS[..], &["memos.view_any", "memos.view", "memos.create"]].concat())
            .await;
    let no_fk = app.acting_as(&no_fk).await;
    let no_trash = narrow::user_with(&app, "no_trash", &[&FOLDERS[..], &MEMOS[..4]].concat()).await;
    let no_trash = app.acting_as(&no_trash).await;
    let locked =
        narrow::user_with(&app, "locked", &[&FOLDERS[..], &["locked_memos.view_any", "locked_memos.create"]].concat())
            .await;
    let locked = app.acting_as(&locked).await;
    for parent in [a.as_str(), "999999"] {
        no_fk.get(&memos(&app, parent, "memos")).await.assert_status(StatusCode::FORBIDDEN);
        no_fk
            .post_json(&memos(&app, parent, "memos"), &json!({"title": "x"}))
            .await
            .assert_status(StatusCode::FORBIDDEN);
        no_trash
            .get(&format!("{}?trashed=only", memos(&app, parent, "memos")))
            .await
            .assert_status(StatusCode::FORBIDDEN);
        locked
            .post_json(&memos(&app, parent, "locked"), &json!({"title": "x"}))
            .await
            .assert_status(StatusCode::FORBIDDEN);
    }
    // The guarded foreign key: listed, but no create offered.
    let schema = locked.get(&app.admin_api(&format!("resources/folders/schema?operation=view&key={a}"))).await;
    assert_eq!(schema.json()["relations"][0]["create"], json!(false));
});

db_test!(relation_creates_run_every_rule_on_the_injected_key, |db: TestDb| async move {
    let (app, admin) = narrow::setup(db).await;
    let admin = app.acting_as(&admin).await;
    let a = folder(&app, &admin, "a").await;
    let editor = narrow::user_with(&app, "editor", &[&FOLDERS[..], &MEMOS[..]].concat()).await;
    let editor = app.acting_as(&editor).await;
    // The required foreign key is satisfied by the relation.
    editor.post_json(&memos(&app, &a, "memos"), &json!({"title": "ok"})).await.assert_status(StatusCode::CREATED);
    editor
        .post_json(&app.admin_api("resources/memos/records"), &json!({"title": "no"}))
        .await
        .assert_validation_error("category_id");
    // A hook that changes it: nothing is written.
    let swapped = editor.post_json(&memos(&app, &a, "memos"), &json!({"title": "swap"})).await;
    swapped.assert_status(StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(titles(&editor.get(&format!("{}?search=swap", memos(&app, &a, "memos"))).await), Vec::<Value>::new());
    // `view` on the parent without `view_any`: the list works, the reference check refuses the create.
    let viewer = narrow::user_with(&app, "viewer", &[&["folders.view"][..], &MEMOS[..]].concat()).await;
    let viewer = app.acting_as(&viewer).await;
    assert_eq!(titles(&viewer.get(&memos(&app, &a, "memos")).await), [json!("ok")]);
    viewer.post_json(&memos(&app, &a, "memos"), &json!({"title": "v"})).await.assert_validation_error("category_id");
});

db_test!(a_retried_relation_create_is_answered_after_the_parent_is_deleted, |db: TestDb| async move {
    let (app, admin_user) = narrow::setup(db).await;
    let admin = app.acting_as(&admin_user).await;
    let a = folder(&app, &admin, "a").await;
    admin.fetch_csrf().await;
    let send = || {
        let request = axum::http::Request::builder()
            .method(Method::POST)
            .uri(memos(&app, &a, "memos"))
            .header("idempotency-key", "rel-2")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(json!({"title": "once"}).to_string()))
            .unwrap();
        admin.request(request)
    };
    let first = send().await;
    first.assert_status(StatusCode::CREATED);
    admin
        .send(Method::DELETE, &app.admin_api(&format!("resources/folders/records/{a}")), None)
        .await
        .assert_status(StatusCode::NO_CONTENT);
    let again = send().await;
    again.assert_status(StatusCode::CREATED);
    assert_eq!(first.json()["data"]["key"], again.json()["data"]["key"]);
    admin.post_json(&memos(&app, &a, "memos"), &json!({"title": "new"})).await.assert_status(StatusCode::NOT_FOUND);
});
