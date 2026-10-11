//! App, seed data and helpers shared by the auth integration tests.
#![allow(dead_code)]

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use rustcms_auth::entities::role;
use rustcms_auth::{ADMIN_GUARD, API_GUARD, Actor, Auth, AuthProvider, AuthState, RbacService, Session};
use rustcms_core::models::user;
use rustcms_core::{AppBuilder, AppResult, AppState, Config, Json, Principal};
use rustcms_testing::{Factory, TestApp, TestDb, UserFactory};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde_json::{Value, json};

pub const PASSWORD: &str = "correct horse battery";

pub fn build(config: Config) -> AppBuilder {
    let mut app = AppBuilder::new(config);
    app.provider(
        AuthProvider::new()
            .permission(API_GUARD, "posts.view", "posts")
            .permission(API_GUARD, "posts.create", "posts")
            .permission(ADMIN_GUARD, "posts.view", "posts")
            .permission(ADMIN_GUARD, "posts.delete", "posts"),
    );
    app.routes(|r| {
        r.route("/api/v1/posts", get(list_posts).post(create_post))
            .route("/admin/api/ping", post(|_: Auth| async { "pong" }))
            .route("/admin/api/race", post(ended_meanwhile))
            .route("/admin/api/race-restamp", post(restamp_after_logout))
            .route("/admin/api/broken-store", post(break_session_store))
            .route("/admin/api/logout-after-reissue", post(logout_after_reissue))
    });
    app
}

/// Does its work, then a concurrent request ends every session before this response is sent.
async fn ended_meanwhile(_: Auth, State(app): State<AppState>) -> AppResult<StatusCode> {
    rustcms_auth::entities::session::Entity::delete_many().exec(app.db()).await?;
    Ok(StatusCode::CREATED)
}

/// A concurrent request signs this session out, then this one rotates credentials.
async fn restamp_after_logout(auth: Auth, session: Session, State(app): State<AppState>) -> AppResult<StatusCode> {
    rustcms_auth::entities::session::Entity::delete_many().exec(app.db()).await?;
    session.restamp(auth.0.user.auth_version);
    Ok(StatusCode::NO_CONTENT)
}

/// Commits work, then the session table becomes unavailable before the response is written.
async fn break_session_store(_: Auth, State(app): State<AppState>) -> AppResult<StatusCode> {
    use sea_orm::ConnectionTrait;
    app.db().execute_unprepared("ALTER TABLE sessions RENAME TO sessions_offline").await?;
    Ok(StatusCode::CREATED)
}

/// Concurrent restamps re-issued this session twice (A → B → C, each deleting its
/// predecessor) after this request loaded A; then this request signs out.
/// Returns C's cookie value.
async fn logout_after_reissue(
    auth: Auth,
    session: Session,
    State(app): State<AppState>,
    headers: axum::http::HeaderMap,
) -> AppResult<String> {
    use rustcms_auth::entities::session as entity;
    use rustcms_auth::secret::sha256_hex;
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
    let cookie =
        headers["cookie"].to_str().unwrap().split("rustcms_session=").nth(1).unwrap().split(';').next().unwrap();
    let a = sha256_hex(cookie);
    let state = app.service::<AuthState>().unwrap();
    let mut previous = a.clone();
    let mut latest = String::new();
    for _ in 0..2 {
        let (token, _) =
            rustcms_auth::session::create_for_user(&state, app.db(), auth.0.user.id, auth.0.user.auth_version).await?;
        entity::Entity::update_many()
            .col_expr(entity::Column::Lineage, sea_orm::sea_query::Expr::value(a.clone()))
            .filter(entity::Column::Id.eq(sha256_hex(&token)))
            .exec(app.db())
            .await?;
        entity::Entity::delete_by_id(previous).exec(app.db()).await?;
        previous = sha256_hex(&token);
        latest = token;
    }
    session.logout();
    Ok(latest)
}

async fn list_posts(auth: Auth) -> AppResult<Json<Value>> {
    auth.authorize("posts.view")?;
    Ok(Json(json!([])))
}

async fn create_post(auth: Auth) -> AppResult<StatusCode> {
    auth.authorize("posts.create")?;
    Ok(StatusCode::CREATED)
}

/// Users and roles every test starts with.
pub struct Seed {
    /// Super admin of both guards.
    pub admin: user::Model,
    /// Second admin-guard super admin.
    pub admin2: user::Model,
    /// Admin role `editor`: posts.view, roles.assign, users.update, roles.update.
    pub editor: user::Model,
    /// No roles.
    pub member: user::Model,
    /// API role `reader`: posts.view.
    pub reader: user::Model,
    pub editor_role: role::Model,
    pub reader_role: role::Model,
    /// Admin role `moderator`: posts.view, posts.delete.
    pub moderator_role: role::Model,
}

pub async fn setup(db: TestDb) -> (TestApp, Seed) {
    let app = TestApp::new(build, db).await;
    let seed = seed(&app).await;
    (app, seed)
}

pub fn auth(app: &TestApp) -> std::sync::Arc<AuthState> {
    app.state().service::<AuthState>().unwrap()
}

pub async fn super_admin_role(app: &TestApp, guard: &str) -> role::Model {
    role::Entity::find()
        .filter(role::Column::Name.eq(role::SUPER_ADMIN))
        .filter(role::Column::GuardName.eq(guard))
        .one(app.state().db())
        .await
        .unwrap()
        .unwrap()
}

async fn seed(app: &TestApp) -> Seed {
    let db = app.state().db();
    let auth = auth(app);
    let service = RbacService::new(db, &auth.resolver);
    service.sync_permissions(&auth.permissions.all()).await.unwrap();

    let user = |name: &str| UserFactory::new().name(name).email(format!("{name}@example.test")).password(PASSWORD);
    let admin = user("admin").create(db).await;
    let admin2 = user("admin2").create(db).await;
    let editor = user("editor").create(db).await;
    let member = user("member").create(db).await;
    let reader = user("reader").create(db).await;

    let strings = |names: &[&str]| names.iter().map(|n| (*n).to_owned()).collect::<Vec<_>>();
    let editor_role = service
        .create_role(
            Actor::System,
            ADMIN_GUARD,
            "editor",
            &strings(&["posts.view", "roles.assign", "users.update", "roles.update"]),
        )
        .await
        .unwrap();
    let moderator_role = service
        .create_role(Actor::System, ADMIN_GUARD, "moderator", &strings(&["posts.view", "posts.delete"]))
        .await
        .unwrap();
    let reader_role = service.create_role(Actor::System, API_GUARD, "reader", &strings(&["posts.view"])).await.unwrap();

    for guard in [ADMIN_GUARD, API_GUARD] {
        let role = super_admin_role(app, guard).await;
        service.assign_role(Actor::System, admin.id, role.id).await.unwrap();
    }
    service.assign_role(Actor::System, admin2.id, super_admin_role(app, ADMIN_GUARD).await.id).await.unwrap();
    service.assign_role(Actor::System, editor.id, editor_role.id).await.unwrap();
    service.assign_role(Actor::System, reader.id, reader_role.id).await.unwrap();
    Seed { admin, admin2, editor, member, reader, editor_role, reader_role, moderator_role }
}

/// Principal of `user` in `guard`, as the guards build it.
pub async fn principal(app: &TestApp, guard: &str, user: &user::Model) -> Principal {
    auth(app).resolver.principal(app.state().db(), guard, user, None).await.unwrap()
}

/// Reloads a user (fresh `auth_version`).
pub async fn reload(app: &TestApp, user: &user::Model) -> user::Model {
    user::Entity::find_by_id(user.id).one(app.state().db()).await.unwrap().unwrap()
}
