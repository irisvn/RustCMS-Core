//! Roles and permissions: list, show, create, delete, grant.

use super::*;

/// `roles` as shown to `actor`, in four queries whatever their number.
/// Contents (permissions, user counts) only with `show_contents`.
pub(super) async fn system_roles(
    app: &AppState,
    actor: &Principal,
    roles: Vec<role::Model>,
    show_contents: bool,
) -> AppResult<Vec<SystemRole>> {
    let ids: Vec<i64> = roles.iter().map(|r| r.id).collect();
    let mut pivots = Vec::new();
    for chunk in chunks(ids.iter().copied()) {
        pivots.extend(
            role_has_permissions::Entity::find()
                .filter(role_has_permissions::Column::RoleId.is_in(chunk))
                .all(app.db())
                .await?,
        );
    }
    let mut names: HashMap<i64, String> = HashMap::new();
    for chunk in chunks(pivots.iter().map(|p| p.permission_id)) {
        names.extend(
            permission::Entity::find()
                .filter(permission::Column::Id.is_in(chunk))
                .filter(permission::Column::Orphaned.eq(false))
                .all(app.db())
                .await?
                .into_iter()
                .map(|p| (p.id, p.name)),
        );
    }
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
                (
                    "delete",
                    actor.can("roles.delete") && manageable && !role.is_system && holds_all && !held.contains(&role.id),
                ),
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

pub(super) async fn system_role(app: &AppState, actor: &Principal, role: role::Model) -> AppResult<SystemRole> {
    let mut list = system_roles(app, actor, vec![role], true).await?;
    Ok(list.remove(0))
}

pub(super) async fn list_roles(
    State(app): State<AppState>,
    AdminCtx(ctx): AdminCtx,
) -> AppResult<Json<Vec<SystemRole>>> {
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

pub(super) async fn show_role(
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
pub(super) struct NewRoleInput {
    name: String,
    #[serde(default)]
    permissions: Vec<String>,
}

pub(super) async fn create_role(
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

pub(super) async fn delete_role(
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
pub(super) struct PermissionsInput {
    permissions: Vec<String>,
    /// The permissions the editor started from (refused if they changed meanwhile).
    expected_permissions: Vec<String>,
}

pub(super) async fn set_permissions(
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

pub(super) async fn list_permissions(
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
            .map(|p| PermissionInfo {
                grantable: ctx.principal.can(&p.name),
                group_label: p.group.as_deref().map(|g| group_label(&ctx, g)),
                name: p.name,
                group: p.group,
            })
            .collect(),
    ))
}

/// A permission group as shown: the plural label of the resource it names,
/// else the group in words (`users` → "Users"), translated.
pub(crate) fn group_label(ctx: &crate::Ctx, group: &str) -> String {
    match ctx.registry.get(group) {
        Some(resource) => resource.info().translated(ctx).plural_label,
        None => ctx.t(&group_source(group)),
    }
}

/// The English source of a group that is not a resource.
pub(crate) fn group_source(group: &str) -> String {
    crate::ui::humanize(group)
}
