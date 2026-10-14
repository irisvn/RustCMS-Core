//! Database reads and writes used by the service.

use super::*;

/// Creates a user inside `txn` (see [`RbacService::create_user`]).
pub(super) async fn create_user_in(
    txn: &DatabaseTransaction,
    actor: Actor<'_>,
    new: NewUser<'_>,
) -> AppResult<user::Model> {
    require(actor, "users.create")?;
    let email = valid_profile(new.name, new.email)?;
    if user::Entity::find().filter(user::Column::Email.eq(&email)).one(txn).await?.is_some() {
        return invalid("email", "has already been taken");
    }
    let super_admin = match new.super_admin_guard {
        Some(guard) => {
            let role = role::Entity::find()
                .filter(role::Column::IsSystem.eq(true))
                .filter(role::Column::Name.eq(role::SUPER_ADMIN))
                .filter(role::Column::GuardName.eq(guard))
                .one(txn)
                .await?;
            let Some(role) = role else {
                return invalid("guard", &format!("no super_admin role for guard `{guard}`"));
            };
            same_guard(actor, guard)?;
            protected(actor, &role)?;
            Some(role)
        }
        None => None,
    };
    let user = user::ActiveModel {
        name: Set(new.name.trim().to_owned()),
        email: Set(email),
        password: Set(new.password_hash),
        is_active: Set(true),
        ..Default::default()
    }
    .insert(txn)
    .await
    .map_err(email_conflict)?;
    if let Some(role) = super_admin {
        user_role::ActiveModel {
            role_id: Set(role.id),
            model_type: Set(USER_MODEL.into()),
            model_id: Set(user.id.to_string()),
        }
        .insert(txn)
        .await?;
    }
    Ok(user)
}

pub(super) fn stale<T>() -> AppResult<T> {
    let mut errors = ValidationErrors::new();
    errors.add_form("This was changed by someone else meanwhile. Reload and try again.");
    Err(errors.into())
}

/// Active (not orphaned) permissions granted to a role, by name.
pub(super) async fn active_permissions<C: ConnectionTrait>(db: &C, role_id: i64) -> AppResult<BTreeMap<String, i64>> {
    let ids = role_perm::Entity::find().filter(role_perm::Column::RoleId.eq(role_id)).all(db).await?;
    Ok(permission::Entity::find()
        .filter(permission::Column::Id.is_in(ids.into_iter().map(|r| r.permission_id)))
        .filter(permission::Column::Orphaned.eq(false))
        .all(db)
        .await?
        .into_iter()
        .map(|p| (p.name, p.id))
        .collect())
}

/// Ids of the roles `user_id` holds in `guard`.
pub(super) async fn held_roles<C: ConnectionTrait>(db: &C, user_id: i64, guard: &str) -> AppResult<BTreeSet<i64>> {
    let assigned: Vec<i64> = user_role::Entity::find()
        .filter(user_role::Column::ModelType.eq(USER_MODEL))
        .filter(user_role::Column::ModelId.eq(user_id.to_string()))
        .all(db)
        .await?
        .into_iter()
        .map(|r| r.role_id)
        .collect();
    Ok(role::Entity::find()
        .filter(role::Column::Id.is_in(assigned))
        .filter(role::Column::GuardName.eq(guard))
        .all(db)
        .await?
        .into_iter()
        .map(|r| r.id)
        .collect())
}

/// Makes `role_ids` the user's roles in `guard`: each added or removed role is
/// checked (`roles.assign`, same guard, protected roles by super admins, only
/// permissions the actor holds), and a guard keeps an active super admin.
pub(super) async fn apply_roles(
    txn: &DatabaseTransaction,
    actor: Actor<'_>,
    user_id: i64,
    guard: &str,
    role_ids: &[i64],
) -> AppResult<()> {
    require(actor, "roles.assign")?;
    same_guard(actor, guard)?;
    if role_ids.len() > MAX_ITEMS {
        return invalid("roleIds", "has too many roles");
    }
    let model_id = user_id.to_string();
    let wanted: BTreeSet<i64> = role_ids.iter().copied().collect();
    let roles = role::Entity::find().filter(role::Column::Id.is_in(wanted.iter().copied())).all(txn).await?;
    if roles.len() != wanted.len() || roles.iter().any(|r| r.guard_name != guard) {
        return invalid("roleIds", "has unknown roles");
    }
    let held = held_roles(txn, user_id, guard).await?;
    let mut removed_super_admin = false;
    for id in wanted.symmetric_difference(&held) {
        let role = role::Entity::find_by_id(*id).one(txn).await?.ok_or(AppError::NotFound)?;
        protected(actor, &role)?;
        grantable(actor, guard, active_permissions(txn, role.id).await?.keys())?;
        if held.contains(id) {
            removed_super_admin |= role.is_super_admin();
            user_role::Entity::delete_many()
                .filter(user_role::Column::RoleId.eq(role.id))
                .filter(user_role::Column::ModelType.eq(USER_MODEL))
                .filter(user_role::Column::ModelId.eq(&model_id))
                .exec(txn)
                .await?;
        } else {
            dormant_grants(txn, actor, role.id).await?;
            user_role::ActiveModel {
                role_id: Set(role.id),
                model_type: Set(USER_MODEL.into()),
                model_id: Set(model_id.clone()),
            }
            .insert(txn)
            .await?;
        }
    }
    if removed_super_admin {
        ensure_super_admin_remains(txn, guard).await?;
    }
    Ok(())
}

/// A role of the actor's guard (other guards' roles look missing, like unknown ids).
pub(super) async fn find_role<C: ConnectionTrait>(db: &C, actor: Actor<'_>, role_id: i64) -> AppResult<role::Model> {
    let mut find = role::Entity::find_by_id(role_id);
    if let Actor::User(p) = actor {
        find = find.filter(role::Column::GuardName.eq(&p.guard));
    }
    find.one(db).await?.ok_or(AppError::NotFound)
}

/// The user row, locked until commit (SQLite writers are already serialized),
/// so checks and writes see the same version even against writers outside RBAC.
pub(super) async fn lock_user(txn: &DatabaseTransaction, user_id: i64) -> AppResult<user::Model> {
    let mut find = user::Entity::find_by_id(user_id);
    if txn.get_database_backend() != DbBackend::Sqlite {
        find = find.lock_exclusive();
    }
    find.one(txn).await?.ok_or(AppError::NotFound)
}

/// A unique violation on `users.email` (a concurrent writer outside RBAC) is a 422.
pub(super) fn email_conflict(err: sea_orm::DbErr) -> AppError {
    if matches!(err.sql_err(), Some(sea_orm::SqlErr::UniqueConstraintViolation(_))) {
        ValidationErrors::field("email", "has already been taken").into()
    } else {
        err.into()
    }
}

/// Role ids exist in `guard` (and are not too many).
pub(super) async fn check_role_ids<C: ConnectionTrait>(db: &C, guard: &str, role_ids: &[i64]) -> AppResult<()> {
    if role_ids.len() > MAX_ITEMS {
        return invalid("roleIds", "has too many roles");
    }
    let wanted: BTreeSet<i64> = role_ids.iter().copied().collect();
    let found = role::Entity::find()
        .filter(role::Column::Id.is_in(wanted.iter().copied()))
        .filter(role::Column::GuardName.eq(guard))
        .count(db)
        .await?;
    if found as usize != wanted.len() { invalid("roleIds", "has unknown roles") } else { Ok(()) }
}

/// Resolves permission names of `guard`; unknown or orphaned names are a 422.
pub(super) async fn permission_ids<C: ConnectionTrait>(
    db: &C,
    guard: &str,
    names: &[String],
) -> AppResult<BTreeMap<String, i64>> {
    if names.len() > MAX_ITEMS {
        return invalid("permissions", "has too many permissions");
    }
    let found: BTreeMap<String, i64> = permission::Entity::find()
        .filter(permission::Column::GuardName.eq(guard))
        .filter(permission::Column::Orphaned.eq(false))
        .filter(permission::Column::Name.is_in(names.iter().cloned()))
        .all(db)
        .await?
        .into_iter()
        .map(|p| (p.name, p.id))
        .collect();
    let unknown: Vec<&String> = names.iter().filter(|n| !found.contains_key(*n)).collect();
    if !unknown.is_empty() {
        let mut list = unknown.iter().take(10).map(|s| s.as_str()).collect::<Vec<_>>().join(", ");
        if unknown.len() > 10 {
            list.push_str(&format!(" and {} more", unknown.len() - 10));
        }
        return invalid("permissions", &format!("unknown permissions: {list}"));
    }
    Ok(found)
}

pub(super) async fn holds_role<C: ConnectionTrait>(db: &C, model_id: &str, role_id: i64) -> AppResult<bool> {
    Ok(user_role::Entity::find_by_id((role_id, USER_MODEL.to_owned(), model_id.to_owned())).one(db).await?.is_some())
}

/// Guards in which `user_id` holds the system super-admin role.
pub(super) async fn super_admin_guards<C: ConnectionTrait>(db: &C, user_id: i64) -> AppResult<Vec<String>> {
    let role_ids = user_role::Entity::find()
        .filter(user_role::Column::ModelType.eq(USER_MODEL))
        .filter(user_role::Column::ModelId.eq(user_id.to_string()))
        .all(db)
        .await?
        .into_iter()
        .map(|r| r.role_id);
    Ok(role::Entity::find()
        .filter(role::Column::Id.is_in(role_ids))
        .all(db)
        .await?
        .into_iter()
        .filter(role::Model::is_super_admin)
        .map(|r| r.guard_name)
        .collect())
}
