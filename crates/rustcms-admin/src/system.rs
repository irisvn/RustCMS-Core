//! System pages of the panel: users, roles and permissions (`<admin>/api/system/…`).
//!
//! Every change goes through [`RbacService`] (escalation rules, last super admin)
//! or the credential helpers; nothing here writes RBAC tables directly.

use std::collections::{BTreeMap, HashMap};

use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::{Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, put};
use rustcms_auth::entities::{USER_MODEL, model_has_roles as user_role, permission, role, role_has_permissions};
use rustcms_auth::{ADMIN_GUARD, Actor, Auth, AuthState, NewUser, RbacService, Session};
use rustcms_core::models::user;
use rustcms_core::{AppError, AppResult, AppState, Json, Principal, ValidationErrors};
use sea_orm::{ColumnTrait, Condition, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder};
use serde::{Deserialize, Serialize};

use crate::http::AdminCtx;
use crate::resource::PageMeta;

/// Which system pages the user may open (`PanelResponse.system`).
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SystemLinks {
    /// Users page (`users.view_any`).
    pub users: bool,
    /// Roles page (`roles.view_any`).
    pub roles: bool,
    /// May create users (`users.create`).
    pub create_users: bool,
    /// May create roles (`roles.create`).
    pub create_roles: bool,
}

impl SystemLinks {
    pub(crate) fn of(principal: &Principal) -> Self {
        Self {
            users: principal.can("users.view_any"),
            roles: principal.can("roles.view_any"),
            create_users: principal.can("users.create"),
            create_roles: principal.can("roles.create"),
        }
    }
}

/// A role as listed on a user.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct RoleRef {
    /// Role id.
    pub id: String,
    /// Role name.
    pub name: String,
}

/// A user on the system pages.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SystemUser {
    /// User id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Sign-in email.
    pub email: String,
    /// May sign in.
    pub is_active: bool,
    /// Roles held in the admin guard.
    pub roles: Vec<RoleRef>,
    /// Holds the admin `super_admin` role.
    pub super_admin: bool,
    /// The signed-in user themselves (managed on `/account`).
    pub is_self: bool,
    /// What the current user may do with this user (the server re-checks).
    pub abilities: BTreeMap<&'static str, bool>,
}

/// `GET system/users`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct UserPage {
    /// Users.
    pub data: Vec<SystemUser>,
    /// Pagination.
    pub meta: PageMeta,
}

/// A role on the system pages.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SystemRole {
    /// Role id.
    pub id: String,
    /// Role name.
    pub name: String,
    /// Created by migrations (immutable, never deleted).
    pub is_system: bool,
    /// Managed by super admins only.
    pub is_privileged: bool,
    /// Permission names.
    pub permissions: Vec<String>,
    /// Users holding it.
    pub users: u64,
    /// What the current user may do with this role (the server re-checks).
    pub abilities: BTreeMap<&'static str, bool>,
}

/// A declared admin permission.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct PermissionInfo {
    /// Name (`posts.update`).
    pub name: String,
    /// Group (usually the resource slug).
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// The current user holds it, so may grant or remove it.
    pub grantable: bool,
}

/// Every system route as `(method, path)` (route listings, authorization oracle).
pub fn routes(admin_path: &str) -> Vec<(Method, String)> {
    let base = format!("{}/api/system", admin_path.trim_end_matches('/'));
    vec![
        (Method::GET, format!("{base}/users")),
        (Method::POST, format!("{base}/users")),
        (Method::GET, format!("{base}/users/{{id}}")),
        (Method::PATCH, format!("{base}/users/{{id}}")),
        (Method::DELETE, format!("{base}/users/{{id}}")),
        (Method::PUT, format!("{base}/users/{{id}}/roles")),
        (Method::PUT, format!("{base}/users/{{id}}/active")),
        (Method::PUT, format!("{base}/users/{{id}}/password")),
        (Method::GET, format!("{base}/roles")),
        (Method::POST, format!("{base}/roles")),
        (Method::GET, format!("{base}/roles/{{id}}")),
        (Method::DELETE, format!("{base}/roles/{{id}}")),
        (Method::PUT, format!("{base}/roles/{{id}}/permissions")),
        (Method::GET, format!("{base}/permissions")),
    ]
}

pub(crate) fn router(router: Router<AppState>, admin_path: &str) -> Router<AppState> {
    let base = format!("{}/api/system", admin_path.trim_end_matches('/'));
    router
        .route(&format!("{base}/users"), get(list_users).post(create_user))
        .route(&format!("{base}/users/{{id}}"), get(show_user).patch(update_user).delete(delete_user))
        .route(&format!("{base}/users/{{id}}/roles"), put(set_roles))
        .route(&format!("{base}/users/{{id}}/active"), put(set_active))
        .route(&format!("{base}/users/{{id}}/password"), put(reset_password))
        .route(&format!("{base}/roles"), get(list_roles).post(create_role))
        .route(&format!("{base}/roles/{{id}}"), get(show_role).delete(delete_role))
        .route(&format!("{base}/roles/{{id}}/permissions"), put(set_permissions))
        .route(&format!("{base}/permissions"), get(list_permissions))
}

fn auth(app: &AppState) -> AppResult<std::sync::Arc<AuthState>> {
    app.service::<AuthState>().ok_or_else(|| AppError::internal("AuthProvider not registered"))
}

fn require(principal: &Principal, permission: &str) -> AppResult<()> {
    if principal.can(permission) { Ok(()) } else { Err(AppError::Forbidden) }
}

fn id_of(raw: &str) -> AppResult<i64> {
    raw.parse().map_err(|_| AppError::NotFound)
}

fn ids_of(field: &str, raw: &[String]) -> AppResult<Vec<i64>> {
    if raw.len() > 1000 {
        return Err(ValidationErrors::field(field, "has too many roles").into());
    }
    raw.iter()
        .map(|id| id.parse().map_err(|_| AppError::from(ValidationErrors::field(field, "has unknown roles"))))
        .collect()
}

/// Roles held by each of `users`, in every guard (one query each).
async fn roles_of(app: &AppState, users: &[i64]) -> AppResult<HashMap<i64, Vec<role::Model>>> {
    let pivots = user_role::Entity::find()
        .filter(user_role::Column::ModelType.eq(USER_MODEL))
        .filter(user_role::Column::ModelId.is_in(users.iter().map(i64::to_string)))
        .all(app.db())
        .await?;
    let roles: HashMap<i64, role::Model> = role::Entity::find()
        .filter(role::Column::Id.is_in(pivots.iter().map(|p| p.role_id)))
        .all(app.db())
        .await?
        .into_iter()
        .map(|r| (r.id, r))
        .collect();
    let mut out: HashMap<i64, Vec<role::Model>> = HashMap::new();
    for pivot in pivots {
        if let (Ok(user), Some(role)) = (pivot.model_id.parse::<i64>(), roles.get(&pivot.role_id)) {
            out.entry(user).or_default().push(role.clone());
        }
    }
    for list in out.values_mut() {
        list.sort_by(|a, b| a.name.cmp(&b.name));
    }
    Ok(out)
}

/// What the current user may do, computed once per request.
struct ActorView<'a> {
    principal: &'a Principal,
    /// Super admin of every guard (may replace credentials, manage protected accounts).
    covers: bool,
}

impl<'a> ActorView<'a> {
    async fn of(app: &AppState, principal: &'a Principal) -> AppResult<Self> {
        let auth = auth(app)?;
        let covers = RbacService::new(app.db(), &auth.resolver).covers_all_guards(Actor::User(principal)).await?;
        Ok(Self { principal, covers })
    }
}

fn system_user(actor: &ActorView<'_>, user: user::Model, roles: Vec<role::Model>) -> SystemUser {
    let p = actor.principal;
    // Accounts with a protected or super-admin role in any guard: super admins of every guard only.
    let protected = roles.iter().any(role::Model::is_protected);
    let super_admin = roles.iter().any(|r| r.is_super_admin() && r.guard_name == ADMIN_GUARD);
    let is_self = p.id == user.id.to_string();
    let manageable = !protected || actor.covers;
    let abilities = BTreeMap::from([
        ("update", is_self || (p.can("users.update") && manageable)),
        // Someone else's email is a credential (takeover): super admins of every guard.
        ("changeEmail", is_self || (p.can("users.update") && manageable && actor.covers)),
        ("delete", !is_self && p.can("users.delete") && manageable),
        ("assignRoles", !is_self && p.can("roles.assign") && (p.can("users.view") || p.can("users.view_any"))),
        ("activate", !is_self && p.can("users.update") && manageable),
        ("resetPassword", !is_self && p.can("users.update") && actor.covers),
    ]);
    let roles = roles.into_iter().filter(|r| r.guard_name == ADMIN_GUARD);
    SystemUser {
        id: user.id.to_string(),
        name: user.name,
        email: user.email,
        is_active: user.is_active,
        roles: roles.map(|r| RoleRef { id: r.id.to_string(), name: r.name }).collect(),
        super_admin,
        is_self,
        abilities,
    }
}

#[derive(Deserialize)]
struct UserQuery {
    search: Option<String>,
    page: Option<u64>,
    per_page: Option<u64>,
}

async fn list_users(
    State(app): State<AppState>,
    AdminCtx(ctx): AdminCtx,
    Query(query): Query<UserQuery>,
) -> AppResult<Json<UserPage>> {
    require(&ctx.principal, "users.view_any")?;
    let per_page = query.per_page.unwrap_or(25).clamp(1, 100);
    let page = query.page.unwrap_or(1).clamp(1, 1_000_000);
    let mut select = user::Entity::find().order_by_asc(user::Column::Name).order_by_asc(user::Column::Id);
    if let Some(search) = query.search.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        if search.chars().count() > 100 {
            return Err(ValidationErrors::field("search", "may not be longer than 100 characters").into());
        }
        // Case-insensitive on both backends; `%`, `_` and `\\` are literal.
        let pattern = format!("%{}%", crate::adapter::escape_like(search));
        let backend = app.db().get_database_backend();
        let like = |column| crate::adapter::search_match(backend, column, crate::wire::FieldKind::String, &pattern);
        select = select.filter(Condition::any().add(like(user::Column::Name)).add(like(user::Column::Email)));
    }
    let paginator = select.paginate(app.db(), per_page);
    let totals = paginator.num_items_and_pages().await?;
    let users = paginator.fetch_page(page - 1).await?;
    let mut roles = roles_of(&app, &users.iter().map(|u| u.id).collect::<Vec<_>>()).await?;
    let actor = ActorView::of(&app, &ctx.principal).await?;
    let data = users
        .into_iter()
        .map(|u| {
            let r = roles.remove(&u.id).unwrap_or_default();
            system_user(&actor, u, r)
        })
        .collect();
    Ok(Json(UserPage {
        data,
        meta: PageMeta { page, per_page, total: totals.number_of_items, last_page: totals.number_of_pages.max(1) },
    }))
}

/// The user after a change, for callers who may read it (`users.view`, or
/// themselves); others get 204 and learn nothing about the account.
async fn changed_user(app: &AppState, actor: &Principal, id: i64, status: StatusCode) -> AppResult<Response> {
    // Current grants (the change may have revoked the caller's own read access).
    let actor_id: i64 = actor.id.parse().map_err(|_| AppError::Forbidden)?;
    let grants = rustcms_auth::rbac::load_grants(app.db(), &actor.guard, actor_id).await?;
    let actor = &Principal { permissions: grants.permissions, super_admin: grants.super_admin, ..actor.clone() };
    if actor.id != id.to_string() && !actor.can("users.view") {
        return Ok(StatusCode::NO_CONTENT.into_response());
    }
    match find_user(app, actor, id).await {
        Ok(user) => Ok((status, Json(user)).into_response()),
        // Deleted right after this change committed: the change still happened.
        Err(AppError::NotFound) => Ok(StatusCode::NO_CONTENT.into_response()),
        Err(e) => Err(e),
    }
}

async fn find_user(app: &AppState, actor: &Principal, id: i64) -> AppResult<SystemUser> {
    let user = user::Entity::find_by_id(id).one(app.db()).await?.ok_or(AppError::NotFound)?;
    let roles = roles_of(app, &[id]).await?.remove(&id).unwrap_or_default();
    Ok(system_user(&ActorView::of(app, actor).await?, user, roles))
}

async fn show_user(
    State(app): State<AppState>,
    AdminCtx(ctx): AdminCtx,
    Path(id): Path<String>,
) -> AppResult<Json<SystemUser>> {
    let id = id_of(&id)?;
    if ctx.principal.id != id.to_string() {
        require(&ctx.principal, "users.view")?;
    }
    Ok(Json(find_user(&app, &ctx.principal, id).await?))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NewUserInput {
    name: String,
    email: String,
    password: String,
    #[serde(default)]
    role_ids: Vec<String>,
}

/// Password hashing for other users is costly and shares capacity with sign-in:
/// counted per actor (`attempts_per_identifier` a minute) before any work.
fn reserve_hashing(auth: &AuthState, actor: &Principal) -> AppResult<String> {
    let key = format!("admin-hash:{}", actor.id);
    auth.limiter.reserve(&[(&key, auth.config.attempts_per_identifier)])?;
    Ok(key)
}

fn check_password(auth: &AuthState, field: &str, password: &str) -> AppResult<()> {
    let mut errors = ValidationErrors::new();
    if password.chars().count() < auth.config.password_min_length {
        errors.add(field, format!("must be at least {} characters", auth.config.password_min_length));
    }
    if password.len() > rustcms_auth::routes::MAX_PASSWORD_LEN {
        errors.add(field, format!("may not be longer than {} bytes", rustcms_auth::routes::MAX_PASSWORD_LEN));
    }
    errors.into_result()
}

async fn create_user(
    State(app): State<AppState>,
    AdminCtx(ctx): AdminCtx,
    Json(input): Json<NewUserInput>,
) -> AppResult<Response> {
    let auth = auth(&app)?;
    require(&ctx.principal, "users.create")?;
    let role_ids = ids_of("roleIds", &input.role_ids)?;
    // Everything cheap first (input, duplicate email, roles), so typos cost no hashing.
    check_password(&auth, "password", &input.password)?;
    let rbac = RbacService::new(app.db(), &auth.resolver);
    rbac.check_new_user(Actor::User(&ctx.principal), &input.name, &input.email, ADMIN_GUARD, &role_ids).await?;
    let budget = reserve_hashing(&auth, &ctx.principal)?;
    let new = NewUser {
        name: &input.name,
        email: &input.email,
        password_hash: auth.passwords.hash(input.password).await?,
        super_admin_guard: None,
    };
    let user = rbac.create_user_with_roles(Actor::User(&ctx.principal), new, ADMIN_GUARD, &role_ids).await?;
    // The budget bounds failed or abusive hashing, not onboarding.
    auth.limiter.release(&budget);
    changed_user(&app, &ctx.principal, user.id, StatusCode::CREATED).await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
struct ProfileInput {
    name: String,
    email: String,
    /// The profile the editor started from (refused if it changed meanwhile).
    expected: ExpectedProfile,
    /// Required to change one's own sign-in email (a stolen session must not
    /// be able to lock the owner out).
    #[serde(default)]
    current_password: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExpectedProfile {
    name: String,
    email: String,
}

async fn update_user(
    State(app): State<AppState>,
    AdminCtx(ctx): AdminCtx,
    Auth(me): Auth,
    session: Session,
    Path(id): Path<String>,
    Json(input): Json<ProfileInput>,
) -> AppResult<Response> {
    let id = id_of(&id)?;
    let auth = auth(&app)?;
    // Verified against the authenticated snapshot; whether it is needed is decided
    // inside the change (against the stored email), which is also refused if the
    // credentials moved on meanwhile (auth_version under lock).
    let mut confirmed = false;
    let own_email_change = me.user.id == id && !input.email.trim().to_lowercase().eq(&me.user.email);
    if own_email_change && let Some(password) = input.current_password.clone() {
        rustcms_auth::routes::confirm_password(&auth, &me.user, password).await?;
        confirmed = true;
    }
    let updated = RbacService::new(app.db(), &auth.resolver)
        .update_profile_if(
            Actor::User(&ctx.principal),
            id,
            &input.name,
            &input.email,
            Some((&input.expected.name, &input.expected.email)),
            confirmed,
        )
        .await?;
    // Changing one's own email ended the other sessions; this one continues re-issued.
    if updated.id == me.user.id && updated.auth_version != me.user.auth_version {
        session.restamp(updated.auth_version);
    }
    changed_user(&app, &ctx.principal, id, StatusCode::OK).await
}

async fn delete_user(
    State(app): State<AppState>,
    AdminCtx(ctx): AdminCtx,
    Path(id): Path<String>,
) -> AppResult<StatusCode> {
    let id = id_of(&id)?;
    let auth = auth(&app)?;
    RbacService::new(app.db(), &auth.resolver).delete_user(Actor::User(&ctx.principal), id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RolesInput {
    role_ids: Vec<String>,
    /// The roles the editor started from (refused if they changed meanwhile).
    expected_role_ids: Vec<String>,
}

async fn set_roles(
    State(app): State<AppState>,
    AdminCtx(ctx): AdminCtx,
    Path(id): Path<String>,
    Json(input): Json<RolesInput>,
) -> AppResult<Response> {
    let id = id_of(&id)?;
    let auth = auth(&app)?;
    // Deny before looking at the input: the answer must not depend on it.
    require(&ctx.principal, "roles.assign")?;
    if !ctx.principal.can("users.view") && !ctx.principal.can("users.view_any") {
        return Err(AppError::Forbidden);
    }
    let role_ids = ids_of("roleIds", &input.role_ids)?;
    let expected = ids_of("roleIds", &input.expected_role_ids)?;
    RbacService::new(app.db(), &auth.resolver)
        .set_user_roles_if(Actor::User(&ctx.principal), id, ADMIN_GUARD, &role_ids, Some(&expected))
        .await?;
    changed_user(&app, &ctx.principal, id, StatusCode::OK).await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ActiveInput {
    active: bool,
    /// The status the editor saw (refused if it changed meanwhile).
    expected: bool,
}

async fn set_active(
    State(app): State<AppState>,
    AdminCtx(ctx): AdminCtx,
    Path(id): Path<String>,
    Json(input): Json<ActiveInput>,
) -> AppResult<Response> {
    let id = id_of(&id)?;
    let auth = auth(&app)?;
    RbacService::new(app.db(), &auth.resolver)
        .set_user_active_if(Actor::User(&ctx.principal), id, input.active, Some(input.expected))
        .await?;
    changed_user(&app, &ctx.principal, id, StatusCode::OK).await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PasswordInput {
    password: String,
}

async fn reset_password(
    State(app): State<AppState>,
    AdminCtx(ctx): AdminCtx,
    Path(id): Path<String>,
    Json(input): Json<PasswordInput>,
) -> AppResult<StatusCode> {
    let id = id_of(&id)?;
    let auth = auth(&app)?;
    require(&ctx.principal, "users.update")?;
    // Refused before any hashing unless the change could succeed (all re-checked in it).
    RbacService::new(app.db(), &auth.resolver).check_reset_password(Actor::User(&ctx.principal), id).await?;
    check_password(&auth, "password", &input.password)?;
    reserve_hashing(&auth, &ctx.principal)?;
    let hash = auth.passwords.hash(input.password).await?;
    RbacService::new(app.db(), &auth.resolver).reset_password(Actor::User(&ctx.principal), id, hash).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `roles` as shown to `actor`, in four queries whatever their number.
/// Contents (permissions, user counts) only with `show_contents`.
async fn system_roles(
    app: &AppState,
    actor: &Principal,
    roles: Vec<role::Model>,
    show_contents: bool,
) -> AppResult<Vec<SystemRole>> {
    let ids: Vec<i64> = roles.iter().map(|r| r.id).collect();
    let pivots = role_has_permissions::Entity::find()
        .filter(role_has_permissions::Column::RoleId.is_in(ids.clone()))
        .all(app.db())
        .await?;
    let names: HashMap<i64, String> = permission::Entity::find()
        .filter(permission::Column::Id.is_in(pivots.iter().map(|p| p.permission_id)))
        .filter(permission::Column::Orphaned.eq(false))
        .all(app.db())
        .await?
        .into_iter()
        .map(|p| (p.id, p.name))
        .collect();
    let mut permissions: HashMap<i64, Vec<String>> = HashMap::new();
    for pivot in &pivots {
        if let Some(name) = names.get(&pivot.permission_id) {
            permissions.entry(pivot.role_id).or_default().push(name.clone());
        }
    }
    // Counts aggregated by the database (only when shown); the actor's own roles apart.
    let users: HashMap<i64, u64> = if show_contents {
        use sea_orm::QuerySelect;
        user_role::Entity::find()
            .select_only()
            .column(user_role::Column::RoleId)
            .column_as(user_role::Column::ModelId.count(), "n")
            .filter(user_role::Column::RoleId.is_in(ids.clone()))
            .filter(user_role::Column::ModelType.eq(USER_MODEL))
            .group_by(user_role::Column::RoleId)
            .into_tuple::<(i64, i64)>()
            .all(app.db())
            .await?
            .into_iter()
            .map(|(role, n)| (role, n.max(0) as u64))
            .collect()
    } else {
        HashMap::new()
    };
    let held: std::collections::HashSet<i64> = user_role::Entity::find()
        .filter(user_role::Column::RoleId.is_in(ids))
        .filter(user_role::Column::ModelType.eq(USER_MODEL))
        .filter(user_role::Column::ModelId.eq(&actor.id))
        .all(app.db())
        .await?
        .into_iter()
        .map(|r| r.role_id)
        .collect();
    Ok(roles
        .into_iter()
        .map(|role| {
            let mut perms = permissions.remove(&role.id).unwrap_or_default();
            perms.sort();
            let manageable = !role.is_protected() || actor.super_admin;
            // Assigning, revoking or deleting needs every permission of the role.
            let holds_all = perms.iter().all(|p| actor.can(p));
            let abilities = BTreeMap::from([
                ("update", actor.can("roles.update") && manageable && !held.contains(&role.id)),
                ("delete", actor.can("roles.delete") && manageable && !role.is_system && holds_all),
                ("assign", actor.can("roles.assign") && manageable && holds_all),
            ]);
            let users = users.get(&role.id).copied().unwrap_or(0);
            let (permissions, users) = if show_contents { (perms, users) } else { (Vec::new(), 0) };
            SystemRole {
                id: role.id.to_string(),
                name: role.name,
                is_system: role.is_system,
                is_privileged: role.is_privileged,
                permissions,
                users,
                abilities,
            }
        })
        .collect())
}

async fn system_role(app: &AppState, actor: &Principal, role: role::Model) -> AppResult<SystemRole> {
    let mut list = system_roles(app, actor, vec![role], true).await?;
    Ok(list.remove(0))
}

async fn list_roles(State(app): State<AppState>, AdminCtx(ctx): AdminCtx) -> AppResult<Json<Vec<SystemRole>>> {
    // Assigning roles needs their names; their contents need roles.view_any.
    if !ctx.principal.can("roles.view_any") && !ctx.principal.can("roles.assign") {
        return Err(AppError::Forbidden);
    }
    let roles = role::Entity::find()
        .filter(role::Column::GuardName.eq(ADMIN_GUARD))
        .order_by_asc(role::Column::Name)
        .all(app.db())
        .await?;
    let contents = ctx.principal.can("roles.view_any");
    Ok(Json(system_roles(&app, &ctx.principal, roles, contents).await?))
}

async fn show_role(
    State(app): State<AppState>,
    AdminCtx(ctx): AdminCtx,
    Path(id): Path<String>,
) -> AppResult<Json<SystemRole>> {
    if !ctx.principal.can("roles.view") && !ctx.principal.can("roles.view_any") {
        return Err(AppError::Forbidden);
    }
    let role = role::Entity::find_by_id(id_of(&id)?)
        .filter(role::Column::GuardName.eq(ADMIN_GUARD))
        .one(app.db())
        .await?
        .ok_or(AppError::NotFound)?;
    Ok(Json(system_role(&app, &ctx.principal, role).await?))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewRoleInput {
    name: String,
    #[serde(default)]
    permissions: Vec<String>,
}

async fn create_role(
    State(app): State<AppState>,
    AdminCtx(ctx): AdminCtx,
    Json(input): Json<NewRoleInput>,
) -> AppResult<(StatusCode, Json<SystemRole>)> {
    let auth = auth(&app)?;
    require(&ctx.principal, "roles.create")?;
    let role = RbacService::new(app.db(), &auth.resolver)
        .create_role(Actor::User(&ctx.principal), ADMIN_GUARD, &input.name, &input.permissions)
        .await?;
    Ok((StatusCode::CREATED, Json(system_role(&app, &ctx.principal, role).await?)))
}

async fn delete_role(
    State(app): State<AppState>,
    AdminCtx(ctx): AdminCtx,
    Path(id): Path<String>,
) -> AppResult<StatusCode> {
    let auth = auth(&app)?;
    RbacService::new(app.db(), &auth.resolver).delete_role(Actor::User(&ctx.principal), id_of(&id)?).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
struct PermissionsInput {
    permissions: Vec<String>,
    /// The permissions the editor started from (refused if they changed meanwhile).
    expected_permissions: Vec<String>,
}

async fn set_permissions(
    State(app): State<AppState>,
    AdminCtx(ctx): AdminCtx,
    Path(id): Path<String>,
    Json(input): Json<PermissionsInput>,
) -> AppResult<Json<SystemRole>> {
    let auth = auth(&app)?;
    let id = id_of(&id)?;
    RbacService::new(app.db(), &auth.resolver)
        .set_role_permissions_if(Actor::User(&ctx.principal), id, &input.permissions, Some(&input.expected_permissions))
        .await?;
    let role = role::Entity::find_by_id(id).one(app.db()).await?.ok_or(AppError::NotFound)?;
    Ok(Json(system_role(&app, &ctx.principal, role).await?))
}

async fn list_permissions(
    State(app): State<AppState>,
    AdminCtx(ctx): AdminCtx,
) -> AppResult<Json<Vec<PermissionInfo>>> {
    if !ctx.principal.can("roles.view_any") && !ctx.principal.can("permissions.view_any") {
        return Err(AppError::Forbidden);
    }
    let permissions = permission::Entity::find()
        .filter(permission::Column::GuardName.eq(ADMIN_GUARD))
        .filter(permission::Column::Orphaned.eq(false))
        .order_by_asc(permission::Column::Name)
        .all(app.db())
        .await?;
    Ok(Json(
        permissions
            .into_iter()
            .map(|p| PermissionInfo { grantable: ctx.principal.can(&p.name), name: p.name, group: p.group })
            .collect(),
    ))
}
