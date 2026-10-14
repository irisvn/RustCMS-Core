//! JWT sign-ins: access tokens, rotating refresh tokens, reuse detection and
//! revocation from the next request. All databases.

mod support;

use axum::http::StatusCode;
use rustcms_auth::entities::{jwt_family, jwt_refresh_token};
use rustcms_auth::guards::jwt::{self, Claims, JwtKey};
use rustcms_auth::{API_GUARD, Actor, RbacService};
use rustcms_core::AppKey;
use rustcms_migration::Migrator;
use rustcms_testing::{TestApp, TestDb, TestResponse, db_test};
use sea_orm::{ActiveModelTrait, EntityTrait, PaginatorTrait, Set};
use serde_json::{Value, json};
use support::{PASSWORD, auth, setup};

/// Signs in `email` over HTTP; the response body.
async fn sign_in(app: &TestApp, email: &str, abilities: Option<&[&str]>) -> Value {
    let mut body = json!({"email": email, "password": PASSWORD});
    if let Some(abilities) = abilities {
        body["abilities"] = json!(abilities);
    }
    let response = app.client().post_json("/api/v1/auth/jwt", &body).await;
    response.assert_status(StatusCode::CREATED);
    response.json()
}

async fn refresh(app: &TestApp, token: &Value) -> TestResponse {
    app.client().post_json("/api/v1/auth/refresh", &json!({"refreshToken": token})).await
}

async fn me(app: &TestApp, access: &Value) -> TestResponse {
    app.client().bearer(access.as_str().unwrap()).get("/api/v1/auth/me").await
}

db_test!(sign_in_refresh_and_reuse_detection, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup(db).await;
    let first = sign_in(&app, "admin@example.test", None).await;
    assert_eq!((first["tokenType"].clone(), first["expiresIn"].clone()), (json!("Bearer"), json!(900)));
    assert_eq!(first["abilities"], json!(["*"]));
    me(&app, &first["accessToken"])
        .await
        .assert_status(StatusCode::OK)
        .assert_json("/user/email", json!("admin@example.test"));

    // Rotation: a new pair; the refresh token used is spent.
    let second = refresh(&app, &first["refreshToken"]).await;
    second.assert_status(StatusCode::OK);
    let second = second.json();
    assert_ne!(second["refreshToken"], first["refreshToken"]);
    me(&app, &second["accessToken"]).await.assert_status(StatusCode::OK);

    // A wrong secret for a real token id changes nothing.
    let id = second["refreshToken"].as_str().unwrap().split('_').nth(1).unwrap().to_owned();
    let guess = format!("rcmsr_{id}_{}", "x".repeat(40));
    refresh(&app, &json!(guess)).await.assert_status(StatusCode::UNAUTHORIZED);
    me(&app, &second["accessToken"]).await.assert_status(StatusCode::OK);

    // Replaying a spent token ends the whole sign-in, also for whoever holds the newer one.
    refresh(&app, &first["refreshToken"]).await.assert_status(StatusCode::UNAUTHORIZED);
    refresh(&app, &second["refreshToken"]).await.assert_status(StatusCode::UNAUTHORIZED);
    me(&app, &second["accessToken"]).await.assert_status(StatusCode::UNAUTHORIZED);
    // Other sign-ins of the user are not affected.
    let other = sign_in(&app, "admin@example.test", None).await;
    me(&app, &other["accessToken"]).await.assert_status(StatusCode::OK);
    let _ = seed;
});

db_test!(revocation_applies_from_the_next_request, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup(db).await;
    // Sign-out of one sign-in (with its refresh token, even after the access token expired).
    let one = sign_in(&app, "admin@example.test", None).await;
    let logout = json!({"refreshToken": one["refreshToken"]});
    app.client().post_json("/api/v1/auth/jwt/logout", &logout).await.assert_status(StatusCode::NO_CONTENT);
    app.client().post_json("/api/v1/auth/jwt/logout", &logout).await.assert_status(StatusCode::NO_CONTENT);
    me(&app, &one["accessToken"]).await.assert_status(StatusCode::UNAUTHORIZED);
    refresh(&app, &one["refreshToken"]).await.assert_status(StatusCode::UNAUTHORIZED);

    // Sign-out everywhere ends every sign-in.
    let two = sign_in(&app, "admin@example.test", None).await;
    rustcms_auth::credentials::logout_everywhere(app.state().db(), &seed.admin).await.unwrap();
    me(&app, &two["accessToken"]).await.assert_status(StatusCode::UNAUTHORIZED);
    refresh(&app, &two["refreshToken"]).await.assert_status(StatusCode::UNAUTHORIZED);

    // Disabling the user, too.
    let reader = sign_in(&app, "reader@example.test", None).await;
    me(&app, &reader["accessToken"]).await.assert_status(StatusCode::OK);
    let state = auth(&app);
    RbacService::new(app.state().db(), &state.resolver)
        .set_user_active(Actor::System, seed.reader.id, false)
        .await
        .unwrap();
    me(&app, &reader["accessToken"]).await.assert_status(StatusCode::UNAUTHORIZED);
    refresh(&app, &reader["refreshToken"]).await.assert_status(StatusCode::UNAUTHORIZED);
});

db_test!(abilities_narrow_and_tokens_are_checked, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup(db).await;
    // Abilities are fixed per sign-in and kept across refreshes.
    let narrow = sign_in(&app, "admin@example.test", Some(&["posts.view"])).await;
    assert_eq!(narrow["abilities"], json!(["posts.view"]));
    me(&app, &narrow["accessToken"]).await.assert_json("/abilities", json!(["posts.view"]));
    let next = refresh(&app, &narrow["refreshToken"]).await.json();
    assert_eq!(next["abilities"], json!(["posts.view"]));
    // An explicit empty list is refused (omit it for every permission).
    let empty = json!({"email": "admin@example.test", "password": PASSWORD, "abilities": []});
    app.client().post_json("/api/v1/auth/jwt", &empty).await.assert_validation_error("abilities");
    // Wrong credentials: the same answer as sign-in.
    let wrong = json!({"email": "admin@example.test", "password": "nope"});
    app.client().post_json("/api/v1/auth/jwt", &wrong).await.assert_validation_error("email");

    // Tokens the server did not sign, or that expired.
    let key = JwtKey::derive(&app.state().config().app.key);
    let family = next["accessToken"].as_str().unwrap().to_owned();
    let claims = jwt::decode(&key, &family, chrono::Utc::now().timestamp()).unwrap();
    let now = chrono::Utc::now().timestamp();
    let expired = jwt::encode(&key, &Claims { iat: now - 1_000, exp: now - 1, ..claims.clone() });
    me(&app, &json!(expired)).await.assert_status(StatusCode::UNAUTHORIZED);
    let foreign = jwt::encode(&JwtKey::derive(&AppKey::generate()), &claims);
    me(&app, &json!(foreign)).await.assert_status(StatusCode::UNAUTHORIZED);
    // Signed by us, but for another user than the family's.
    let other = jwt::encode(&key, &Claims { sub: seed.member.id.to_string(), ..claims.clone() });
    me(&app, &json!(other)).await.assert_status(StatusCode::UNAUTHORIZED);
    // Not canonical, or an old credential version.
    let padded = jwt::encode(&key, &Claims { sub: format!("0{}", claims.sub), ..claims.clone() });
    me(&app, &json!(padded)).await.assert_status(StatusCode::UNAUTHORIZED);
    let stale = jwt::encode(&key, &Claims { ver: claims.ver - 1, ..claims.clone() });
    me(&app, &json!(stale)).await.assert_status(StatusCode::UNAUTHORIZED);
    me(&app, &json!(jwt::encode(&key, &claims))).await.assert_status(StatusCode::OK);
    let _ = API_GUARD;
});

db_test!(concurrent_refreshes_let_one_win, migrate = Migrator, |db: TestDb| async move {
    let (app, _) = setup(db).await;
    let signed = sign_in(&app, "admin@example.test", None).await;
    let (a, b) = tokio::join!(refresh(&app, &signed["refreshToken"]), refresh(&app, &signed["refreshToken"]));
    let statuses = [a.status, b.status];
    assert!(statuses.contains(&StatusCode::OK) && statuses.contains(&StatusCode::UNAUTHORIZED), "{statuses:?}");
    // The loser was treated as a replay: the winner's tokens are dead too.
    let winner = if a.status == StatusCode::OK { a.json() } else { b.json() };
    refresh(&app, &winner["refreshToken"]).await.assert_status(StatusCode::UNAUTHORIZED);
});

db_test!(spent_tokens_are_kept_until_their_family_ends, migrate = Migrator, |db: TestDb| async move {
    let (app, _) = setup(db).await;
    let first = sign_in(&app, "admin@example.test", None).await;
    let second = refresh(&app, &first["refreshToken"]).await.json();
    // Pruning keeps live families and their spent tokens…
    rustcms_auth::guards::jwt_session::prune(app.state().db()).await.unwrap();
    assert_eq!(jwt_refresh_token::Entity::find().count(app.state().db()).await.unwrap(), 2);
    // …so a replay is still recognized.
    refresh(&app, &first["refreshToken"]).await.assert_status(StatusCode::UNAUTHORIZED);
    refresh(&app, &second["refreshToken"]).await.assert_status(StatusCode::UNAUTHORIZED);
    // Ended families go, with their tokens.
    rustcms_auth::guards::jwt_session::prune(app.state().db()).await.unwrap();
    assert_eq!(jwt_family::Entity::find().count(app.state().db()).await.unwrap(), 0);
    assert_eq!(jwt_refresh_token::Entity::find().count(app.state().db()).await.unwrap(), 0);

    // A family past its end refreshes no more.
    let third = sign_in(&app, "admin@example.test", None).await;
    let family = jwt_family::Entity::find().one(app.state().db()).await.unwrap().unwrap();
    let mut ended: jwt_family::ActiveModel = family.into();
    ended.expires_at = Set(chrono::Utc::now() - chrono::Duration::seconds(1));
    ended.update(app.state().db()).await.unwrap();
    refresh(&app, &third["refreshToken"]).await.assert_status(StatusCode::UNAUTHORIZED);
    me(&app, &third["accessToken"]).await.assert_status(StatusCode::UNAUTHORIZED);
});

db_test!(refreshes_are_limited_per_family, migrate = Migrator, |db: TestDb| async move {
    let (app, _) = setup(db).await;
    let mut current = sign_in(&app, "admin@example.test", None).await;
    for _ in 0..30 {
        let next = refresh(&app, &current["refreshToken"]).await;
        next.assert_status(StatusCode::OK);
        current = next.json();
    }
    let limited = refresh(&app, &current["refreshToken"]).await;
    limited.assert_status(StatusCode::TOO_MANY_REQUESTS);
    assert!(limited.headers.contains_key("retry-after"));
});

fn without_jwt(config: rustcms_core::Config) -> rustcms_core::AppBuilder {
    support::build(config.with("auth.jwt.enabled", false).unwrap())
}

db_test!(jwt_can_be_turned_off, migrate = Migrator, |db: TestDb| async move {
    let on = TestApp::new(support::build, db.reopen().await).await;
    let key = JwtKey::derive(&on.state().config().app.key);
    let app = TestApp::with_config(without_jwt, on.state().config().clone(), db).await;
    let body = json!({"email": "admin@example.test", "password": PASSWORD});
    app.client().post_json("/api/v1/auth/jwt", &body).await.assert_status(StatusCode::NOT_FOUND);
    let now = chrono::Utc::now().timestamp();
    let claims = Claims {
        sub: "1".into(),
        iat: now,
        exp: now + 60,
        jti: "j".into(),
        ver: 0,
        fam: "f".into(),
        scope: vec!["*".into()],
    };
    me(&app, &json!(jwt::encode(&key, &claims))).await.assert_status(StatusCode::UNAUTHORIZED);
});

db_test!(rotating_the_app_key_ends_every_sign_in, migrate = Migrator, |db: TestDb| async move {
    let (app, _) = setup(db.clone()).await;
    let signed = sign_in(&app, "admin@example.test", None).await;

    // Reverse check: another instance with the same key accepts both tokens.
    let mut same = rustcms_testing::test_config(db.url());
    same.app.key = app.state().config().app.key.clone();
    let same = TestApp::with_config(support::build, same, db.clone()).await;
    me(&same, &signed["accessToken"]).await.assert_status(StatusCode::OK);

    // A new key: neither the access token nor the refresh token works.
    let rotated = TestApp::new(support::build, db).await;
    me(&rotated, &signed["accessToken"]).await.assert_status(StatusCode::UNAUTHORIZED);
    refresh(&rotated, &signed["refreshToken"]).await.assert_status(StatusCode::UNAUTHORIZED);
    // The old key still works where it is configured (the sign-in was not revoked).
    refresh(&same, &signed["refreshToken"]).await.assert_status(StatusCode::OK);
});

db_test!(abilities_fit_in_an_access_token, migrate = Migrator, |db: TestDb| async move {
    let (app, _) = setup(db).await;
    // 12 names of 255 bytes would make a token over 4 KiB: refused before signing in.
    let long: Vec<String> = (0..12).map(|i| format!("{i:0>255}")).collect();
    let body = json!({"email": "admin@example.test", "password": PASSWORD, "abilities": long});
    app.client().post_json("/api/v1/auth/jwt", &body).await.assert_validation_error("abilities");
    // The largest accepted set gives a token the guard takes, and so does its refresh.
    let most: Vec<String> = (0..8).map(|i| format!("{i:0>255}")).collect();
    let most: Vec<&str> = most.iter().map(String::as_str).collect();
    let signed = sign_in(&app, "admin@example.test", Some(&most)).await;
    assert!(signed["accessToken"].as_str().unwrap().len() <= jwt::MAX_LEN);
    me(&app, &signed["accessToken"]).await.assert_status(StatusCode::OK);
    let next = refresh(&app, &signed["refreshToken"]).await.json();
    me(&app, &next["accessToken"]).await.assert_status(StatusCode::OK);

    // A token signed by us with an overlong scope name is refused.
    let key = JwtKey::derive(&app.state().config().app.key);
    let claims = jwt::decode(&key, next["accessToken"].as_str().unwrap(), chrono::Utc::now().timestamp()).unwrap();
    let odd = jwt::encode(&key, &Claims { scope: vec!["x".repeat(256)], ..claims.clone() });
    me(&app, &json!(odd)).await.assert_status(StatusCode::UNAUTHORIZED);
    let empty = jwt::encode(&key, &Claims { scope: vec![String::new()], ..claims });
    me(&app, &json!(empty)).await.assert_status(StatusCode::UNAUTHORIZED);
});

db_test!(a_password_change_ends_every_sign_in, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup(db).await;
    let signed = sign_in(&app, "admin@example.test", None).await;
    // Also when API tokens are kept (`revoke_tokens_on_password_change = false`).
    let hash = auth(&app).passwords.hash("another password 123".into()).await.unwrap();
    rustcms_auth::credentials::change_password(app.state().db(), &seed.admin, hash, false).await.unwrap();
    me(&app, &signed["accessToken"]).await.assert_status(StatusCode::UNAUTHORIZED);
    refresh(&app, &signed["refreshToken"]).await.assert_status(StatusCode::UNAUTHORIZED);
});

db_test!(an_unused_refresh_token_expires, migrate = Migrator, |db: TestDb| async move {
    let (app, _) = setup(db).await;
    let signed = sign_in(&app, "admin@example.test", None).await;
    let token = jwt_refresh_token::Entity::find().one(app.state().db()).await.unwrap().unwrap();
    let mut expired: jwt_refresh_token::ActiveModel = token.into();
    expired.expires_at = Set(chrono::Utc::now() - chrono::Duration::seconds(1));
    expired.update(app.state().db()).await.unwrap();
    refresh(&app, &signed["refreshToken"]).await.assert_status(StatusCode::UNAUTHORIZED);
});
