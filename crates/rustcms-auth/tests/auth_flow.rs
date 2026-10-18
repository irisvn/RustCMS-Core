//! Real authentication flows: sessions, CSRF, tokens, throttling, revocation.

mod support;

use std::net::SocketAddr;

use axum::http::{Method, StatusCode};
use chrono::{Duration, Utc};
use rustcms_auth::entities::session;
use rustcms_auth::{ADMIN_GUARD, Actor, RbacService};
use rustcms_migration::Migrator;
use rustcms_testing::{TestDb, db_test};
use sea_orm::sea_query::Expr;
use sea_orm::{EntityTrait, QueryFilter};
use serde_json::json;
use support::{PASSWORD, auth, reload, setup};

const COOKIE: &str = "rustcms_session";

db_test!(login_requires_csrf_and_rotates_the_session, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup(db).await;
    let client = app.client();
    let login = app.admin_api("auth/login");
    let credentials = json!({ "email": seed.member.email, "password": PASSWORD });

    client.post_json(&login, &credentials).await.assert_status(StatusCode::FORBIDDEN);

    client.fetch_csrf().await;
    let before = client.cookie(COOKIE).expect("session cookie after csrf");
    let response = client.post_json(&login, &credentials).await;
    response.assert_status(StatusCode::OK).assert_json("/user/email", json!(seed.member.email));
    let set_cookie = response.headers["set-cookie"].to_str().unwrap();
    assert!(
        set_cookie.contains("HttpOnly") && set_cookie.contains("SameSite=Lax") && set_cookie.contains("Path=/admin")
    );
    assert!(!set_cookie.contains("Max-Age"), "expiry is enforced by the server: {set_cookie}");
    let after = client.cookie(COOKIE).unwrap();
    assert_ne!(before, after, "session id regenerated on login (fixation)");

    client.get(&app.admin_api("auth/me")).await.assert_status(StatusCode::OK).assert_json("/guard", json!("admin"));

    // The pre-login session id is dead.
    let stale = app.client();
    stale.request(cookie_request(&app.admin_api("auth/me"), &before)).await.assert_status(StatusCode::UNAUTHORIZED);

    client.fetch_csrf().await;
    client.post_json(&app.admin_api("auth/logout"), &json!({})).await.assert_status(StatusCode::NO_CONTENT);
    assert_eq!(client.cookie(COOKIE), None, "cookie cleared");
    let replay = app.client();
    replay.request(cookie_request(&app.admin_api("auth/me"), &after)).await.assert_status(StatusCode::UNAUTHORIZED);
});

fn cookie_request(uri: &str, token: &str) -> axum::http::Request<axum::body::Body> {
    axum::http::Request::get(uri).header("cookie", format!("{COOKIE}={token}")).body(axum::body::Body::empty()).unwrap()
}

db_test!(rotating_the_app_key_signs_everyone_out, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup(db.clone()).await;
    let client = app.acting_as(&seed.member).await;
    let me = app.admin_api("auth/me");
    client.get(&me).await.assert_status(StatusCode::OK);
    let token = client.cookie(COOKIE).expect("session cookie");

    // Reverse check: another instance with the same key accepts the cookie.
    let mut same = rustcms_testing::test_config(db.url());
    same.app.key = app.state().config().app.key.clone();
    let same = rustcms_testing::TestApp::with_config(support::build, same, db.clone()).await;
    same.request(cookie_request(&me, &token)).await.assert_status(StatusCode::OK);

    // After `key:generate --force` (a new random key), the cookie is dead.
    let rotated = rustcms_testing::TestApp::new(support::build, db.clone()).await;
    rotated.request(cookie_request(&me, &token)).await.assert_status(StatusCode::UNAUTHORIZED);
    // Also with the old key kept in `app.previous_keys`: they only open encrypted values.
    let mut keeping = rustcms_testing::test_config(db.url());
    keeping.app.previous_keys = vec![app.state().config().app.key.clone()];
    let keeping = rustcms_testing::TestApp::with_config(support::build, keeping, db).await;
    keeping.request(cookie_request(&me, &token)).await.assert_status(StatusCode::UNAUTHORIZED);
});

db_test!(state_changes_need_allowed_origin_and_csrf, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup(db).await;
    let ping = "/admin/api/ping";
    let client = app.acting_as(&seed.member).await;
    client.post_json(ping, &json!({})).await.assert_status(StatusCode::OK);

    let foreign = app.acting_as(&seed.member).await.origin(Some("https://evil.example"));
    foreign
        .post_json(ping, &json!({}))
        .await
        .assert_status(StatusCode::FORBIDDEN)
        .assert_json("/code", json!("forbidden"));

    let no_origin = app.acting_as(&seed.member).await.origin(None);
    no_origin.post_json(ping, &json!({})).await.assert_status(StatusCode::FORBIDDEN);
    let with_referer = axum::http::Request::post(ping)
        .header("referer", "http://localhost:8000/admin/posts")
        .body(axum::body::Body::empty())
        .unwrap();
    no_origin.request(with_referer).await.assert_status(StatusCode::OK);

    // A bad token has its own code, so clients refresh it instead of reporting a permission problem.
    client.set_csrf(Some("forged"));
    client
        .post_json(ping, &json!({}))
        .await
        .assert_status(StatusCode::FORBIDDEN)
        .assert_json("/code", json!("csrf_mismatch"));
    client.set_csrf(None);
    client
        .post_json(ping, &json!({}))
        .await
        .assert_status(StatusCode::FORBIDDEN)
        .assert_json("/code", json!("csrf_mismatch"));
    client.get(&app.admin_api("auth/me")).await.assert_status(StatusCode::OK);
});

db_test!(bad_credentials_look_the_same, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup(db).await;
    let client = app.client();
    client.fetch_csrf().await;
    let login = app.admin_api("auth/login");
    let message = |r: &rustcms_testing::TestResponse| r.json()["errors"]["email"][0].clone();

    let wrong = client.post_json(&login, &json!({ "email": seed.member.email, "password": "nope" })).await;
    let unknown = client.post_json(&login, &json!({ "email": "ghost@example.test", "password": PASSWORD })).await;
    wrong.assert_validation_error("email");
    unknown.assert_validation_error("email");
    assert_eq!(message(&wrong), message(&unknown));

    let auth = auth(&app);
    let actor = support::principal(&app, ADMIN_GUARD, &seed.admin).await;
    RbacService::new(app.state().db(), &auth.resolver)
        .set_user_active(Actor::User(&actor), seed.member.id, false)
        .await
        .unwrap();
    let disabled = client.post_json(&login, &json!({ "email": seed.member.email, "password": PASSWORD })).await;
    disabled.assert_validation_error("email");
    assert_eq!(message(&disabled), message(&wrong));
});

db_test!(sign_in_is_throttled_by_client_ip_not_headers, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup(db).await;
    let peer: SocketAddr = "203.0.113.50:4000".parse().unwrap();
    let client = app.client().from(peer);
    client.fetch_csrf().await;
    let login = app.admin_api("auth/login");
    let bad = json!({ "email": seed.member.email, "password": "nope" });
    for _ in 0..5 {
        client.post_json(&login, &bad).await.assert_validation_error("email");
    }
    let limited = client.post_json(&login, &json!({ "email": seed.member.email, "password": PASSWORD })).await;
    limited.assert_status(StatusCode::TOO_MANY_REQUESTS);
    assert!(limited.headers.contains_key("retry-after"));

    // A spoofed X-Forwarded-For from an untrusted peer does not reset the counter.
    let spoofed = axum::http::Request::post(&login)
        .header("content-type", "application/json")
        .header("x-forwarded-for", "198.51.100.1")
        .body(axum::body::Body::from(bad.to_string()))
        .unwrap();
    client.request(spoofed).await.assert_status(StatusCode::TOO_MANY_REQUESTS);

    // The API token endpoint shares the same counters.
    let token = json!({ "email": seed.member.email, "password": PASSWORD, "token_name": "cli" });
    app.client().from(peer).post_json("/api/v1/auth/token", &token).await.assert_status(StatusCode::TOO_MANY_REQUESTS);
});

db_test!(api_tokens_are_scoped_and_strict, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup(db).await;
    let request =
        json!({ "email": seed.reader.email, "password": PASSWORD, "token_name": "cli", "abilities": ["posts.view"] });
    let issued = app.client().post_json("/api/v1/auth/token", &request).await;
    issued.assert_status(StatusCode::CREATED).assert_json("/abilities", json!(["posts.view"]));
    let token = issued.json()["token"].as_str().unwrap().to_owned();
    assert!(token.starts_with("rcms_"));

    let client = app.client().bearer(&token);
    client.get("/api/v1/posts").await.assert_status(StatusCode::OK);
    client.post_json("/api/v1/posts", &json!({})).await.assert_status(StatusCode::FORBIDDEN);
    client
        .get("/api/v1/auth/me")
        .await
        .assert_json("/guard", json!("api"))
        .assert_json("/abilities", json!(["posts.view"]));

    // A super admin's narrow token stays narrow.
    let narrow = app.with_token(&seed.admin, &["posts.view"]).await;
    narrow.get("/api/v1/posts").await.assert_status(StatusCode::OK);
    narrow.post_json("/api/v1/posts", &json!({})).await.assert_status(StatusCode::FORBIDDEN);
    app.with_token(&seed.admin, &["*"])
        .await
        .post_json("/api/v1/posts", &json!({}))
        .await
        .assert_status(StatusCode::CREATED);

    // Admin-guard permissions do not leak into the api guard.
    app.with_token(&seed.editor, &["*"]).await.get("/api/v1/posts").await.assert_status(StatusCode::FORBIDDEN);

    for bad in ["rcms_1_short", "Bearer-less", "eyJhbGciOi.x.y", &format!("{token}x")] {
        app.client().bearer(bad).get("/api/v1/posts").await.assert_status(StatusCode::UNAUTHORIZED);
    }
    let basic = axum::http::Request::get("/api/v1/posts")
        .header("authorization", "Basic dXNlcjpwYXNz")
        .body(axum::body::Body::empty())
        .unwrap();
    app.client().request(basic).await.assert_status(StatusCode::UNAUTHORIZED);

    // An admin session cookie does not authenticate API routes.
    let session = app.acting_as(&seed.admin).await;
    session.get("/api/v1/posts").await.assert_status(StatusCode::UNAUTHORIZED);

    client.send(Method::DELETE, "/api/v1/auth/token", None).await.assert_status(StatusCode::NO_CONTENT);
    client.get("/api/v1/posts").await.assert_status(StatusCode::UNAUTHORIZED);
    narrow.get("/api/v1/posts").await.assert_status(StatusCode::OK);
});

db_test!(token_lifetimes_are_bounded, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup(db).await;
    let request = |days: u64| json!({ "email": seed.reader.email, "password": PASSWORD, "token_name": "cli", "expires_in_days": days });
    for days in [0, 3651, u64::from(u32::MAX)] {
        let response = app.client().post_json("/api/v1/auth/token", &request(days)).await;
        response.assert_status(StatusCode::UNPROCESSABLE_ENTITY).assert_validation_error("expires_in_days");
    }
    assert!(
        rustcms_auth::entities::personal_access_token::Entity::find().all(app.state().db()).await.unwrap().is_empty()
    );
    // Reverse check: the longest lifetime is accepted.
    app.client().post_json("/api/v1/auth/token", &request(3650)).await.assert_status(StatusCode::CREATED);
});

db_test!(expired_tokens_and_sessions_are_rejected, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup(db).await;
    let request =
        json!({ "email": seed.reader.email, "password": PASSWORD, "token_name": "cli", "expires_in_days": 1 });
    let token =
        app.client().post_json("/api/v1/auth/token", &request).await.json()["token"].as_str().unwrap().to_owned();
    let client = app.client().bearer(&token);
    client.get("/api/v1/posts").await.assert_status(StatusCode::OK);
    rustcms_auth::entities::personal_access_token::Entity::update_many()
        .col_expr(
            rustcms_auth::entities::personal_access_token::Column::ExpiresAt,
            Expr::value(Utc::now() - Duration::minutes(1)),
        )
        .exec(app.state().db())
        .await
        .unwrap();
    client.get("/api/v1/posts").await.assert_status(StatusCode::UNAUTHORIZED);

    let browser = app.acting_as(&seed.member).await;
    browser.get(&app.admin_api("auth/me")).await.assert_status(StatusCode::OK);
    session::Entity::update_many()
        .col_expr(session::Column::ExpiresAt, Expr::value(Utc::now() - Duration::minutes(1)))
        .filter(sea_orm::sea_query::Condition::all())
        .exec(app.state().db())
        .await
        .unwrap();
    browser.get(&app.admin_api("auth/me")).await.assert_status(StatusCode::UNAUTHORIZED);
    assert_eq!(rustcms_auth::session::prune_expired(app.state().db()).await.unwrap(), 1);
});

// Every row of the revocation table in docs/guide/src/security/revocation.md.
db_test!(credential_revocation_table, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup(db).await;
    let me = app.admin_api("auth/me");

    // Sign out everywhere: current session kept, other sessions and every token revoked.
    let current = app.login_as(&seed.member.email, PASSWORD).await;
    let other = app.acting_as(&seed.member).await;
    let token = app.with_token(&seed.member, &["*"]).await;
    current
        .post_json(&app.admin_api("auth/logout-others"), &json!({ "password": PASSWORD }))
        .await
        .assert_status(StatusCode::NO_CONTENT);
    current.get(&me).await.assert_status(StatusCode::OK);
    other.get(&me).await.assert_status(StatusCode::UNAUTHORIZED);
    token.get("/api/v1/auth/me").await.assert_status(StatusCode::UNAUTHORIZED);
    current.fetch_csrf().await;
    current.post_json("/admin/api/ping", &json!({})).await.assert_status(StatusCode::OK);

    // Password change: same, and the new password works.
    let member = reload(&app, &seed.member).await;
    let other = app.acting_as(&member).await;
    let token = app.with_token(&member, &["*"]).await;
    let change = json!({ "current_password": PASSWORD, "password": "new password 123", "password_confirmation": "new password 123" });
    current.post_json(&app.admin_api("auth/password"), &change).await.assert_status(StatusCode::NO_CONTENT);
    current.get(&me).await.assert_status(StatusCode::OK);
    other.get(&me).await.assert_status(StatusCode::UNAUTHORIZED);
    token.get("/api/v1/auth/me").await.assert_status(StatusCode::UNAUTHORIZED);
    app.login_as(&seed.member.email, "new password 123").await;

    // Wrong current password, short or unconfirmed new password.
    current.fetch_csrf().await;
    let wrong = json!({ "current_password": "nope", "password": "another one 123", "password_confirmation": "another one 123" });
    current.post_json(&app.admin_api("auth/password"), &wrong).await.assert_validation_error("current_password");
    let short = json!({ "current_password": "new password 123", "password": "short", "password_confirmation": "nope" });
    current
        .post_json(&app.admin_api("auth/password"), &short)
        .await
        .assert_validation_error("password")
        .assert_validation_error("password_confirmation");

    // Disabling: every session and token of the user.
    let member = reload(&app, &seed.member).await;
    let session = app.acting_as(&member).await;
    let token = app.with_token(&member, &["*"]).await;
    let auth = auth(&app);
    let actor = support::principal(&app, ADMIN_GUARD, &seed.admin).await;
    RbacService::new(app.state().db(), &auth.resolver)
        .set_user_active(Actor::User(&actor), member.id, false)
        .await
        .unwrap();
    session.get(&me).await.assert_status(StatusCode::UNAUTHORIZED);
    current.get(&me).await.assert_status(StatusCode::UNAUTHORIZED);
    token.get("/api/v1/auth/me").await.assert_status(StatusCode::UNAUTHORIZED);

    // Role changes apply on the next request, without signing out.
    let reader_token = app.with_token(&seed.reader, &["*"]).await;
    reader_token.get("/api/v1/posts").await.assert_status(StatusCode::OK);
    RbacService::new(app.state().db(), &auth.resolver)
        .revoke_role(Actor::System, seed.reader.id, seed.reader_role.id)
        .await
        .unwrap();
    reader_token.get("/api/v1/posts").await.assert_status(StatusCode::FORBIDDEN);
});

db_test!(revoked_cookie_can_sign_in_again, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup(db).await;
    let me = app.admin_api("auth/me");
    let login = app.admin_api("auth/login");
    let credentials = json!({ "email": seed.member.email, "password": PASSWORD });
    let revoke = || async {
        let txn = rustcms_core::db::begin_write(app.state().db()).await.unwrap();
        rustcms_auth::credentials::revoke_all(&txn, seed.member.id, true).await.unwrap();
        txn.commit().await.unwrap();
    };

    // Revoked cookie → csrf → login: the returned token is usable and the new session sticks.
    let browser = app.login_as(&seed.member.email, PASSWORD).await;
    revoke().await;
    browser.fetch_csrf().await;
    browser.post_json(&login, &credentials).await.assert_status(StatusCode::OK);
    browser.get(&me).await.assert_status(StatusCode::OK);

    // Revoked cookie → direct login with that session's CSRF token: the new session sticks.
    browser.fetch_csrf().await;
    revoke().await;
    browser.post_json(&login, &credentials).await.assert_status(StatusCode::OK);
    browser.get(&me).await.assert_status(StatusCode::OK);
});

db_test!(concurrent_bad_sign_ins_stay_within_the_limit, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup(db).await;
    let login = app.admin_api("auth/login");
    let bad = json!({ "email": seed.member.email, "password": "nope" });
    let clients: Vec<_> = futures::future::join_all((0..20).map(|_| async {
        let client = app.client();
        client.fetch_csrf().await;
        client
    }))
    .await;
    let responses = futures::future::join_all(clients.iter().map(|c| c.post_json(&login, &bad))).await;
    let guesses = responses.iter().filter(|r| r.status == StatusCode::UNPROCESSABLE_ENTITY).count();
    let limited = responses.iter().filter(|r| r.status == StatusCode::TOO_MANY_REQUESTS).count();
    assert_eq!(guesses, 5, "exactly the configured number of guesses is verified");
    assert_eq!(limited, 15);
});

// Credentials verified earlier in a request no longer count once they changed.
db_test!(stale_credential_snapshots_are_refused, migrate = Migrator, |db: TestDb| async move {
    use rustcms_auth::credentials;
    let (app, seed) = setup(db).await;
    let db = app.state().db();
    let snapshot = seed.member.clone();

    // A password change commits between verification and the operation.
    let new_hash = rustcms_auth::password::hash_blocking("changed elsewhere 1").unwrap();
    credentials::change_password(db, &snapshot, new_hash, true, &rustcms_core::audit::AuditContext::system(None))
        .await
        .unwrap();

    let stale_token = credentials::issue_token(db, &snapshot, "late", &[], None, &Default::default()).await;
    assert!(matches!(stale_token, Err(rustcms_core::AppError::Unauthenticated)), "{stale_token:?}");
    let stale_change =
        credentials::change_password(db, &snapshot, "x".into(), true, &rustcms_core::audit::AuditContext::system(None))
            .await;
    assert!(matches!(stale_change, Err(rustcms_core::AppError::Unauthenticated)));
    let stale_logout =
        credentials::logout_everywhere(db, &snapshot, &rustcms_core::audit::AuditContext::system(None)).await;
    assert!(matches!(stale_logout, Err(rustcms_core::AppError::Unauthenticated)));

    let fresh = reload(&app, &seed.member).await;
    credentials::issue_token(db, &fresh, "ok", &[], None, &Default::default()).await.expect("fresh snapshot works");

    // Disabled users too.
    let auth = auth(&app);
    RbacService::new(db, &auth.resolver).set_user_active(Actor::System, fresh.id, false).await.unwrap();
    assert!(credentials::issue_token(db, &fresh, "late", &[], None, &Default::default()).await.is_err());
});

// Password changes racing token issuance: no token issued with the old password survives.
db_test!(token_issuance_cannot_outlive_a_password_change, migrate = Migrator, |db: TestDb| async move {
    use rustcms_auth::credentials;
    let (app, seed) = setup(db).await;
    let db = app.state().db().clone();
    let snapshot = seed.member.clone();
    let mut tasks = tokio::task::JoinSet::new();
    for i in 0..10 {
        let (db, snapshot) = (db.clone(), snapshot.clone());
        tasks.spawn(async move {
            credentials::issue_token(&db, &snapshot, &format!("t{i}"), &[], None, &Default::default())
                .await
                .ok()
                .map(|t| t.plain_text)
        });
    }
    let hash = rustcms_auth::password::hash_blocking("rotated password 1").unwrap();
    let (db2, snap2) = (db.clone(), snapshot.clone());
    tasks.spawn(async move {
        credentials::change_password(&db2, &snap2, hash, true, &rustcms_core::audit::AuditContext::system(None))
            .await
            .unwrap();
        None
    });
    let mut issued = Vec::new();
    while let Some(token) = tasks.join_next().await {
        issued.extend(token.unwrap());
    }
    for token in issued {
        app.client().bearer(&token).get("/api/v1/auth/me").await.assert_status(StatusCode::UNAUTHORIZED);
    }
});

db_test!(oversized_identifiers_are_rejected_before_throttling, migrate = Migrator, |db: TestDb| async move {
    let (app, _seed) = setup(db).await;
    let auth = auth(&app);
    let before = auth.limiter.tracked_keys();
    let huge = format!("{}@example.test", "a".repeat(500_000));
    let token = json!({ "email": huge, "password": "x", "token_name": "t" });
    for _ in 0..20 {
        app.client().post_json("/api/v1/auth/token", &token).await.assert_validation_error("email");
    }
    let long_password = json!({ "email": "a@example.test", "password": "p".repeat(2_000), "token_name": "t" });
    app.client().post_json("/api/v1/auth/token", &long_password).await.assert_validation_error("password");
    assert_eq!(auth.limiter.tracked_keys(), before, "no throttling state for rejected input");
});

db_test!(a_session_ended_by_a_concurrent_request_keeps_the_response, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup(db).await;
    let browser = app.acting_as(&seed.member).await;
    // Make the session due for sliding renewal, so the response tries to write it back.
    session::Entity::update_many()
        .col_expr(session::Column::ExpiresAt, Expr::value(Utc::now() + Duration::minutes(10)))
        .exec(app.state().db())
        .await
        .unwrap();
    let response = browser.post_json("/admin/api/race", &json!({})).await;
    response.assert_status(StatusCode::CREATED);
    assert!(!response.headers.contains_key("set-cookie"), "an ended session is not revived");
    browser.get(&app.admin_api("auth/me")).await.assert_status(StatusCode::UNAUTHORIZED);
});

db_test!(anonymous_sessions_are_throttled_and_short_lived, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup(db).await;
    let csrf = app.admin_api("auth/csrf");
    let flooder: SocketAddr = "203.0.113.77:1000".parse().unwrap();
    for _ in 0..30 {
        app.client().from(flooder).get(&csrf).await.assert_status(StatusCode::OK);
    }
    app.client().from(flooder).get(&csrf).await.assert_status(StatusCode::TOO_MANY_REQUESTS);
    app.client().from("198.51.100.9:1000".parse().unwrap()).get(&csrf).await.assert_status(StatusCode::OK);

    // An existing session can always fetch its token again.
    let browser = app.client().from("198.51.100.10:1000".parse().unwrap());
    browser.fetch_csrf().await;
    for _ in 0..40 {
        browser.get(&csrf).await.assert_status(StatusCode::OK);
    }

    // Anonymous sessions expire after 15 minutes; signed-in ones after 120.
    let lifetimes = || async {
        session::Entity::find()
            .all(app.state().db())
            .await
            .unwrap()
            .iter()
            .map(|s| (s.expires_at - Utc::now()).num_minutes())
            .collect::<Vec<_>>()
    };
    assert!(lifetimes().await.iter().all(|m| (13..=15).contains(m)), "{:?}", lifetimes().await);
    app.login_as(&seed.member.email, PASSWORD).await;
    assert!(lifetimes().await.iter().any(|m| (118..=120).contains(m)));
});

db_test!(restamp_does_not_undo_a_concurrent_logout, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup(db).await;
    let browser = app.acting_as(&seed.member).await;
    let response = browser.post_json("/admin/api/race-restamp", &json!({})).await;
    response.assert_status(StatusCode::NO_CONTENT);
    assert!(response.headers["set-cookie"].to_str().unwrap().contains("Max-Age=0"), "cookie cleared");
    assert!(session::Entity::find().all(app.state().db()).await.unwrap().is_empty(), "no session re-created");
    browser.get(&app.admin_api("auth/me")).await.assert_status(StatusCode::UNAUTHORIZED);
});

db_test!(renewal_failure_keeps_the_committed_response, migrate = Migrator, |db: TestDb| async move {
    use sea_orm::ConnectionTrait;
    let (app, seed) = setup(db).await;
    let browser = app.acting_as(&seed.member).await;
    session::Entity::update_many()
        .col_expr(session::Column::ExpiresAt, Expr::value(Utc::now() + Duration::minutes(10)))
        .exec(app.state().db())
        .await
        .unwrap();
    let response = browser.post_json("/admin/api/broken-store", &json!({})).await;
    app.state().db().execute_unprepared("ALTER TABLE sessions_offline RENAME TO sessions").await.unwrap();
    response.assert_status(StatusCode::CREATED);
});

// A renewal response that arrives late must not overwrite a cookie rotated
// meanwhile (e.g. by a password change in another tab): renewals send none.
db_test!(sliding_renewal_does_not_resend_the_cookie, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup(db).await;
    let browser = app.acting_as(&seed.member).await;
    session::Entity::update_many()
        .col_expr(session::Column::ExpiresAt, Expr::value(Utc::now() + Duration::minutes(10)))
        .exec(app.state().db())
        .await
        .unwrap();
    let response = browser.get(&app.admin_api("auth/me")).await;
    response.assert_status(StatusCode::OK);
    assert!(!response.headers.contains_key("set-cookie"), "{:?}", response.headers.get("set-cookie"));
    let row = session::Entity::find().one(app.state().db()).await.unwrap().unwrap();
    assert!((row.expires_at - Utc::now()).num_minutes() >= 118, "the server-side expiry still slides");
});

db_test!(anonymous_sessions_have_a_fixed_lifetime, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup(db).await;
    let db = app.state().db().clone();
    async fn expiry(db: &sea_orm::DatabaseConnection) -> chrono::DateTime<Utc> {
        session::Entity::find().one(db).await.unwrap().unwrap().expires_at
    }
    async fn set_expiry(db: &sea_orm::DatabaseConnection, at: chrono::DateTime<Utc>) {
        session::Entity::update_many().col_expr(session::Column::ExpiresAt, Expr::value(at)).exec(db).await.unwrap();
    }
    let soon = Utc::now() + Duration::minutes(2);

    // Kept busy, an anonymous session still expires on schedule.
    let browser = app.client();
    browser.fetch_csrf().await;
    set_expiry(&db, soon).await;
    for _ in 0..5 {
        browser.get(&app.admin_api("auth/csrf")).await.assert_status(StatusCode::OK);
        browser.post_json("/admin/api/ping", &json!({})).await;
    }
    assert!((expiry(&db).await - soon).num_seconds().abs() <= 1, "anonymous expiry never extended");

    // Signed-in sessions slide.
    session::Entity::delete_many().exec(app.state().db()).await.unwrap();
    let member = app.acting_as(&seed.member).await;
    set_expiry(&db, Utc::now() + Duration::minutes(10)).await;
    member.get(&app.admin_api("auth/me")).await.assert_status(StatusCode::OK);
    assert!((expiry(&db).await - Utc::now()).num_minutes() >= 118, "signed-in expiry slides");
});

db_test!(logout_ends_a_concurrently_reissued_session, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup(db).await;
    let browser = app.acting_as(&seed.member).await;
    let response = browser.post_json("/admin/api/logout-after-reissue", &json!({})).await;
    response.assert_status(StatusCode::OK);
    let replacement = String::from_utf8(response.body.to_vec()).unwrap();
    assert!(session::Entity::find().all(app.state().db()).await.unwrap().is_empty(), "replacement ended too");
    app.client()
        .request(cookie_request(&app.admin_api("auth/me"), &replacement))
        .await
        .assert_status(StatusCode::UNAUTHORIZED);
});

// Real concurrency: sign-out racing a credential re-issue of the same session
// must never leave a live session.
db_test!(logout_and_reissue_race_leaves_no_session, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup(db).await;
    for _ in 0..10 {
        let member = reload(&app, &seed.member).await;
        let browser = app.acting_as(&member).await;
        let csrf = browser.fetch_csrf().await;
        let cookie = browser.cookie(COOKIE).unwrap();
        let request = |path: String, body: serde_json::Value| {
            axum::http::Request::post(path)
                .header("cookie", format!("{COOKIE}={cookie}"))
                .header("origin", "http://localhost:8000")
                .header("x-csrf-token", &csrf)
                .header("content-type", "application/json")
                .body(axum::body::Body::from(body.to_string()))
                .unwrap()
        };
        let logout = app.request(request(app.admin_api("auth/logout"), json!({})));
        let reissue = app.request(request(app.admin_api("auth/logout-others"), json!({ "password": PASSWORD })));
        let (logout, reissue) = tokio::join!(logout, reissue);
        logout.assert_status(StatusCode::NO_CONTENT);
        // The re-issue may win (204), or find the session already ended (401, or 403
        // when its CSRF token went with it); either way sign-out must stand.
        assert!(
            [StatusCode::NO_CONTENT, StatusCode::UNAUTHORIZED, StatusCode::FORBIDDEN].contains(&reissue.status),
            "{}",
            reissue.status
        );
        assert!(
            session::Entity::find().all(app.state().db()).await.unwrap().is_empty(),
            "sign-out won: no live session"
        );
    }
});

// Deterministic interleaving: a re-issue (A → C) is mid-transaction when sign-out
// of A starts. Sign-out must wait for it and end C as well.
db_test!(sign_out_waits_for_an_in_flight_reissue, migrate = Migrator, |db: TestDb| async move {
    use sea_orm::ActiveModelTrait;
    let (app, seed) = setup(db).await;
    let auth = auth(&app);
    let db = app.state().db().clone();
    let (a, _) =
        rustcms_auth::session::create_for_user(&auth, &db, seed.member.id, seed.member.auth_version).await.unwrap();
    let a = auth.session_id(&a);

    let reissue = rustcms_core::db::begin_write(&db).await.unwrap();
    rustcms_auth::session::lock_lineage(&reissue, &a).await.unwrap();
    session::Entity::delete_by_id(a.clone()).exec(&reissue).await.unwrap();
    session::ActiveModel {
        id: sea_orm::Set("c".repeat(64)),
        data: sea_orm::Set("{}".into()),
        expires_at: sea_orm::Set(Utc::now() + Duration::hours(1)),
        lineage: sea_orm::Set(Some(a.clone())),
    }
    .insert(&reissue)
    .await
    .unwrap();

    let (db2, a2) = (db.clone(), a.clone());
    let sign_out = tokio::spawn(async move { rustcms_auth::session::end(&db2, &a2, &a2).await });
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    reissue.commit().await.unwrap();
    sign_out.await.unwrap().unwrap();
    assert!(session::Entity::find().all(&db).await.unwrap().is_empty(), "the re-issued session is ended too");
});

db_test!(admin_api_responses_are_never_cached, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup(db).await;
    let anonymous = app.client();
    let member = app.acting_as(&seed.member).await;
    for response in [
        anonymous.get(&app.admin_api("auth/csrf")).await,
        anonymous.get(&app.admin_api("auth/me")).await,
        member.get(&app.admin_api("auth/me")).await,
        member.get(&app.admin_api("no/such/route")).await,
    ] {
        assert_eq!(response.header("cache-control").as_deref(), Some("private, no-store"));
    }
    assert_eq!(anonymous.get("/health").await.header("cache-control"), None, "only the admin API");
    // Every admin response says whom it was produced for.
    let me = member.get(&app.admin_api("auth/me")).await;
    assert_eq!(me.header("x-rustcms-user"), Some(seed.member.id.to_string()));
    assert_eq!(anonymous.get(&app.admin_api("auth/me")).await.header("x-rustcms-user"), None);
});

db_test!(a_stale_request_does_not_end_the_rotated_session, migrate = Migrator, |db: TestDb| async move {
    use sea_orm::PaginatorTrait;
    let (app, seed) = setup(db).await;
    let browser = app.login_as(&seed.member.email, PASSWORD).await;
    browser.get(&app.admin_api("auth/me")).await.assert_status(StatusCode::OK);
    let sessions = || async { session::Entity::find().count(app.state().db()).await.unwrap() };
    let before = sessions().await;
    // Credentials move on while this browser still holds the previous stamp
    // (as when a parallel request of the same tab is changing the password).
    rustcms_auth::credentials::logout_everywhere(
        app.state().db(),
        &seed.member,
        &rustcms_core::audit::AuditContext::system(None),
    )
    .await
    .unwrap();
    browser.get(&app.admin_api("auth/me")).await.assert_status(StatusCode::UNAUTHORIZED);
    // The stale request is anonymous but destroys nothing: a re-issue in flight survives.
    assert_eq!(sessions().await, before);
});
