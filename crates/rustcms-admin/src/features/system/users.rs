//! Users: list, show, create, edit, roles, enable/disable, password reset.

use super::*;

/// What the current user may do, computed once per request.
pub(super) struct ActorView<'a> {
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

pub(super) fn system_user(actor: &ActorView<'_>, user: user::Model, roles: Vec<role::Model>) -> SystemUser {
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
pub(super) struct UserQuery {
    search: Option<String>,
    page: Option<u64>,
    per_page: Option<u64>,
}

pub(super) async fn list_users(
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
pub(super) async fn changed_user(
    app: &AppState,
    actor: &Principal,
    id: i64,
    status: StatusCode,
) -> AppResult<Response> {
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

pub(super) async fn find_user(app: &AppState, actor: &Principal, id: i64) -> AppResult<SystemUser> {
    let user = user::Entity::find_by_id(id).one(app.db()).await?.ok_or(AppError::NotFound)?;
    let roles = roles_of(app, &[id]).await?.remove(&id).unwrap_or_default();
    Ok(system_user(&ActorView::of(app, actor).await?, user, roles))
}

pub(super) async fn show_user(
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
pub(super) struct NewUserInput {
    name: String,
    email: String,
    password: String,
    #[serde(default)]
    role_ids: Vec<String>,
}

/// Password hashing for other users is costly and shares capacity with sign-in:
/// counted per actor (`attempts_per_identifier` a minute) before any work.
/// Every hash counts, successful or not (it is the CPU shared with sign-in that is bounded);
/// creations have a larger budget than password resets.
pub(super) fn reserve_hashing(auth: &AuthState, actor: &Principal, kind: &str, per_minute: u32) -> AppResult<()> {
    auth.limiter.reserve(&[(&format!("admin-hash:{kind}:{}", actor.id), per_minute)])
}

pub(super) fn check_password(auth: &AuthState, field: &str, password: &str) -> AppResult<()> {
    let mut errors = ValidationErrors::new();
    if password.chars().count() < auth.config.password_min_length {
        errors.add(field, format!("must be at least {} characters", auth.config.password_min_length));
    }
    if password.len() > rustcms_auth::routes::MAX_PASSWORD_LEN {
        errors.add(field, format!("may not be longer than {} bytes", rustcms_auth::routes::MAX_PASSWORD_LEN));
    }
    errors.into_result()
}

pub(super) async fn create_user(
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
    reserve_hashing(&auth, &ctx.principal, "create", auth.config.attempts_per_ip)?;
    let new = NewUser {
        name: &input.name,
        email: &input.email,
        password_hash: auth.passwords.hash(input.password).await?,
        super_admin_guard: None,
    };
    let user = rbac.create_user_with_roles(Actor::User(&ctx.principal), new, ADMIN_GUARD, &role_ids).await?;
    changed_user(&app, &ctx.principal, user.id, StatusCode::CREATED).await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub(super) struct ProfileInput {
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
pub(super) struct ExpectedProfile {
    name: String,
    email: String,
}

pub(super) async fn update_user(
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

pub(super) async fn delete_user(
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
pub(super) struct RolesInput {
    role_ids: Vec<String>,
    /// The roles the editor started from (refused if they changed meanwhile).
    expected_role_ids: Vec<String>,
}

pub(super) async fn set_roles(
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
pub(super) struct ActiveInput {
    active: bool,
    /// The status the editor saw (refused if it changed meanwhile).
    expected: bool,
}

pub(super) async fn set_active(
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
pub(super) struct PasswordInput {
    password: String,
}

pub(super) async fn reset_password(
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
    reserve_hashing(&auth, &ctx.principal, "reset", auth.config.attempts_per_identifier)?;
    let hash = auth.passwords.hash(input.password).await?;
    RbacService::new(app.db(), &auth.resolver).reset_password(Actor::User(&ctx.principal), id, hash).await?;
    Ok(StatusCode::NO_CONTENT)
}
