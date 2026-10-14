//! Audit log of RBAC, account and sign-in events (ADR 0014): who did what,
//! from where; never a secret. All databases.

mod support;

use axum::http::{Method, StatusCode};
use clap::ArgMatches;
use rustcms_auth::{ADMIN_GUARD, API_GUARD, Actor, NewUser, RbacService};
use rustcms_core::audit::{AuditActor, AuditContext, RequestMeta};
use rustcms_core::models::audit_log;
use rustcms_core::{AppCommand, AppState};
use rustcms_migration::Migrator;
use rustcms_testing::{Factory, TestApp, TestDb, UserFactory, db_test};
use sea_orm::{ActiveModelTrait, DatabaseConnection, EntityTrait, QueryOrder, Set};
use serde_json::json;
use support::{PASSWORD, auth, principal, setup};

async fn entries(db: &DatabaseConnection) -> Vec<audit_log::Model> {
    audit_log::Entity::find()
        .order_by_asc(audit_log::Column::At)
        .order_by_asc(audit_log::Column::Id)
        .all(db)
        .await
        .unwrap()
}

async fn events(db: &DatabaseConnection) -> Vec<String> {
    entries(db).await.into_iter().map(|e| e.event).collect()
}

/// Forgets what the seed recorded.
async fn clear(app: &TestApp) {
    audit_log::Entity::delete_many().exec(app.state().db()).await.unwrap();
}

/// No column of any entry contains one of `secrets`.
async fn assert_no_secret(db: &DatabaseConnection, secrets: &[&str]) {
    let dump = format!("{:?}", entries(db).await);
    for secret in secrets {
        assert!(!secret.is_empty() && !dump.contains(secret), "secret stored: {secret}");
    }
}

db_test!(rbac_writes_record_one_entry_each, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup(db).await;
    clear(&app).await;
    let db = app.state().db();
    let auth = auth(&app);
    let me = principal(&app, ADMIN_GUARD, &seed.admin).await;
    let request = RequestMeta { request_id: Some("rbac-1".into()), ..Default::default() };
    let rbac = RbacService::new(db, &auth.resolver).audited(AuditContext::new(AuditActor::user(&me), request));
    let actor = Actor::User(&me);
    let names = |n: &[&str]| n.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    let member = seed.member.id;

    let role = rbac.create_role(actor, ADMIN_GUARD, "writers", &names(&["posts.view"])).await.unwrap();
    rbac.set_role_permissions(actor, role.id, &names(&["posts.view"])).await.unwrap(); // no change
    rbac.set_role_permissions(actor, role.id, &names(&["posts.view", "posts.delete"])).await.unwrap();
    rbac.assign_role(actor, member, role.id).await.unwrap();
    rbac.assign_role(actor, member, role.id).await.unwrap(); // already held
    rbac.revoke_role(actor, member, role.id).await.unwrap();
    rbac.revoke_role(actor, member, role.id).await.unwrap(); // not held
    rbac.set_user_roles(actor, member, ADMIN_GUARD, &[role.id]).await.unwrap();
    rbac.set_user_roles(actor, member, ADMIN_GUARD, &[role.id]).await.unwrap(); // same
    rbac.grant_permission(actor, member, ADMIN_GUARD, "posts.view").await.unwrap();
    rbac.revoke_permission(actor, member, ADMIN_GUARD, "posts.view").await.unwrap();
    rbac.set_user_active(actor, member, false).await.unwrap();
    rbac.set_user_active(actor, member, false).await.unwrap(); // already inactive
    rbac.set_user_active(actor, member, true).await.unwrap();
    rbac.update_profile(actor, member, "Member", &seed.member.email).await.unwrap();
    rbac.update_profile(actor, member, "Member", &seed.member.email).await.unwrap(); // same
    let hash = auth.passwords.hash("reset secret 123".into()).await.unwrap();
    rbac.reset_password(actor, member, hash.clone()).await.unwrap();
    rbac.delete_role(actor, role.id).await.unwrap();
    let new = NewUser { name: "New", email: "new@example.test", password_hash: hash.clone(), super_admin_guard: None };
    let created = rbac.create_user(actor, new).await.unwrap();
    rbac.delete_user(actor, created.id).await.unwrap();

    assert_eq!(
        events(db).await,
        [
            "role.created",
            "role.permissions_changed",
            "user.roles_changed",
            "user.roles_changed",
            "user.roles_changed",
            "user.permissions_changed",
            "user.permissions_changed",
            "user.deactivated",
            "user.activated",
            "user.updated",
            "user.password_reset",
            "role.deleted",
            "user.created",
            "user.deleted",
        ]
    );
    let all = entries(db).await;
    assert!(all.iter().all(|e| e.actor_user_id == Some(seed.admin.id) && e.request_id.as_deref() == Some("rbac-1")));
    assert_eq!(
        all[0].changes,
        Some(json!({"name": [null, "writers"], "guard": [null, "admin"], "permissions": [null, ["posts.view"]]}))
    );
    assert_eq!(all[1].changes, Some(json!({"permissions": [["posts.view"], ["posts.delete", "posts.view"]]})));
    assert_eq!(all[2].meta, Some(json!({"guard": "admin"})));
    // Whole sets, before and after.
    assert_eq!(all[2].changes, Some(json!({"roles": [[], [role.id]]})));
    assert_eq!(all[3].changes, Some(json!({"roles": [[role.id], []]})));
    assert_eq!(all[5].changes, Some(json!({"permissions": [[], ["posts.view"]]})));
    assert_eq!(all[6].changes, Some(json!({"permissions": [["posts.view"], []]})));
    assert_eq!(all[9].changes, Some(json!({"name": ["member", "Member"]})));
    assert_eq!((all[10].subject_key.clone(), all[10].changes.clone()), (Some(member.to_string()), None));
    assert_eq!(all[13].changes, Some(json!({"name": ["New", null], "email": ["new@example.test", null]})));
    assert_no_secret(db, &[&hash, "reset secret 123"]).await;

    // Refused: nothing recorded.
    let editor = principal(&app, ADMIN_GUARD, &seed.editor).await;
    let before = entries(db).await.len();
    let refused = RbacService::new(db, &auth.resolver);
    refused.assign_role(Actor::User(&editor), seed.editor.id, seed.moderator_role.id).await.unwrap_err();
    refused.delete_user(Actor::User(&editor), member).await.unwrap_err();
    assert_eq!(entries(db).await.len(), before);

    // Without an explicit context, the actor still is who acted.
    refused.assign_role(Actor::User(&editor), member, seed.editor_role.id).await.unwrap();
    let last = entries(db).await.pop().unwrap();
    assert_eq!((last.actor_kind.as_str(), last.actor_user_id), ("user", Some(seed.editor.id)));
});

db_test!(entries_outlive_their_actor, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup(db).await;
    clear(&app).await;
    let db = app.state().db();
    let auth = auth(&app);
    let admin2 = principal(&app, ADMIN_GUARD, &seed.admin2).await;
    let rbac = RbacService::new(db, &auth.resolver);
    rbac.assign_role(Actor::User(&admin2), seed.member.id, seed.moderator_role.id).await.unwrap();
    let admin = principal(&app, ADMIN_GUARD, &seed.admin).await;
    rbac.delete_user(Actor::User(&admin), seed.admin2.id).await.unwrap();
    let all = entries(db).await;
    assert_eq!(all[0].actor_user_id, Some(seed.admin2.id), "kept after the actor was deleted");
    assert_eq!(all[1].event, "user.deleted");
});

db_test!(sign_ins_are_recorded_and_failures_look_alike, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup(db).await;
    let db = app.state().db();
    let inactive = UserFactory::new().email("off@example.test").password(PASSWORD).inactive().create(db).await;
    clear(&app).await;
    let login = app.admin_api("auth/login");
    let attempts = [
        ("nobody@example.test", PASSWORD),
        (seed.member.email.as_str(), "wrong password 1"),
        (inactive.email.as_str(), PASSWORD),
    ];
    for (i, (email, password)) in attempts.iter().enumerate() {
        let client = app.client().from(std::net::SocketAddr::from(([10, 0, 0, i as u8 + 1], 4000)));
        client.fetch_csrf().await;
        let response = client.post_json(&login, &json!({"email": email, "password": password})).await;
        response.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
    }
    let failures = entries(db).await;
    assert_eq!(failures.len(), 3);
    for (entry, (email, _)) in failures.iter().zip(attempts) {
        assert_eq!(
            (entry.event.as_str(), entry.subject_type.as_str(), entry.subject_key.as_deref()),
            ("auth.sign_in_failed", "session", None)
        );
        assert_eq!((entry.actor_kind.as_str(), entry.actor_user_id), ("guest", None));
        assert_eq!(entry.meta, Some(json!({"email": email, "guard": "admin"})), "one shape for every reason");
        assert!(entry.ip.as_deref().is_some_and(|ip| ip.starts_with("10.0.0.")));
    }

    // Success, then sign-out.
    clear(&app).await;
    let client = app.login_as(&seed.member.email, PASSWORD).await;
    client.fetch_csrf().await;
    client.post_json(&app.admin_api("auth/logout"), &json!({})).await.assert_status(StatusCode::NO_CONTENT);
    let all = entries(db).await;
    let shape: Vec<_> = all
        .iter()
        .map(|e| {
            (
                e.event.as_str(),
                e.subject_key.clone(),
                e.actor_user_id,
                e.credential.as_deref(),
                e.actor_guard.as_deref(),
            )
        })
        .collect();
    let member = Some(seed.member.id.to_string());
    assert_eq!(
        shape,
        [
            ("auth.signed_in", member.clone(), Some(seed.member.id), Some("session"), Some("admin")),
            ("auth.signed_out", member, Some(seed.member.id), Some("session"), Some("admin")),
        ]
    );
    assert_no_secret(db, &[PASSWORD, "wrong password 1"]).await;
    let session = client.cookie("rustcms_session").unwrap_or_default();
    if !session.is_empty() {
        assert_no_secret(db, &[&session]).await;
    }
});

db_test!(failed_sign_ins_are_capped_per_minute, migrate = Migrator, |db: TestDb| async move {
    let config = rustcms_testing::test_config(db.url()).with("audit.sign_in_failures_per_minute", 2).unwrap();
    let app = TestApp::with_config(support::build, config, db).await;
    let minute = || chrono::Utc::now().timestamp() / 60;
    let started = minute();
    for i in 0..5u8 {
        let client = app.client().from(std::net::SocketAddr::from(([10, 0, 1, i + 1], 4000)));
        client.fetch_csrf().await;
        let body = json!({"email": format!("u{i}@example.test"), "password": "nope nope nope"});
        client.post_json(&app.admin_api("auth/login"), &body).await.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
    }
    // The skipped ones are summarized when the next minute starts.
    if minute() == started {
        assert_eq!(events(app.state().db()).await, ["auth.sign_in_failed", "auth.sign_in_failed"]);
    }
});

db_test!(account_and_token_events_are_recorded, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup(db).await;
    clear(&app).await;
    let db = app.state().db();
    let client = app.acting_as(&seed.member).await;
    client.fetch_csrf().await;
    let new_password = "brand new password 9";
    let change = json!({"current_password": PASSWORD, "password": new_password, "password_confirmation": new_password});
    client.post_json(&app.admin_api("auth/password"), &change).await.assert_status(StatusCode::NO_CONTENT);
    client.fetch_csrf().await;
    client
        .post_json(&app.admin_api("auth/logout-others"), &json!({"password": new_password}))
        .await
        .assert_status(StatusCode::NO_CONTENT);

    let request =
        json!({"email": seed.reader.email, "password": PASSWORD, "token_name": "cli", "abilities": ["posts.view"]});
    let issued = app.client().post_json("/api/v1/auth/token", &request).await;
    issued.assert_status(StatusCode::CREATED);
    let token = issued.json()["token"].as_str().unwrap().to_owned();
    app.client()
        .bearer(&token)
        .send(Method::DELETE, "/api/v1/auth/token", None)
        .await
        .assert_status(StatusCode::NO_CONTENT);

    let all = entries(db).await;
    let shape: Vec<_> = all.iter().map(|e| (e.event.as_str(), e.subject_key.clone(), e.actor_user_id)).collect();
    let (member, reader) = (seed.member.id, seed.reader.id);
    assert_eq!(
        shape,
        [
            ("auth.password_changed", Some(member.to_string()), Some(member)),
            ("auth.signed_out_everywhere", Some(member.to_string()), Some(member)),
            ("token.created", Some(reader.to_string()), Some(reader)),
            ("token.revoked", Some(reader.to_string()), Some(reader)),
        ]
    );
    assert_eq!(all[0].credential.as_deref(), Some("session"));
    assert!(all[3].credential.as_deref().is_some_and(|c| c.starts_with("token:")), "{:?}", all[3].credential);
    let meta = all[2].meta.clone().unwrap();
    assert_eq!((meta["name"].clone(), meta["abilities"].clone()), (json!("cli"), json!(["posts.view"])));
    assert_no_secret(db, &[PASSWORD, new_password, &token]).await;
});

db_test!(jwt_sign_ins_and_reuse_are_recorded, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup(db).await;
    clear(&app).await;
    let db = app.state().db();
    let body = json!({"email": seed.admin.email, "password": PASSWORD});
    let first = app.client().post_json("/api/v1/auth/jwt", &body).await.json();
    let second = app.client().post_json("/api/v1/auth/refresh", &json!({"refreshToken": first["refreshToken"]})).await;
    second.assert_status(StatusCode::OK);
    let second = second.json();
    // Replaying the spent token ends the sign-in: recorded as a guest's act.
    app.client()
        .post_json("/api/v1/auth/refresh", &json!({"refreshToken": first["refreshToken"]}))
        .await
        .assert_status(StatusCode::UNAUTHORIZED);
    let other = app.client().post_json("/api/v1/auth/jwt", &body).await.json();
    let logout = json!({"refreshToken": other["refreshToken"]});
    app.client().post_json("/api/v1/auth/jwt/logout", &logout).await.assert_status(StatusCode::NO_CONTENT);
    app.client().post_json("/api/v1/auth/jwt/logout", &logout).await.assert_status(StatusCode::NO_CONTENT);

    let all = entries(db).await;
    assert_eq!(
        all.iter().map(|e| e.event.as_str()).collect::<Vec<_>>(),
        ["jwt.signed_in", "jwt.reuse_detected", "jwt.signed_in", "jwt.signed_out"],
        "a refresh is not an event; a second sign-out changes nothing"
    );
    assert!(all.iter().all(|e| e.subject_key == Some(seed.admin.id.to_string())));
    assert!(all[0].credential.as_deref().is_some_and(|c| c.starts_with("jwt:")));
    assert_eq!((all[1].actor_kind.as_str(), all[1].actor_user_id), ("guest", None));
    let family = all[0].credential.as_deref().unwrap().trim_start_matches("jwt:");
    assert_eq!(all[1].meta, Some(json!({"family": family})));
    let secrets: Vec<&str> = [&first, &second, &other]
        .iter()
        .flat_map(|t| [t["accessToken"].as_str().unwrap(), t["refreshToken"].as_str().unwrap()])
        .collect();
    let mut secrets = secrets;
    secrets.push(PASSWORD);
    assert_no_secret(db, &secrets).await;
});

fn matches(command: &dyn AppCommand, args: &[&str]) -> ArgMatches {
    command.definition().try_get_matches_from(args).unwrap()
}

async fn run(command: &dyn AppCommand, state: &AppState, args: &[&str]) {
    command.run(state, &matches(command, args)).await.unwrap();
}

db_test!(console_writes_are_recorded_as_the_system, migrate = Migrator, |db: TestDb| async move {
    use rustcms_auth::commands::{MakeUser, RoleAssign, TokenCreate};
    let (app, seed) = setup(db).await;
    clear(&app).await;
    let state = app.state();
    run(
        &MakeUser,
        state,
        &["make:user", "--email", "ops@example.test", "--name", "Ops", "--password", "console pass 1"],
    )
    .await;
    run(&RoleAssign, state, &["role:assign", "--email", "ops@example.test", "--role", "reader", "--guard", API_GUARD])
        .await;
    run(&TokenCreate, state, &["token:create", "--email", &seed.member.email, "--name", "ci"]).await;
    let all = entries(state.db()).await;
    let shape: Vec<_> = all
        .iter()
        .map(|e| (e.event.as_str(), e.actor_kind.as_str(), e.actor_user_id, e.credential.as_deref()))
        .collect();
    assert_eq!(
        shape,
        [
            ("user.created", "system", None, Some("console:make:user")),
            ("user.roles_changed", "system", None, Some("console:role:assign")),
            ("token.created", "system", None, Some("console:token:create")),
        ]
    );
    assert_no_secret(state.db(), &["console pass 1"]).await;
});

/// An entry from `days` ago.
async fn old_entry(db: &DatabaseConnection, days: i64) {
    audit_log::ActiveModel {
        id: Set(sea_orm::prelude::Uuid::now_v7()),
        at: Set(chrono::Utc::now() - chrono::Duration::days(days)),
        event: Set("old".into()),
        subject_type: Set("test".into()),
        actor_kind: Set("system".into()),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap();
}

db_test!(retention_prunes_old_entries_only, migrate = Migrator, |db: TestDb| async move {
    let config = rustcms_testing::test_config(db.url()).with("audit.retention_days", 30).unwrap();
    let app = TestApp::with_config(support::build, config, db).await;
    clear(&app).await;
    let conn = app.state().db();
    for days in [400, 200, 31, 29, 1] {
        old_entry(conn, days).await;
    }
    run(&rustcms_auth::commands::AuditPrune, app.state(), &["audit:prune"]).await;
    assert_eq!(entries(conn).await.len(), 2, "kept the last 30 days");

    // Several runners at once delete each entry once.
    for _ in 0..30 {
        old_entry(conn, 90).await;
    }
    let cutoff = chrono::Utc::now() - chrono::Duration::days(30);
    let budget = std::time::Duration::from_secs(30);
    let (a, b) = tokio::join!(
        rustcms_core::audit::prune(conn, cutoff, budget),
        rustcms_core::audit::prune(conn, cutoff, budget)
    );
    let deleted = a.unwrap() + b.unwrap();
    assert_eq!(deleted, 30, "each entry deleted once");
    assert_eq!(entries(conn).await.len(), 2);
});

db_test!(invalid_retention_fails_at_boot, |db: TestDb| async move {
    let config = rustcms_testing::test_config(db.url()).with("audit.retention_days", 0).unwrap();
    let result = support::build(config).build_with_db(db.conn().clone()).await;
    assert!(result.err().is_some_and(|e| e.to_string().contains("retention_days")));
});

db_test!(role_changes_record_whole_sets, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup(db).await;
    clear(&app).await;
    let db = app.state().db();
    let auth = auth(&app);
    let rbac = RbacService::new(db, &auth.resolver);
    // The editor already holds `editor`: assigning `moderator` shows both.
    rbac.assign_role(Actor::System, seed.editor.id, seed.moderator_role.id).await.unwrap();
    let mut both = vec![seed.editor_role.id, seed.moderator_role.id];
    both.sort();
    assert_eq!(entries(db).await[0].changes, Some(json!({"roles": [[seed.editor_role.id], both]})));
});

db_test!(permission_sync_records_reactivations, migrate = Migrator, |db: TestDb| async move {
    let (app, _) = setup(db).await;
    let db = app.state().db();
    let auth = auth(&app);
    let rbac = RbacService::new(db, &auth.resolver);
    let mut declared = auth.permissions.all();
    let dropped = declared.keys().find(|(_, name)| name == "posts.delete").cloned().unwrap();
    let group = declared.remove(&dropped).unwrap();
    rbac.sync_permissions(&declared).await.unwrap();
    clear(&app).await;
    // Declared again: its grants apply again, which is recorded.
    declared.insert(dropped, group);
    let report = rbac.sync_permissions(&declared).await.unwrap();
    assert!(report.created.is_empty() && report.orphaned.is_empty());
    let all = entries(db).await;
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].meta, Some(json!({"created": 0, "orphaned": 0, "restored": 1, "regrouped": 0})));
    // Nothing to do: nothing recorded.
    rbac.sync_permissions(&declared).await.unwrap();
    assert_eq!(entries(db).await.len(), 1);
});

db_test!(a_failed_entry_rolls_rbac_and_token_writes_back, migrate = Migrator, |db: TestDb| async move {
    use sea_orm::{ConnectionTrait, PaginatorTrait, QueryFilter};
    let (app, seed) = setup(db).await;
    let db = app.state().db();
    let auth = auth(&app);
    db.execute_unprepared("DROP TABLE audit_logs").await.unwrap();
    let rbac = RbacService::new(db, &auth.resolver);
    rbac.assign_role(Actor::System, seed.member.id, seed.moderator_role.id).await.unwrap_err();
    let roles = rustcms_auth::entities::model_has_roles::Entity::find()
        .filter(sea_orm::ColumnTrait::eq(
            &rustcms_auth::entities::model_has_roles::Column::ModelId,
            seed.member.id.to_string(),
        ))
        .count(db)
        .await
        .unwrap();
    assert_eq!(roles, 0, "the role assignment rolled back");
    let request = json!({"email": seed.reader.email, "password": PASSWORD, "token_name": "cli"});
    app.client().post_json("/api/v1/auth/token", &request).await.assert_status(StatusCode::INTERNAL_SERVER_ERROR);
    let tokens = rustcms_auth::entities::personal_access_token::Entity::find().count(db).await.unwrap();
    assert_eq!(tokens, 0, "no token without its entry");
});

db_test!(a_reused_refresh_token_revokes_even_without_the_log, migrate = Migrator, |db: TestDb| async move {
    use sea_orm::ConnectionTrait;
    let (app, seed) = setup(db).await;
    clear(&app).await;
    let db = app.state().db();
    let body = json!({"email": seed.admin.email, "password": PASSWORD});
    let first = app.client().post_json("/api/v1/auth/jwt", &body).await.json();
    refresh(&app, &first["refreshToken"]).await.assert_status(StatusCode::OK);
    // Replays after the first one change nothing: one entry.
    refresh(&app, &first["refreshToken"]).await.assert_status(StatusCode::UNAUTHORIZED);
    refresh(&app, &first["refreshToken"]).await.assert_status(StatusCode::UNAUTHORIZED);
    assert_eq!(events(db).await, ["jwt.signed_in", "jwt.reuse_detected"]);

    // With the log unavailable, a replay still ends the sign-in.
    let other = app.client().post_json("/api/v1/auth/jwt", &body).await.json();
    let newer = refresh(&app, &other["refreshToken"]).await.json();
    db.execute_unprepared("DROP TABLE audit_logs").await.unwrap();
    refresh(&app, &other["refreshToken"]).await.assert_status(StatusCode::UNAUTHORIZED);
    refresh(&app, &newer["refreshToken"]).await.assert_status(StatusCode::UNAUTHORIZED);
    let me = app.client().bearer(newer["accessToken"].as_str().unwrap()).get("/api/v1/auth/me").await;
    me.assert_status(StatusCode::UNAUTHORIZED);
});

async fn refresh(app: &TestApp, token: &serde_json::Value) -> rustcms_testing::TestResponse {
    app.client().post_json("/api/v1/auth/refresh", &json!({"refreshToken": token})).await
}
