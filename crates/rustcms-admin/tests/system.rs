//! System pages: users, roles and permissions over HTTP, with the escalation
//! rules of `RbacService` behind them.

mod support;

use axum::http::StatusCode;
use rustcms_auth::{ADMIN_GUARD, Actor, AuthState, RbacService};
use rustcms_core::models::user;
use rustcms_testing::{Factory, TestApp, TestDb, UserFactory, db_test};
use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter};
use serde_json::json;
use support::app::{Users, setup};

fn rbac(app: &TestApp) -> (std::sync::Arc<AuthState>, &sea_orm::DatabaseConnection) {
    (app.state().service::<AuthState>().unwrap(), app.state().db())
}

/// A user holding a role with exactly `permissions`.
async fn with_role(app: &TestApp, name: &str, permissions: &[&str]) -> user::Model {
    let (auth, db) = rbac(app);
    let service = RbacService::new(db, &auth.resolver);
    let permissions: Vec<String> = permissions.iter().map(|p| (*p).to_owned()).collect();
    let role = service.create_role(Actor::System, ADMIN_GUARD, name, &permissions).await.unwrap();
    let user = UserFactory::new().name(name).create(db).await;
    service.assign_role(Actor::System, user.id, role.id).await.unwrap();
    user
}

fn api(app: &TestApp, path: &str) -> String {
    app.admin_api(&format!("system/{path}"))
}

db_test!(user_managers_cannot_hand_out_roles, |db: TestDb| async move {
    let (app, users): (TestApp, Users) = setup(db).await;
    let manager = with_role(&app, "user-manager", &["users.view_any", "users.view", "users.create"]).await;
    let client = app.acting_as(&manager).await;
    let editor_role = support::role_id(&app, "editor").await;

    let body = json!({"name": "New", "email": "new@example.test", "password": "long enough password"});
    client.post_json(&api(&app, "users"), &body).await.assert_status(StatusCode::CREATED);
    // Asking for a role needs roles.assign, and the whole creation is refused.
    let with_role = json!({"name": "Other", "email": "other@example.test", "password": "long enough password",
                           "roleIds": [editor_role]});
    client.post_json(&api(&app, "users"), &with_role).await.assert_status(StatusCode::FORBIDDEN);
    let other = user::Entity::find().filter(user::Column::Email.eq("other@example.test")).count(app.state().db());
    assert_eq!(other.await.unwrap(), 0, "nothing is created when a role cannot be granted");
    let _ = users;
});

db_test!(super_admins_are_managed_by_super_admins_only, |db: TestDb| async move {
    let (app, users): (TestApp, Users) = setup(db).await;
    let manager = with_role(&app, "people", &["users.view_any", "users.view", "users.update", "users.delete"]).await;
    let client = app.acting_as(&manager).await;
    let admin = users.admin.id;
    let send = |path: String, body: serde_json::Value| {
        let client = &client;
        async move { client.send(axum::http::Method::PUT, &path, Some(&body)).await }
    };
    send(api(&app, &format!("users/{admin}/password")), json!({"password": "taken over account"}))
        .await
        .assert_status(StatusCode::FORBIDDEN);
    send(api(&app, &format!("users/{admin}/active")), json!({"active": false, "expected": true}))
        .await
        .assert_status(StatusCode::FORBIDDEN);
    client
        .send(
            axum::http::Method::PATCH,
            &api(&app, &format!("users/{admin}")),
            Some(&json!({"name": "x", "email": "x@example.test", "expected": {"name": "Admin", "email": users.admin.email}})),
        )
        .await
        .assert_status(StatusCode::FORBIDDEN);
    client
        .send(axum::http::Method::DELETE, &api(&app, &format!("users/{admin}")), None)
        .await
        .assert_status(StatusCode::FORBIDDEN);
    // An ordinary user is fine.
    let member = users.member.id;
    send(api(&app, &format!("users/{member}/active")), json!({"active": false, "expected": true}))
        .await
        .assert_status(StatusCode::OK);
    // The listing tells the panel what is possible.
    let list = client.get(&api(&app, "users")).await;
    let rows = list.json()["data"].as_array().unwrap().clone();
    let admin_row = rows.iter().find(|r| r["id"] == json!(admin.to_string())).unwrap();
    assert_eq!(admin_row["abilities"]["resetPassword"], json!(false));
    assert_eq!(admin_row["superAdmin"], json!(true));
});

db_test!(roles_cannot_be_used_to_escalate, |db: TestDb| async move {
    let (app, _users): (TestApp, Users) = setup(db).await;
    let lead =
        with_role(&app, "lead", &["roles.view_any", "roles.view", "roles.create", "roles.update", "posts.view"]).await;
    let client = app.acting_as(&lead).await;
    // Only permissions the lead holds can go into a role.
    client
        .post_json(&api(&app, "roles"), &json!({"name": "power", "permissions": ["users.delete"]}))
        .await
        .assert_status(StatusCode::FORBIDDEN);
    let created =
        client.post_json(&api(&app, "roles"), &json!({"name": "readers", "permissions": ["posts.view"]})).await;
    created.assert_status(StatusCode::CREATED);
    let id = created.json()["id"].as_str().unwrap().to_owned();
    client
        .send(
            axum::http::Method::PUT,
            &api(&app, &format!("roles/{id}/permissions")),
            Some(&json!({"permissions": ["users.delete"], "expectedPermissions": ["posts.view"]})),
        )
        .await
        .assert_status(StatusCode::FORBIDDEN);
    // The lead's own role is off limits.
    let own = support::role_id(&app, "lead").await;
    client
        .send(
            axum::http::Method::PUT,
            &api(&app, &format!("roles/{own}/permissions")),
            Some(&json!({"permissions": [], "expectedPermissions": []})),
        )
        .await
        .assert_status(StatusCode::FORBIDDEN);
    // Permission listing marks what the lead could grant.
    let permissions = client.get(&api(&app, "permissions")).await.json();
    let grantable = |name: &str| {
        permissions.as_array().unwrap().iter().find(|p| p["name"] == json!(name)).unwrap()["grantable"].clone()
    };
    assert_eq!(grantable("posts.view"), json!(true));
    assert_eq!(grantable("users.delete"), json!(false));
});

db_test!(password_reset_ends_the_targets_sessions, |db: TestDb| async move {
    let (app, users): (TestApp, Users) = setup(db).await;
    let target =
        UserFactory::new().email("target@example.test").password("original password").create(app.state().db()).await;
    let browser = app.login_as("target@example.test", "original password").await;
    browser.get(&app.admin_api("auth/me")).await.assert_status(StatusCode::OK);
    app.acting_as(&users.admin)
        .await
        .send(
            axum::http::Method::PUT,
            &api(&app, &format!("users/{}/password", target.id)),
            Some(&json!({"password": "a brand new password"})),
        )
        .await
        .assert_status(StatusCode::NO_CONTENT);
    browser.get(&app.admin_api("auth/me")).await.assert_status(StatusCode::UNAUTHORIZED);
    app.login_as("target@example.test", "a brand new password")
        .await
        .get(&app.admin_api("auth/me"))
        .await
        .assert_status(StatusCode::OK);
});

db_test!(role_sets_keep_a_super_admin_and_never_touch_oneself, |db: TestDb| async move {
    let (app, users): (TestApp, Users) = setup(db).await;
    let (auth, db) = rbac(&app);
    let service = RbacService::new(db, &auth.resolver);
    // The only super admin cannot lose the role, even by the system.
    let error = service.set_user_roles(Actor::System, users.admin.id, ADMIN_GUARD, &[]).await.unwrap_err();
    assert_eq!(error.status(), StatusCode::UNPROCESSABLE_ENTITY);
    // Nobody edits their own roles through the panel.
    app.acting_as(&users.admin)
        .await
        .send(
            axum::http::Method::PUT,
            &api(&app, &format!("users/{}/roles", users.admin.id)),
            Some(&json!({"roleIds": [], "expectedRoleIds": []})),
        )
        .await
        .assert_status(StatusCode::FORBIDDEN);
    // Unknown or foreign-guard roles are a validation error.
    app.acting_as(&users.admin)
        .await
        .send(
            axum::http::Method::PUT,
            &api(&app, &format!("users/{}/roles", users.member.id)),
            Some(&json!({"roleIds": ["999999"], "expectedRoleIds": []})),
        )
        .await
        .assert_status(StatusCode::UNPROCESSABLE_ENTITY);
});

db_test!(user_search_is_literal_and_case_insensitive, |db: TestDb| async move {
    let (app, users): (TestApp, Users) = setup(db).await;
    UserFactory::new().name("Zoë 100% Real").email("zoe@example.test").create(app.state().db()).await;
    let admin = app.acting_as(&users.admin).await;
    let names = |r: rustcms_testing::TestResponse| {
        r.json()["data"].as_array().unwrap().iter().map(|u| u["name"].clone()).collect::<Vec<_>>()
    };
    assert_eq!(names(admin.get(&format!("{}?search=ZOE%40", api(&app, "users"))).await), [json!("Zoë 100% Real")]);
    assert_eq!(names(admin.get(&format!("{}?search=100%25", api(&app, "users"))).await), [json!("Zoë 100% Real")]);
    assert!(names(admin.get(&format!("{}?search=%25%25%25%25", api(&app, "users"))).await).is_empty());
    admin
        .get(&format!("{}?search={}", api(&app, "users"), "x".repeat(101)))
        .await
        .assert_status(StatusCode::UNPROCESSABLE_ENTITY);
});

db_test!(panel_links_follow_permissions, |db: TestDb| async move {
    let (app, users): (TestApp, Users) = setup(db).await;
    let panel = |user| {
        let app = &app;
        async move { app.acting_as(user).await.get(&app.admin_api("panel")).await.json()["system"].clone() }
    };
    assert_eq!(
        panel(&users.admin).await,
        json!({"users": true, "roles": true, "createUsers": true, "createRoles": true, "audit": true})
    );
    assert_eq!(
        panel(&users.editor).await,
        json!({"users": false, "roles": false, "createUsers": false, "createRoles": false, "audit": false})
    );
});

async fn reset(app: &TestApp, client: &rustcms_testing::TestClient<'_>, id: i64) -> rustcms_testing::TestResponse {
    let body = json!({"password": "chosen for them"});
    client.send(axum::http::Method::PUT, &api(app, &format!("users/{id}/password")), Some(&body)).await
}

async fn profile(
    app: &TestApp,
    client: &rustcms_testing::TestClient<'_>,
    id: i64,
    email: &str,
) -> rustcms_testing::TestResponse {
    let current = user::Entity::find_by_id(id).one(app.state().db()).await.unwrap().unwrap();
    let body = json!({"name": "Renamed", "email": email, "expected": {"name": current.name, "email": current.email}});
    client.send(axum::http::Method::PATCH, &api(app, &format!("users/{id}")), Some(&body)).await
}

db_test!(credentials_are_replaced_by_super_admins_of_the_same_guard_only, |db: TestDb| async move {
    let (app, users): (TestApp, Users) = setup(db).await;
    // A delegated manager holding even more than the target may not choose
    // their password or email: policies may decide on the identity (ownership).
    let manager = with_role(&app, "helpdesk", &["users.view_any", "users.view", "users.update", "posts.view"]).await;
    let helpdesk = app.acting_as(&manager).await;
    reset(&app, &helpdesk, users.member.id).await.assert_status(StatusCode::FORBIDDEN);
    profile(&app, &helpdesk, users.member.id, "taken@example.test").await.assert_status(StatusCode::FORBIDDEN);
    // Renaming without touching the email is fine.
    let member_email = users.member.email.clone();
    profile(&app, &helpdesk, users.member.id, &member_email).await.assert_status(StatusCode::OK);

    // A super admin may, within their guard...
    let admin = app.acting_as(&users.admin).await;
    reset(&app, &admin, users.editor.id).await.assert_status(StatusCode::NO_CONTENT);
    profile(&app, &admin, users.editor.id, "editor-new@example.test").await.assert_status(StatusCode::OK);
    // ...but a super admin of the admin guard only may not: the same credentials
    // reach the API, where access may be granted by identity alone (gates).
    let (auth, db) = rbac(&app);
    let service = RbacService::new(db, &auth.resolver);
    let panel_only = UserFactory::new().name("Panel only").create(db).await;
    let admin_super = rustcms_auth::entities::role::super_admin_id(db, ADMIN_GUARD).await.unwrap().unwrap();
    service.assign_role(Actor::System, panel_only.id, admin_super).await.unwrap();
    let panel_only = app.acting_as(&panel_only).await;
    reset(&app, &panel_only, users.member.id).await.assert_status(StatusCode::FORBIDDEN);
    profile(&app, &panel_only, users.member.id, "member-new@example.test").await.assert_status(StatusCode::FORBIDDEN);
});

db_test!(stale_drafts_cannot_undo_changes_made_meanwhile, |db: TestDb| async move {
    let (app, users): (TestApp, Users) = setup(db).await;
    let admin = app.acting_as(&users.admin).await;
    let editor_role = support::role_id(&app, "editor").await;
    let member = users.member.id;
    // Two admins open the member's roles (none). One assigns `editor`...
    admin
        .send(
            axum::http::Method::PUT,
            &api(&app, &format!("users/{member}/roles")),
            Some(&json!({"roleIds": [editor_role], "expectedRoleIds": []})),
        )
        .await
        .assert_status(StatusCode::OK);
    // ...the other saves an older draft: refused, nothing overwritten.
    let stale = admin
        .send(
            axum::http::Method::PUT,
            &api(&app, &format!("users/{member}/roles")),
            Some(&json!({"roleIds": [], "expectedRoleIds": []})),
        )
        .await;
    stale.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
    assert!(stale.json()["formErrors"][0].as_str().unwrap().contains("changed by someone else"));
    let roles = admin.get(&api(&app, &format!("users/{member}"))).await.json()["roles"].clone();
    assert_eq!(roles.as_array().unwrap().len(), 1);

    // Same for role permissions.
    let current = admin.get(&api(&app, &format!("roles/{editor_role}"))).await.json()["permissions"].clone();
    let path = api(&app, &format!("roles/{editor_role}/permissions"));
    admin
        .send(
            axum::http::Method::PUT,
            &path,
            Some(&json!({"permissions": ["posts.view"], "expectedPermissions": current})),
        )
        .await
        .assert_status(StatusCode::OK);
    admin
        .send(
            axum::http::Method::PUT,
            &path,
            Some(&json!({"permissions": ["posts.view", "posts.delete"], "expectedPermissions": current})),
        )
        .await
        .assert_status(StatusCode::UNPROCESSABLE_ENTITY);
});

db_test!(role_edits_keep_orphaned_grants, |db: TestDb| async move {
    use sea_orm::{ActiveModelTrait, IntoActiveModel, Set};
    let (app, users): (TestApp, Users) = setup(db).await;
    let (auth, db) = rbac(&app);
    let service = RbacService::new(db, &auth.resolver);
    let role = service
        .create_role(Actor::System, ADMIN_GUARD, "legacy", &["posts.view".into(), "posts.update".into()])
        .await
        .unwrap();
    // `posts.update` stops being declared (orphaned)...
    let orphan = rustcms_auth::entities::permission::Entity::find()
        .filter(rustcms_auth::entities::permission::Column::Name.eq("posts.update"))
        .one(db)
        .await
        .unwrap()
        .unwrap();
    let mut active = orphan.clone().into_active_model();
    active.orphaned = Set(true);
    active.update(db).await.unwrap();
    // ...the panel shows and edits only active grants...
    let admin = app.acting_as(&users.admin).await;
    let shown = admin.get(&api(&app, &format!("roles/{}", role.id))).await.json()["permissions"].clone();
    assert_eq!(shown, json!(["posts.view"]));
    admin
        .send(
            axum::http::Method::PUT,
            &api(&app, &format!("roles/{}/permissions", role.id)),
            Some(&json!({"permissions": ["posts.view", "posts.view_any"], "expectedPermissions": ["posts.view"]})),
        )
        .await
        .assert_status(StatusCode::OK);
    // ...and redeclaring the permission brings the grant back.
    let mut active = orphan.into_active_model();
    active.orphaned = Set(false);
    active.update(db).await.unwrap();
    let shown = admin.get(&api(&app, &format!("roles/{}", role.id))).await.json()["permissions"].clone();
    assert_eq!(shown, json!(["posts.update", "posts.view", "posts.view_any"]));
});

db_test!(changes_by_a_revoked_caller_are_refused, |db: TestDb| async move {
    let (app, users): (TestApp, Users) = setup(db).await;
    let (auth, db) = rbac(&app);
    let service = RbacService::new(db, &auth.resolver);
    // Authenticated as the admin...
    let caller = auth.resolver.principal(db, ADMIN_GUARD, &users.admin, None).await.unwrap();
    // ...whose sessions are revoked while the request is still running (e.g. hashing).
    rustcms_auth::credentials::logout_everywhere(db, &users.admin, &rustcms_core::audit::AuditContext::system(None))
        .await
        .unwrap();
    let hash = rustcms_auth::password::hash_blocking("chosen by an attacker").unwrap();
    let refused = service.reset_password(Actor::User(&caller), users.member.id, hash).await.unwrap_err();
    assert_eq!(refused.status(), StatusCode::FORBIDDEN);
    assert!(service.set_user_active(Actor::User(&caller), users.member.id, false).await.is_err());
    // A fresh principal works again.
    let fresh = auth
        .resolver
        .principal(db, ADMIN_GUARD, &user::Entity::find_by_id(users.admin.id).one(db).await.unwrap().unwrap(), None)
        .await
        .unwrap();
    service.set_user_active(Actor::User(&fresh), users.member.id, false).await.unwrap();
});

db_test!(stale_profile_drafts_never_restore_an_old_email, |db: TestDb| async move {
    let (app, users): (TestApp, Users) = setup(db).await;
    let admin = app.acting_as(&users.admin).await;
    let path = api(&app, &format!("users/{}", users.member.id));
    let original = json!({"name": "Member", "email": users.member.email});
    // B changes the email...
    admin
        .send(
            axum::http::Method::PATCH,
            &path,
            Some(&json!({"name": "Member", "email": "new@example.test", "expected": original})),
        )
        .await
        .assert_status(StatusCode::OK);
    // ...A saves a rename from the old draft: refused.
    let stale = admin
        .send(
            axum::http::Method::PATCH,
            &path,
            Some(&json!({"name": "Renamed", "email": users.member.email, "expected": original})),
        )
        .await;
    stale.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(admin.get(&path).await.json()["email"], json!("new@example.test"));
});

db_test!(changing_ones_own_email_needs_the_current_password, |db: TestDb| async move {
    let (app, _users): (TestApp, Users) = setup(db).await;
    let me = UserFactory::new()
        .name("Self")
        .email("self@example.test")
        .password("my own password")
        .create(app.state().db())
        .await;
    let browser = app.login_as("self@example.test", "my own password").await;
    let path = api(&app, &format!("users/{}", me.id));
    let expected = json!({"name": "Self", "email": "self@example.test"});
    let patch = |body: serde_json::Value| {
        let (browser, path) = (&browser, path.clone());
        async move { browser.send(axum::http::Method::PATCH, &path, Some(&body)).await }
    };
    // Renaming oneself needs nothing more.
    patch(json!({"name": "Self Renamed", "email": "self@example.test", "expected": expected}))
        .await
        .assert_status(StatusCode::OK);
    let expected = json!({"name": "Self Renamed", "email": "self@example.test"});
    // The sign-in email does: without or with a wrong password, nothing changes.
    patch(json!({"name": "Self Renamed", "email": "thief@example.test", "expected": expected}))
        .await
        .assert_validation_error("current_password");
    patch(json!({"name": "Self Renamed", "email": "thief@example.test", "expected": expected, "currentPassword": "guess"}))
        .await
        .assert_validation_error("current_password");
    patch(json!({"name": "Self Renamed", "email": "mine@example.test", "expected": expected, "currentPassword": "my own password"}))
        .await
        .assert_status(StatusCode::OK);
    let stored = user::Entity::find_by_id(me.id).one(app.state().db()).await.unwrap().unwrap();
    assert_eq!(stored.email, "mine@example.test");
});

db_test!(own_email_changes_are_checked_against_the_stored_email, |db: TestDb| async move {
    let (app, users): (TestApp, Users) = setup(db).await;
    let (auth, db) = rbac(&app);
    let service = RbacService::new(db, &auth.resolver);
    let me = auth.resolver.principal(db, ADMIN_GUARD, &users.member, None).await.unwrap();
    // Whatever the request believed the email was, an unconfirmed change of the
    // stored one is refused.
    let refused = service
        .update_profile_if(Actor::User(&me), users.member.id, "Member", "other@example.test", None, false)
        .await
        .unwrap_err();
    assert_eq!(refused.status(), StatusCode::UNPROCESSABLE_ENTITY);
    service
        .update_profile_if(Actor::User(&me), users.member.id, "Renamed", &users.member.email, None, false)
        .await
        .unwrap();
});

db_test!(a_new_email_is_not_verified, |db: TestDb| async move {
    use sea_orm::{ActiveModelTrait, IntoActiveModel, Set};
    let (app, users): (TestApp, Users) = setup(db).await;
    let (auth, db) = rbac(&app);
    let mut verified = users.member.clone().into_active_model();
    verified.email_verified_at = Set(Some(chrono::Utc::now()));
    verified.update(db).await.unwrap();
    let service = RbacService::new(db, &auth.resolver);
    service.update_profile(Actor::System, users.member.id, "Renamed", &users.member.email).await.unwrap();
    let kept = user::Entity::find_by_id(users.member.id).one(db).await.unwrap().unwrap();
    assert!(kept.email_verified_at.is_some(), "a rename keeps the verification");
    service.update_profile(Actor::System, users.member.id, "Renamed", "elsewhere@example.test").await.unwrap();
    let changed = user::Entity::find_by_id(users.member.id).one(db).await.unwrap().unwrap();
    assert!(changed.email_verified_at.is_none(), "a new address is unverified");
});

db_test!(unauthorized_role_updates_reveal_nothing, |db: TestDb| async move {
    let (app, users): (TestApp, Users) = setup(db).await;
    let editor = app.acting_as(&users.editor).await;
    let super_role = support::role_id(&app, "super_admin").await;
    let mut answers = Vec::new();
    for (id, expected) in [
        (users.admin.id.to_string(), json!([super_role])),
        (users.admin.id.to_string(), json!([])),
        ("999999".to_owned(), json!([])),
    ] {
        let response = editor
            .send(
                axum::http::Method::PUT,
                &api(&app, &format!("users/{id}/roles")),
                Some(&json!({"roleIds": [], "expectedRoleIds": expected})),
            )
            .await;
        answers.push((response.status, response.json()["code"].clone()));
    }
    assert!(answers.iter().all(|a| *a == (StatusCode::FORBIDDEN, json!("forbidden"))), "{answers:?}");
});

db_test!(password_hashing_for_others_is_authorized_and_throttled_first, |db: TestDb| async move {
    let (app, users): (TestApp, Users) = setup(db).await;
    let auth = app.state().service::<AuthState>().unwrap();
    let limit = auth.config.attempts_per_identifier;
    let manager = with_role(&app, "hasher", &["users.view_any", "users.view", "users.update"]).await;
    let client = app.acting_as(&manager).await;
    let path = api(&app, &format!("users/{}/password", users.member.id));
    // Refused before hashing, so never throttled either: the limit is not consumed.
    for _ in 0..=limit {
        client
            .send(axum::http::Method::PUT, &path, Some(&json!({"password": "attacker password"})))
            .await
            .assert_status(StatusCode::FORBIDDEN);
    }
    // A super admin is throttled per actor.
    let admin = app.acting_as(&users.admin).await;
    for _ in 0..limit {
        admin
            .send(axum::http::Method::PUT, &path, Some(&json!({"password": "a fine new password"})))
            .await
            .assert_status(StatusCode::NO_CONTENT);
    }
    admin
        .send(axum::http::Method::PUT, &path, Some(&json!({"password": "a fine new password"})))
        .await
        .assert_status(StatusCode::TOO_MANY_REQUESTS);
});

db_test!(write_responses_never_show_what_the_caller_may_not_read, |db: TestDb| async move {
    let (app, users): (TestApp, Users) = setup(db).await;
    let assigner = with_role(&app, "assigner", &["roles.assign"]).await;
    let client = app.acting_as(&assigner).await;
    let member = users.member.id;
    client.get(&api(&app, &format!("users/{member}"))).await.assert_status(StatusCode::FORBIDDEN);
    let noop = client
        .send(
            axum::http::Method::PUT,
            &api(&app, &format!("users/{member}/roles")),
            Some(&json!({"roleIds": [], "expectedRoleIds": []})),
        )
        .await;
    // Comparing with the expected roles would reveal them: refused without users.view.
    noop.assert_status(StatusCode::FORBIDDEN);
    let viewer = with_role(&app, "assign-viewer", &["roles.assign", "users.view"]).await;
    let ok = app
        .acting_as(&viewer)
        .await
        .send(
            axum::http::Method::PUT,
            &api(&app, &format!("users/{member}/roles")),
            Some(&json!({"roleIds": [], "expectedRoleIds": []})),
        )
        .await;
    ok.assert_status(StatusCode::OK);
    // Roles are listed by name for assigning, without their contents.
    let roles = client.get(&api(&app, "roles")).await.json();
    let editor = roles.as_array().unwrap().iter().find(|r| r["name"] == json!("editor")).unwrap().clone();
    assert_eq!(editor["permissions"], json!([]));
    assert_eq!(editor["users"], json!(0));
    // Creating without users.view: created, but nothing shown.
    let creator = with_role(&app, "creator", &["users.create"]).await;
    let created = app
        .acting_as(&creator)
        .await
        .post_json(
            &api(&app, "users"),
            &json!({"name": "N", "email": "n@example.test", "password": "long enough password"}),
        )
        .await;
    created.assert_status(StatusCode::NO_CONTENT);
});

db_test!(batch_rules_for_protected_accounts_and_credentials, |db: TestDb| async move {
    use axum::http::Method;
    let (app, users): (TestApp, Users) = setup(db).await;
    let (auth, db) = rbac(&app);
    let service = RbacService::new(db, &auth.resolver);
    // A user holding a protected (privileged) role.
    let vip = UserFactory::new().name("Vip").create(db).await;
    let role = service.create_role(Actor::System, ADMIN_GUARD, "vip", &[]).await.unwrap();
    {
        use sea_orm::{ActiveModelTrait, IntoActiveModel, Set};
        let mut active = role.clone().into_active_model();
        active.is_privileged = Set(true);
        active.update(db).await.unwrap();
    }
    service.assign_role(Actor::System, vip.id, role.id).await.unwrap();
    let manager =
        with_role(&app, "manager", &["users.view_any", "users.view", "users.update", "users.delete", "roles.assign"])
            .await;
    let manager = app.acting_as(&manager).await;
    let vip_path = api(&app, &format!("users/{}", vip.id));
    manager
        .send(Method::PUT, &format!("{vip_path}/active"), Some(&json!({"active": false, "expected": true})))
        .await
        .assert_status(StatusCode::FORBIDDEN);
    manager.send(Method::DELETE, &vip_path, None).await.assert_status(StatusCode::FORBIDDEN);
    manager
        .send(
            Method::PATCH,
            &vip_path,
            Some(&json!({"name": "x", "email": vip.email, "expected": {"name": "Vip", "email": vip.email}})),
        )
        .await
        .assert_status(StatusCode::FORBIDDEN);
    // The listing says so.
    let row = manager.get(&vip_path).await.json();
    assert_eq!(row["abilities"]["activate"], json!(false));
    assert_eq!(row["abilities"]["changeEmail"], json!(false));

    // A stale status is refused.
    manager
        .send(
            Method::PUT,
            &api(&app, &format!("users/{}/active", users.member.id)),
            Some(&json!({"active": false, "expected": false})),
        )
        .await
        .assert_status(StatusCode::UNPROCESSABLE_ENTITY);

    // Someone else's email change ends their sessions.
    let target = UserFactory::new().email("moving@example.test").password("target password").create(db).await;
    let browser = app.login_as("moving@example.test", "target password").await;
    let admin = app.acting_as(&users.admin).await;
    admin
        .send(
            Method::PATCH,
            &api(&app, &format!("users/{}", target.id)),
            Some(&json!({"name": target.name, "email": "moved@example.test", "expected": {"name": target.name, "email": "moving@example.test"}})),
        )
        .await
        .assert_status(StatusCode::OK);
    browser.get(&app.admin_api("auth/me")).await.assert_status(StatusCode::UNAUTHORIZED);

    // Your own email change keeps this session and ends the others.
    let me = UserFactory::new().name("Me").email("me@example.test").password("my password").create(db).await;
    let here = app.login_as("me@example.test", "my password").await;
    let elsewhere = app.login_as("me@example.test", "my password").await;
    here.send(
        Method::PATCH,
        &api(&app, &format!("users/{}", me.id)),
        Some(&json!({"name": "Me", "email": "me2@example.test", "expected": {"name": "Me", "email": "me@example.test"}, "currentPassword": "my password"})),
    )
    .await
    .assert_status(StatusCode::OK);
    here.get(&app.admin_api("auth/me")).await.assert_status(StatusCode::OK);
    elsewhere.get(&app.admin_api("auth/me")).await.assert_status(StatusCode::UNAUTHORIZED);
});

db_test!(batch_rules_for_roles_and_input, |db: TestDb| async move {
    use axum::http::Method;
    use sea_orm::{ActiveModelTrait, IntoActiveModel, Set};
    let (app, users): (TestApp, Users) = setup(db).await;
    let (auth, db) = rbac(&app);
    let service = RbacService::new(db, &auth.resolver);
    // A role carrying an orphaned (dormant) grant is assigned only by super admins:
    // a redeclaration would otherwise hand out a permission nobody checked.
    let role = service
        .create_role(Actor::System, ADMIN_GUARD, "legacy", &["posts.view".into(), "posts.update".into()])
        .await
        .unwrap();
    let orphan = rustcms_auth::entities::permission::Entity::find()
        .filter(rustcms_auth::entities::permission::Column::Name.eq("posts.update"))
        .one(db)
        .await
        .unwrap()
        .unwrap();
    let mut active = orphan.into_active_model();
    active.orphaned = Set(true);
    active.update(db).await.unwrap();
    let assigner = with_role(&app, "assigner2", &["roles.assign", "users.view", "posts.view"]).await;
    let assigner = auth.resolver.principal(db, ADMIN_GUARD, &assigner, None).await.unwrap();
    let refused = service.assign_role(Actor::User(&assigner), users.member.id, role.id).await.unwrap_err();
    assert_eq!(refused.status(), StatusCode::FORBIDDEN);
    let admin_principal = auth.resolver.principal(db, ADMIN_GUARD, &users.admin, None).await.unwrap();
    service.assign_role(Actor::User(&admin_principal), users.member.id, role.id).await.unwrap();

    let admin = app.acting_as(&users.admin).await;
    // Role names: case-insensitive unique, no control characters, bounded.
    for name in ["Legacy", "bad\nname", &"x".repeat(121)] {
        admin
            .post_json(&api(&app, "roles"), &json!({"name": name, "permissions": []}))
            .await
            .assert_validation_error("name");
    }
    // Emails are real addresses.
    for email in ["a@", "@b.c", "a b@c.d", "a@@b.c", "a@b"] {
        admin
            .post_json(&api(&app, "users"), &json!({"name": "N", "email": email, "password": "long enough password"}))
            .await
            .assert_validation_error("email");
    }
    // Typos cost no hashing budget: many refused creations, then creations still work.
    for _ in 0..10 {
        admin
            .post_json(
                &api(&app, "users"),
                &json!({"name": "Dup", "email": users.member.email, "password": "long enough password"}),
            )
            .await
            .assert_validation_error("email");
    }
    for i in 0..8 {
        admin
            .post_json(
                &api(&app, "users"),
                &json!({"name": "Ok", "email": format!("ok{i}@example.test"), "password": "long enough password"}),
            )
            .await
            .assert_status(StatusCode::CREATED);
    }
    // You may not edit a role you hold: the hint agrees with the server.
    let lead = with_role(&app, "lead2", &["roles.view_any", "roles.view", "roles.update"]).await;
    let lead_client = app.acting_as(&lead).await;
    let own = support::role_id(&app, "lead2").await;
    let shown = lead_client.get(&api(&app, &format!("roles/{own}"))).await.json();
    assert_eq!(shown["abilities"]["update"], json!(false));
    // Comparing with expected permissions needs to be able to read the role.
    let blind = with_role(&app, "blind", &["roles.update", "posts.view"]).await;
    let readers = service.create_role(Actor::System, ADMIN_GUARD, "readers2", &["posts.view".into()]).await.unwrap();
    app.acting_as(&blind)
        .await
        .send(
            Method::PUT,
            &api(&app, &format!("roles/{}/permissions", readers.id)),
            Some(&json!({"permissions": ["posts.view"], "expectedPermissions": ["posts.view"]})),
        )
        .await
        .assert_status(StatusCode::FORBIDDEN);
});

db_test!(verification_round_rules, |db: TestDb| async move {
    use axum::http::Method;
    let (app, users): (TestApp, Users) = setup(db).await;
    // An update-only caller learns nothing about ids from password resets.
    let updater = with_role(&app, "updater", &["users.update"]).await;
    let client = app.acting_as(&updater).await;
    for id in [users.member.id.to_string(), "999999".into()] {
        let r = client
            .send(Method::PUT, &api(&app, &format!("users/{id}/password")), Some(&json!({"password": "whatever long"})))
            .await;
        r.assert_status(StatusCode::FORBIDDEN);
        // Nor about status without read access.
        let r = client
            .send(
                Method::PUT,
                &api(&app, &format!("users/{id}/active")),
                Some(&json!({"active": true, "expected": false})),
            )
            .await;
        r.assert_status(StatusCode::FORBIDDEN);
    }
});

db_test!(nobody_deletes_a_role_they_hold, |db: TestDb| async move {
    let (app, _users): (TestApp, Users) = setup(db).await;
    let holder = with_role(&app, "self-deleter", &["roles.view_any", "roles.view", "roles.delete"]).await;
    let client = app.acting_as(&holder).await;
    let own = support::role_id(&app, "self-deleter").await;
    let shown = client.get(&api(&app, &format!("roles/{own}"))).await.json();
    assert_eq!(shown["abilities"]["delete"], json!(false));
    client
        .send(axum::http::Method::DELETE, &api(&app, &format!("roles/{own}")), None)
        .await
        .assert_status(StatusCode::FORBIDDEN);
});
