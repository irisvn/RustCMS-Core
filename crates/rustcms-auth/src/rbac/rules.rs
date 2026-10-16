//! Rules that prevent privilege escalation: who may grant what, to whom, and
//! that a super admin always remains.

use super::*;

/// The actor may replace another user's credentials (password, email), which
/// is signing in as them, everywhere: the same credentials open the admin
/// panel and issue API tokens, and an application may grant access by identity
/// (gates, ownership) without any role. So only a super admin of *every* guard
/// may do it; [`Actor::System`] always may.
pub(super) async fn covers<C: ConnectionTrait>(txn: &C, actor: Actor<'_>, _user_id: i64) -> AppResult<()> {
    let Actor::User(principal) = actor else { return Ok(()) };
    if !is_super_admin_actor(actor) {
        return forbidden();
    }
    let actor_id: i64 = principal.id.parse().map_err(|_| AppError::Forbidden)?;
    let guards: BTreeSet<String> = role::Entity::find()
        .filter(role::Column::IsSystem.eq(true))
        .filter(role::Column::Name.eq(role::SUPER_ADMIN))
        .all(txn)
        .await?
        .into_iter()
        .map(|r| r.guard_name)
        .collect();
    let held: BTreeSet<String> = super_admin_guards(txn, actor_id).await?.into_iter().collect();
    if guards.is_subset(&held) { Ok(()) } else { forbidden() }
}

/// Largest list of ids or names one change accepts.
pub(super) const MAX_ITEMS: usize = 1000;

pub(super) fn require_any(actor: Actor<'_>, permissions: &[&str]) -> AppResult<()> {
    match actor {
        Actor::System => Ok(()),
        Actor::User(p) if permissions.iter().any(|name| p.can(name)) => Ok(()),
        Actor::User(_) => forbidden(),
    }
}

pub(super) fn require(actor: Actor<'_>, permission: &str) -> AppResult<()> {
    match actor {
        Actor::System => Ok(()),
        Actor::User(p) if p.can(permission) => Ok(()),
        Actor::User(_) => forbidden(),
    }
}

/// Users manage only their own guard's roles and permissions.
pub(super) fn same_guard(actor: Actor<'_>, guard: &str) -> AppResult<()> {
    match actor {
        Actor::User(p) if p.guard != guard => forbidden(),
        _ => Ok(()),
    }
}

pub(super) fn not_self(actor: Actor<'_>, user_id: i64) -> AppResult<()> {
    match actor {
        Actor::User(p) if p.id == user_id.to_string() => forbidden(),
        _ => Ok(()),
    }
}

pub(super) fn is_super_admin_actor(actor: Actor<'_>) -> bool {
    match actor {
        Actor::System => true,
        Actor::User(p) => p.super_admin && p.token_abilities.as_ref().is_none_or(|a| a.contains("*")),
    }
}

pub(super) fn protected(actor: Actor<'_>, role: &role::Model) -> AppResult<()> {
    if role.is_protected() && !is_super_admin_actor(actor) { forbidden() } else { Ok(()) }
}

/// The actor must hold every permission it grants or takes away.
pub(super) fn grantable<'p>(
    actor: Actor<'_>,
    guard: &str,
    permissions: impl IntoIterator<Item = &'p String>,
) -> AppResult<()> {
    match actor {
        Actor::System => Ok(()),
        Actor::User(p) if p.guard != guard => forbidden(),
        Actor::User(p) => {
            if permissions.into_iter().all(|name| p.can(name)) {
                Ok(())
            } else {
                forbidden()
            }
        }
    }
}

/// Assigning a role that carries orphaned grants would hand them out, dormant,
/// to come back on a redeclaration nobody checked: super admins only.
pub(super) async fn dormant_grants<C: ConnectionTrait>(db: &C, actor: Actor<'_>, role_id: i64) -> AppResult<()> {
    if is_super_admin_actor(actor) {
        return Ok(());
    }
    let ids: Vec<i64> = role_perm::Entity::find()
        .filter(role_perm::Column::RoleId.eq(role_id))
        .all(db)
        .await?
        .into_iter()
        .map(|r| r.permission_id)
        .collect();
    let orphaned = permission::Entity::find()
        .filter(permission::Column::Id.is_in(ids))
        .filter(permission::Column::Orphaned.eq(true))
        .count(db)
        .await?;
    if orphaned > 0 { forbidden() } else { Ok(()) }
}

/// Accounts holding a protected role or a super-admin role in any guard are
/// managed (status, profile, deletion) only by super admins of every guard.
pub(super) async fn guard_target(txn: &DatabaseTransaction, actor: Actor<'_>, user_id: i64) -> AppResult<()> {
    let assigned: Vec<i64> = user_role::Entity::find()
        .filter(user_role::Column::ModelType.eq(USER_MODEL))
        .filter(user_role::Column::ModelId.eq(user_id.to_string()))
        .all(txn)
        .await?
        .into_iter()
        .map(|r| r.role_id)
        .collect();
    let roles = role::Entity::find().filter(role::Column::Id.is_in(assigned)).all(txn).await?;
    if roles.iter().any(role::Model::is_protected) { covers(txn, actor, user_id).await } else { Ok(()) }
}

pub(super) async fn ensure_super_admin_remains<C: ConnectionTrait>(db: &C, guard: &str) -> AppResult<()> {
    let Some(role) = role::Entity::find()
        .filter(role::Column::IsSystem.eq(true))
        .filter(role::Column::Name.eq(role::SUPER_ADMIN))
        .filter(role::Column::GuardName.eq(guard))
        .one(db)
        .await?
    else {
        return Ok(());
    };
    let holders: Vec<i64> = user_role::Entity::find()
        .filter(user_role::Column::RoleId.eq(role.id))
        .filter(user_role::Column::ModelType.eq(USER_MODEL))
        .all(db)
        .await?
        .into_iter()
        .filter_map(|r| r.model_id.parse().ok())
        .collect();
    let active = user::Entity::find()
        .filter(user::Column::Id.is_in(holders))
        .filter(user::Column::IsActive.eq(true))
        // An invitation not yet accepted cannot sign in: it does not count.
        .filter(user::Column::InvitationPending.eq(false))
        .count(db)
        .await?;
    if active == 0 {
        let mut errors = ValidationErrors::new();
        errors.add_form(format!("At least one active super admin is required for the `{guard}` guard."));
        return Err(errors.into());
    }
    Ok(())
}

/// Validated, normalized email for a profile with `name`.
pub(super) fn valid_profile(name: &str, email: &str) -> AppResult<String> {
    let email = email.trim().to_lowercase();
    let mut errors = ValidationErrors::new();
    let name = name.trim();
    if name.is_empty() {
        errors.add("name", "is required");
    }
    if name.chars().count() > 255 {
        errors.add("name", "may not be longer than 255 characters");
    }
    if name.chars().any(char::is_control) {
        errors.add("name", "may not contain control characters");
    }
    if !valid_email(&email) || email.len() > crate::routes::MAX_EMAIL_LEN {
        errors.add("email", "must be a valid email address");
    }
    errors.into_result()?;
    Ok(email)
}

/// `local@domain`: one `@`, both parts non-empty, a dot in the domain, no whitespace or control characters.
pub(super) fn valid_email(email: &str) -> bool {
    let Some((local, domain)) = email.split_once('@') else { return false };
    !local.is_empty()
        && !domain.is_empty()
        && !domain.contains('@')
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !email.chars().any(|c| c.is_whitespace() || c.is_control())
}
