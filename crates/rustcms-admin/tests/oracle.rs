//! Checks the hand-written oracle `fixtures/authz/admin.toml` against the
//! running admin API, and that it covers every admin route.

mod support;

use std::collections::BTreeSet;

use axum::http::{Method, StatusCode};
use rustcms_testing::{TestApp, TestClient, TestDb, db_test};
use serde::Deserialize;
use serde_json::json;
use support::app::Users;

#[derive(Deserialize)]
struct Oracle {
    case: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    route: String,
    path: String,
    actor: String,
    status: u16,
    body: Option<String>,
}

fn oracle() -> Oracle {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/authz/admin.toml");
    toml::from_str(&std::fs::read_to_string(path).expect("oracle file")).expect("oracle syntax")
}

struct Extra {
    /// users.view_any, users.view, users.update, users.delete, roles.assign; no super admin.
    manager: rustcms_core::models::user::Model,
    /// Super admin of the admin guard only.
    panel_super: rustcms_core::models::user::Model,
    /// media.view_any, media.view, media.delete, media.view_all.
    librarian: rustcms_core::models::user::Model,
    /// media.view_any, media.view, media.delete (own files only).
    uploader: rustcms_core::models::user::Model,
}

async fn extra(app: &TestApp) -> Extra {
    use rustcms_auth::{ADMIN_GUARD, Actor, AuthState, RbacService};
    use rustcms_testing::{Factory, UserFactory};
    let auth = app.state().service::<AuthState>().unwrap();
    let db = app.state().db();
    let rbac = RbacService::new(db, &auth.resolver);
    let permissions: Vec<String> =
        ["users.view_any", "users.view", "users.update", "users.delete", "roles.assign"].map(String::from).to_vec();
    let role = rbac.create_role(Actor::System, ADMIN_GUARD, "manager", &permissions).await.unwrap();
    let manager = UserFactory::new().name("Manager").create(db).await;
    rbac.assign_role(Actor::System, manager.id, role.id).await.unwrap();
    let panel_super = UserFactory::new().name("Panel super").create(db).await;
    let super_role = rustcms_auth::entities::role::super_admin_id(db, ADMIN_GUARD).await.unwrap().unwrap();
    rbac.assign_role(Actor::System, panel_super.id, super_role).await.unwrap();
    let media_user = async |name: &str, permissions: &[&str]| {
        let permissions: Vec<String> = permissions.iter().map(|p| (*p).to_owned()).collect();
        let role = rbac.create_role(Actor::System, ADMIN_GUARD, name, &permissions).await.unwrap();
        let user = UserFactory::new().name(name).create(db).await;
        rbac.assign_role(Actor::System, user.id, role.id).await.unwrap();
        user
    };
    let uploader = media_user("uploader", &["media.view_any", "media.view", "media.delete"]).await;
    let librarian = media_user("librarian", &["media.view_any", "media.view", "media.delete", "media.view_all"]).await;
    Extra { manager, panel_super, librarian, uploader }
}

async fn actor<'a>(app: &'a TestApp, users: &Users, extra: &Extra, name: &str) -> TestClient<'a> {
    match name {
        "manager" => app.acting_as(&extra.manager).await,
        "panel_super" => app.acting_as(&extra.panel_super).await,
        "librarian" => app.acting_as(&extra.librarian).await,
        "uploader" => app.acting_as(&extra.uploader).await,
        "guest" => {
            let client = app.client();
            client.fetch_csrf().await;
            client
        }
        "member" => app.acting_as(&users.member).await,
        "editor" => app.acting_as(&users.editor).await,
        "admin" => app.acting_as(&users.admin).await,
        "foreign" => app.acting_as(&users.editor).await.origin(Some("https://evil.example")),
        "token" => app.with_token(&users.admin, &["*"]).await,
        other => panic!("unknown actor `{other}` in oracle"),
    }
}

#[test]
fn oracle_covers_every_admin_route() {
    let covered: BTreeSet<String> = oracle().case.into_iter().map(|c| c.route).collect();
    for (method, path) in rustcms_admin::http::routes("/admin") {
        let route = format!("{method} {path}");
        assert!(covered.contains(&route), "route `{route}` has no case in fixtures/authz/admin.toml");
    }
}

db_test!(oracle_matches_the_admin_api, |db: TestDb| async move {
    // One anonymous session per guest case: more than a real browser makes in a minute.
    let config = rustcms_testing::test_config(db.url()).with("auth.anonymous_sessions_per_ip", 10_000).unwrap();
    let (app, users) = support::app::setup_with(db, config).await;
    let post = async |user, title: &str| {
        let response = app
            .acting_as(user)
            .await
            .post_json(&app.admin_api("resources/posts/records"), &json!({"title": title}))
            .await;
        response.json()["data"]["key"].as_str().unwrap().to_owned()
    };
    let extra = extra(&app).await;
    let own = post(&users.editor, "own").await;
    let spare = post(&users.editor, "spare").await;
    let theirs = post(&users.admin, "theirs").await;
    let note = async |title: &str| {
        let client = app.acting_as(&users.admin).await;
        let response = client.post_json(&app.admin_api("resources/notes/records"), &json!({"title": title})).await;
        response.json()["data"]["key"].as_str().unwrap().to_owned()
    };
    let active_note = note("active").await;
    let trashed_note = note("trashed").await;
    let trash = app.acting_as(&users.admin).await;
    trash
        .send(Method::DELETE, &app.admin_api(&format!("resources/notes/records/{trashed_note}")), None)
        .await
        .assert_status(StatusCode::NO_CONTENT);
    // A file of the admin's (inserted directly: the oracle needs no storage).
    let media = {
        use sea_orm::{ActiveModelTrait, ActiveValue::Set};
        let id = uuid::Uuid::now_v7();
        rustcms_admin::media::entity::ActiveModel {
            id: Set(id),
            disk: Set("local".into()),
            key: Set(format!("media/{id}")),
            name: Set("theirs.png".into()),
            mime: Set("image/png".into()),
            size: Set(1),
            sha256: Set("0".repeat(64)),
            width: Set(None),
            height: Set(None),
            owner_id: Set(users.admin.id),
            created_at: Set(chrono::Utc::now()),
            deleted_at: Set(None),
        }
        .insert(app.state().db())
        .await
        .unwrap();
        id.to_string()
    };
    let role_id = async |name: &str| {
        use rustcms_auth::entities::role;
        use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
        let found = role::Entity::find().filter(role::Column::Name.eq(name)).one(app.state().db()).await.unwrap();
        found.unwrap().id.to_string()
    };
    let (editor_role, super_role) = (role_id("editor").await, role_id("super_admin").await);
    let ids = [
        ("{member_id}", users.member.id.to_string()),
        ("{editor_id}", users.editor.id.to_string()),
        ("{admin_id}", users.admin.id.to_string()),
        ("{member_email}", users.member.email.clone()),
        ("{editor_email}", users.editor.email.clone()),
        ("{editor_role}", editor_role),
        ("{super_role}", super_role),
    ];
    let fill = |text: &str| {
        let mut text = text
            .replace("{own}", &own)
            .replace("{spare}", &spare)
            .replace("{theirs}", &theirs)
            .replace("{note}", &active_note)
            .replace("{trashed}", &trashed_note)
            .replace("{media}", &media);
        for (placeholder, value) in &ids {
            text = text.replace(placeholder, value);
        }
        text
    };

    let mut failures = Vec::new();
    for case in oracle().case {
        let client = actor(&app, &users, &extra, &case.actor).await;
        let (method, _) = case.route.split_once(' ').expect("`METHOD /path`");
        let body = case.body.as_deref().map(|b| serde_json::from_str(&fill(b)).expect("case body is JSON"));
        let response = client.send(method.parse::<Method>().unwrap(), &fill(&case.path), body.as_ref()).await;
        if response.status != StatusCode::from_u16(case.status).unwrap() {
            failures.push(format!(
                "{} {} as {}: expected {}, got {} {}",
                method,
                case.path,
                case.actor,
                case.status,
                response.status,
                String::from_utf8_lossy(&response.body)
            ));
        }
    }
    assert!(failures.is_empty(), "oracle mismatches:\n{}", failures.join("\n"));
});
