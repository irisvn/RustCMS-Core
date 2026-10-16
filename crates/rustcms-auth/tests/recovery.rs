//! Password resets, invitations and "remember me" (ADR 0023): the same
//! answer whether an account exists or not, tokens made and mailed only by
//! jobs, used once, dead after any credential change, and the password,
//! token and revocation in one transaction. SQLite, PostgreSQL, MySQL.

mod support;

use axum::http::StatusCode;
use rustcms_auth::entities::{role, user_token};
use rustcms_auth::{ADMIN_GUARD, Actor, RbacService};
use rustcms_core::models::user;
use rustcms_migration::Migrator;
use rustcms_testing::{TestApp, TestDb, db_test};
use sea_orm::sea_query::Expr;
use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter};
use serde_json::{Value, json};
use support::{PASSWORD, auth, reload};

const NEW_PASSWORD: &str = "a brand new passphrase";

async fn setup(db: TestDb) -> (TestApp, support::Seed) {
    support::setup_with(support::build_with_mail, db, &[]).await
}

async fn work(app: &TestApp) {
    rustcms_core::queue::run_until_idle(app.state(), None, 20).await.unwrap();
}

/// The token of the last mail sent (from its link's fragment).
fn token_in(app: &TestApp, page: &str) -> String {
    let mails = app.take_mails();
    let mail = mails.last().expect("a mail");
    let marker = format!("/admin/{page}#token=");
    let start = mail.text.find(&marker).unwrap_or_else(|| panic!("no link in {}", mail.text)) + marker.len();
    mail.text[start..].split_whitespace().next().unwrap().to_owned()
}

async fn forgot(app: &TestApp, email: &str) -> rustcms_testing::TestResponse {
    let client = app.client();
    client.fetch_csrf().await;
    client.post_json(&app.admin_api("auth/forgot-password"), &json!({ "email": email })).await
}

async fn reset(app: &TestApp, token: &str, password: &str) -> rustcms_testing::TestResponse {
    let client = app.client();
    client.fetch_csrf().await;
    let body = json!({ "token": token, "password": password, "password_confirmation": password });
    client.post_json(&app.admin_api("auth/reset-password"), &body).await
}

async fn signs_in(app: &TestApp, email: &str, password: &str) -> bool {
    let client = app.client();
    client.fetch_csrf().await;
    let body = json!({ "email": email, "password": password });
    client.post_json(&app.admin_api("auth/login"), &body).await.status == StatusCode::OK
}

/// Tokens a secret could still match (marks of credential changes excluded).
async fn live_tokens(app: &TestApp) -> u64 {
    user_token::Entity::find().filter(user_token::Column::TokenHash.ne("")).count(app.state().db()).await.unwrap()
}

async fn jobs(app: &TestApp) -> u64 {
    rustcms_core::models::job::Entity::find().count(app.state().db()).await.unwrap()
}

db_test!(a_reset_link_sets_a_new_password_once, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup(db).await;
    let signed_in = app.acting_as(&seed.member).await;
    let asked = forgot(&app, &seed.member.email.to_uppercase()).await;
    asked.assert_status(StatusCode::ACCEPTED);
    assert_eq!(asked.json(), json!({}));
    work(&app).await;
    let token = token_in(&app, "reset-password");

    // Password rules first (nothing told about the token).
    reset(&app, &token, "short").await.assert_validation_error("password");
    reset(&app, &token, NEW_PASSWORD).await.assert_status(StatusCode::NO_CONTENT);
    assert!(!signs_in(&app, &seed.member.email, PASSWORD).await);
    assert!(signs_in(&app, &seed.member.email, NEW_PASSWORD).await);
    // Every session of the user ended with it; the link works once.
    signed_in.get(&app.admin_api("auth/me")).await.assert_status(StatusCode::UNAUTHORIZED);
    reset(&app, &token, "yet another passphrase").await.assert_validation_error("token");
    assert_eq!(live_tokens(&app).await, 0, "only the marks of the change remain");
    let audited = rustcms_core::models::audit_log::Entity::find()
        .filter(rustcms_core::models::audit_log::Column::Event.eq("auth.password_reset"))
        .count(app.state().db())
        .await
        .unwrap();
    assert_eq!(audited, 1);
});

db_test!(requests_answer_alike_whoever_asks, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup(db).await;
    let disabled = &seed.reader;
    RbacService::new(app.state().db(), &auth(&app).resolver)
        .set_user_active(Actor::System, disabled.id, false)
        .await
        .unwrap();
    let mut answers = Vec::new();
    for email in [seed.member.email.as_str(), "ghost@example.test", disabled.email.as_str()] {
        let before = jobs(&app).await;
        let answer = forgot(&app, email).await;
        assert_eq!(jobs(&app).await, before + 1, "one insert whoever asks");
        answers.push((answer.status, answer.body.clone()));
    }
    assert!(answers.windows(2).all(|w| w[0] == w[1]), "{answers:?}");
    // The queue carries no secret: the token is made by the job.
    let payloads: Vec<Value> = rustcms_core::models::job::Entity::find()
        .all(app.state().db())
        .await
        .unwrap()
        .into_iter()
        .map(|j| j.payload)
        .collect();
    assert!(payloads.iter().all(|p| p.get("token").is_none() && !p.to_string().contains("#token")), "{payloads:?}");
    work(&app).await;
    let mails = app.take_mails();
    assert_eq!(mails.len(), 1, "only the active account gets a mail");
    assert_eq!(mails[0].to, std::slice::from_ref(&seed.member.email));
    // Nothing of the link in the audit log or the jobs left.
    let audit = format!("{:?}", rustcms_core::models::audit_log::Entity::find().all(app.state().db()).await.unwrap());
    assert!(!audit.contains("#token"));
    // Malformed input is refused the same for everyone (422 on the field).
    forgot(&app, "not an address").await.assert_validation_error("email");
});

db_test!(tokens_die_with_any_change, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup(db).await;
    let db = app.state().db();
    let ask = async |app: &TestApp| {
        forgot(app, &seed.member.email).await.assert_status(StatusCode::ACCEPTED);
        work(app).await;
        token_in(app, "reset-password")
    };
    // A wrong secret, a malformed token.
    let token = ask(&app).await;
    let (id, secret) = token.split_once('.').unwrap();
    let wrong = format!("{id}.{}{}", if secret.starts_with('A') { "B" } else { "A" }, &secret[1..]);
    reset(&app, &wrong, NEW_PASSWORD).await.assert_validation_error("token");
    reset(&app, "garbage", NEW_PASSWORD).await.assert_validation_error("token");
    // A newer request replaces the link.
    let newer = ask(&app).await;
    reset(&app, &token, NEW_PASSWORD).await.assert_validation_error("token");
    // Expired.
    user_token::Entity::update_many()
        .col_expr(user_token::Column::ExpiresAt, Expr::value(chrono::Utc::now() - chrono::Duration::minutes(1)))
        .exec(db)
        .await
        .unwrap();
    reset(&app, &newer, NEW_PASSWORD).await.assert_validation_error("token");
    // Another sign-in email, or a credential change: dead.
    let token = ask(&app).await;
    user::Entity::update_many()
        .col_expr(user::Column::Email, Expr::value("moved@example.test"))
        .filter(user::Column::Id.eq(seed.member.id))
        .exec(db)
        .await
        .unwrap();
    reset(&app, &token, NEW_PASSWORD).await.assert_validation_error("token");
    user::Entity::update_many()
        .col_expr(user::Column::Email, Expr::value(seed.member.email.clone()))
        .filter(user::Column::Id.eq(seed.member.id))
        .exec(db)
        .await
        .unwrap();
    let token = ask(&app).await;
    rustcms_auth::credentials::revoke_all(db, seed.member.id, false).await.unwrap();
    reset(&app, &token, NEW_PASSWORD).await.assert_validation_error("token");
    // A disabled account.
    let token = ask(&app).await;
    RbacService::new(db, &auth(&app).resolver).set_user_active(Actor::System, seed.member.id, false).await.unwrap();
    reset(&app, &token, NEW_PASSWORD).await.assert_validation_error("token");
    assert!(reload(&app, &seed.member).await.password == seed.member.password, "the password never changed");
});

db_test!(invitations_set_a_first_password, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup(db).await;
    let db = app.state().db();
    let state = auth(&app);
    let rbac = RbacService::new(db, &state.resolver);
    let invited = rbac.invite_user(Actor::System, "Invited", "Invited@Example.test", ADMIN_GUARD, &[]).await.unwrap();
    assert!(invited.invitation_pending && invited.is_active && invited.invited_at.is_some());
    // No password works meanwhile, not even the placeholder.
    assert!(!signs_in(&app, "invited@example.test", rustcms_auth::recovery::INVITED_PASSWORD).await);
    work(&app).await;
    let token = token_in(&app, "invitation");
    // An invitation is not a reset link.
    reset(&app, &token, NEW_PASSWORD).await.assert_validation_error("token");
    let client = app.client();
    client.fetch_csrf().await;
    let body =
        json!({ "token": token, "password": NEW_PASSWORD, "password_confirmation": NEW_PASSWORD, "name": "Ann" });
    client.post_json(&app.admin_api("auth/invitation"), &body).await.assert_status(StatusCode::NO_CONTENT);
    let accepted = reload(&app, &invited).await;
    assert!(!accepted.invitation_pending && accepted.email_verified_at.is_some() && accepted.is_active);
    assert_eq!(accepted.name, "Ann");
    assert!(signs_in(&app, "invited@example.test", NEW_PASSWORD).await);
    // Accepted: nothing to send again.
    let again = rbac.resend_invitation(Actor::System, invited.id).await.unwrap_err();
    assert!(matches!(again, rustcms_core::AppError::Conflict { code: "invitation_accepted", .. }), "{again:?}");
    let _ = seed;
});

db_test!(a_pending_super_admin_does_not_count, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup(db).await;
    let db = app.state().db();
    let state = auth(&app);
    let rbac = RbacService::new(db, &state.resolver);
    let invited = rbac.invite_user(Actor::System, "Boss", "boss@example.test", ADMIN_GUARD, &[]).await.unwrap();
    let super_admin = role::super_admin_id(db, ADMIN_GUARD).await.unwrap().unwrap();
    rbac.assign_role(Actor::System, invited.id, super_admin).await.unwrap();
    // The two accepted super admins: the second one still can go, the last one not.
    rbac.set_user_active(Actor::System, seed.admin2.id, false).await.unwrap();
    assert!(rbac.set_user_active(Actor::System, seed.admin.id, false).await.is_err(), "the pending one cannot sign in");
    // Set by an administrator: the invitation ends, the user signs in with that password.
    let hash = auth(&app).passwords.hash(NEW_PASSWORD.into()).await.unwrap();
    rbac.reset_password(Actor::System, invited.id, hash).await.unwrap();
    assert!(!reload(&app, &invited).await.invitation_pending);
    assert_eq!(live_tokens(&app).await, 0);
    assert!(signs_in(&app, "boss@example.test", NEW_PASSWORD).await);
});

db_test!(remember_me_keeps_the_session_longer, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup(db).await;
    let client = app.client();
    client.fetch_csrf().await;
    let body = json!({ "email": seed.member.email, "password": PASSWORD, "remember": true });
    let response = client.post_json(&app.admin_api("auth/login"), &body).await;
    response.assert_status(StatusCode::OK);
    let cookie = response.headers["set-cookie"].to_str().unwrap().to_owned();
    assert!(cookie.contains("Max-Age=2592000"), "{cookie}");
    let sessions = rustcms_auth::entities::session::Entity::find().all(app.state().db()).await.unwrap();
    let longest = sessions.iter().map(|s| s.expires_at).max().unwrap();
    assert!(longest > chrono::Utc::now() + chrono::Duration::days(29), "{longest}");
    // Still revoked at once by a credential change.
    rustcms_auth::credentials::revoke_all(app.state().db(), seed.member.id, false).await.unwrap();
    client.get(&app.admin_api("auth/me")).await.assert_status(StatusCode::UNAUTHORIZED);
});

db_test!(resets_are_off_unless_configured, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = support::setup(db).await;
    forgot(&app, &seed.member.email).await.assert_status(StatusCode::NOT_FOUND);
    reset(&app, "x", NEW_PASSWORD).await.assert_status(StatusCode::NOT_FOUND);
});

db_test!(an_older_request_never_comes_back_after_a_reset, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup(db).await;
    // Request A is made (its mail "fails": the job is kept for a retry).
    let older = rustcms_auth::recovery::PasswordResetJob {
        email: seed.member.email.clone(),
        locale: "en".into(),
        request_id: uuid::Uuid::now_v7(),
        requested_at: chrono::Utc::now() - chrono::Duration::minutes(2),
    };
    // Request B, newer, is answered and used.
    forgot(&app, &seed.member.email).await.assert_status(StatusCode::ACCEPTED);
    work(&app).await;
    let token = token_in(&app, "reset-password");
    reset(&app, &token, NEW_PASSWORD).await.assert_status(StatusCode::NO_CONTENT);
    // A's retry runs after B's reset: superseded, nothing sent.
    rustcms_core::queue::enqueue(app.state().db(), &older).await.unwrap();
    work(&app).await;
    assert!(app.take_mails().is_empty(), "no link for a request older than the reset");
    assert_eq!(live_tokens(&app).await, 0);
    // An administrator's password change supersedes earlier requests too.
    let earlier = rustcms_auth::recovery::PasswordResetJob {
        request_id: uuid::Uuid::now_v7(),
        requested_at: chrono::Utc::now(),
        ..older
    };
    let state = auth(&app);
    let hash = state.passwords.hash("set by an administrator".into()).await.unwrap();
    RbacService::new(app.state().db(), &state.resolver)
        .reset_password(Actor::System, seed.member.id, hash)
        .await
        .unwrap();
    rustcms_core::queue::enqueue(app.state().db(), &earlier).await.unwrap();
    work(&app).await;
    assert!(app.take_mails().is_empty());
});
