//! Roles and permissions: resolution (with cache) and mutations (with invariants).
//!
//! Every change to roles, permissions or role assignments goes through
//! [`RbacService`], which enforces:
//!
//! - the actor holds the required permission (`roles.assign`, `roles.update`, …);
//! - users only manage roles of their own guard;
//! - nobody grants, through a role or directly, a permission they do not hold;
//! - protected roles (system or privileged) are managed by super admins only,
//!   and system roles cannot be renamed, re-flagged or deleted;
//! - nobody changes their own roles or direct permissions, nor edits a role they hold;
//! - each guard keeps at least one active super admin.
//!
//! Changes are serialized by [`lock`] and authorize with the actor's grants
//! re-read under it.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use moka::future::Cache;
use rustcms_core::models::user;
use rustcms_core::{AppError, AppResult, Principal, ValidationErrors};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, DatabaseTransaction, DbBackend, EntityTrait,
    PaginatorTrait, QueryFilter, QuerySelect, Set,
};

use crate::entities::{
    USER_MODEL, model_has_permissions as user_perm, model_has_roles as user_role, permission, role,
    role_has_permissions as role_perm,
};

/// What a user may do within one guard.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Grants {
    /// Effective permissions (via roles and direct), excluding orphaned ones.
    pub permissions: HashSet<String>,
    /// Holds the guard's system super-admin role.
    pub super_admin: bool,
}

/// Loads [`Grants`] with a short-lived cache, evicted on every RBAC change.
///
/// Entries are keyed by a generation that every change advances. A load that
/// started before a change can only populate the previous generation, which no
/// later request reads, so eviction cannot be undone by an in-flight load.
#[derive(Clone)]
pub struct PermissionResolver {
    cache: Cache<(String, i64, u64), Arc<Grants>>,
    generation: Arc<AtomicU64>,
}

/// Loads retried before giving up on caching under constant churn.
const LOAD_ATTEMPTS: usize = 3;

impl PermissionResolver {
    /// Cache entries expire after `ttl` even without eviction.
    pub fn new(ttl: Duration) -> Self {
        Self {
            cache: Cache::builder().time_to_live(ttl).max_capacity(10_000).build(),
            generation: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Grants of `user_id` in `guard`.
    pub async fn grants(&self, db: &DatabaseConnection, guard: &str, user_id: i64) -> AppResult<Arc<Grants>> {
        self.get_or_load(guard, user_id, || load_grants(db, guard, user_id)).await
    }

    async fn get_or_load<F, Fut>(&self, guard: &str, user_id: i64, load: F) -> AppResult<Arc<Grants>>
    where
        F: Fn() -> Fut,
        Fut: Future<Output = AppResult<Grants>>,
    {
        for _ in 0..LOAD_ATTEMPTS {
            let generation = self.generation.load(Ordering::SeqCst);
            let key = (guard.to_owned(), user_id, generation);
            if let Some(grants) = self.cache.get(&key).await {
                return Ok(grants);
            }
            let grants = Arc::new(load().await?);
            if self.generation.load(Ordering::SeqCst) == generation {
                self.cache.insert(key, grants.clone()).await;
                return Ok(grants);
            }
        }
        Ok(Arc::new(load().await?))
    }

    /// Builds the principal of `user` for `guard`, optionally limited by token abilities.
    pub async fn principal(
        &self,
        db: &DatabaseConnection,
        guard: &str,
        user: &user::Model,
        token_abilities: Option<HashSet<String>>,
    ) -> AppResult<Principal> {
        let grants = self.grants(db, guard, user.id).await?;
        Ok(Principal {
            id: user.id.to_string(),
            guard: guard.to_owned(),
            permissions: grants.permissions.clone(),
            super_admin: grants.super_admin,
            token_abilities,
            auth_version: Some(user.auth_version),
        })
    }

    /// Forgets every cached entry, including loads still in flight.
    pub fn evict_all(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.cache.invalidate_all();
    }
}

/// Loads grants without caching.
pub async fn load_grants<C: ConnectionTrait>(db: &C, guard: &str, user_id: i64) -> AppResult<Grants> {
    let model_id = user_id.to_string();
    let role_ids: Vec<i64> = user_role::Entity::find()
        .filter(user_role::Column::ModelType.eq(USER_MODEL))
        .filter(user_role::Column::ModelId.eq(&model_id))
        .all(db)
        .await?
        .into_iter()
        .map(|r| r.role_id)
        .collect();
    let roles = role::Entity::find()
        .filter(role::Column::Id.is_in(role_ids))
        .filter(role::Column::GuardName.eq(guard))
        .all(db)
        .await?;
    let super_admin = roles.iter().any(role::Model::is_super_admin);

    let mut permission_ids: Vec<i64> = role_perm::Entity::find()
        .filter(role_perm::Column::RoleId.is_in(roles.iter().map(|r| r.id)))
        .all(db)
        .await?
        .into_iter()
        .map(|p| p.permission_id)
        .collect();
    permission_ids.extend(
        user_perm::Entity::find()
            .filter(user_perm::Column::ModelType.eq(USER_MODEL))
            .filter(user_perm::Column::ModelId.eq(&model_id))
            .all(db)
            .await?
            .into_iter()
            .map(|p| p.permission_id),
    );
    let permissions = permission::Entity::find()
        .filter(permission::Column::Id.is_in(permission_ids))
        .filter(permission::Column::GuardName.eq(guard))
        .filter(permission::Column::Orphaned.eq(false))
        .all(db)
        .await?
        .into_iter()
        .map(|p| p.name)
        .collect();
    Ok(Grants { permissions, super_admin })
}

/// Who performs an RBAC change.
#[derive(Debug, Clone, Copy)]
pub enum Actor<'a> {
    /// Trusted operator (console commands, seeders).
    System,
    /// An authenticated user.
    User(&'a Principal),
}

/// Input of [`RbacService::create_user`].
#[derive(Debug, Clone)]
pub struct NewUser<'a> {
    /// Display name.
    pub name: &'a str,
    /// Sign-in email (normalized to lowercase).
    pub email: &'a str,
    /// Argon2id hash of the password.
    pub password_hash: String,
    /// Also grant the system `super_admin` role of this guard.
    pub super_admin_guard: Option<&'a str>,
}

/// Result of [`RbacService::sync_permissions`].
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SyncReport {
    /// Newly created permissions.
    pub created: Vec<String>,
    /// Permissions no longer declared, now marked orphaned.
    pub orphaned: Vec<String>,
}

/// Every mutation of roles, permissions and user status. See the module docs.
pub struct RbacService<'a> {
    db: &'a DatabaseConnection,
    resolver: &'a PermissionResolver,
}

fn forbidden<T>() -> AppResult<T> {
    Err(AppError::Forbidden)
}

fn invalid<T>(field: &str, message: &str) -> AppResult<T> {
    Err(ValidationErrors::field(field, message).into())
}

impl<'a> RbacService<'a> {
    /// Service over `db`, evicting `resolver` after each change.
    pub fn new(db: &'a DatabaseConnection, resolver: &'a PermissionResolver) -> Self {
        Self { db, resolver }
    }

    /// Commits and always evicts cached grants: if COMMIT reached the server but
    /// its answer was lost, the change may be applied, so the cache must not outlive it.
    async fn commit(&self, txn: DatabaseTransaction) -> AppResult<()> {
        // COMMIT and the eviction after it run to completion even if the request is
        // cancelled meanwhile: evicting before the commit is visible would let a
        // concurrent load cache the old grants again.
        let resolver = self.resolver.clone();
        let done = tokio::spawn(async move {
            let result = txn.commit().await;
            resolver.evict_all();
            result
        });
        Ok(done.await.map_err(|e| AppError::internal(format!("commit task: {e}")))??)
    }

    /// Opens a write transaction holding the RBAC lock and reloads the acting
    /// user's grants inside it, so authorization uses current permissions, not
    /// the snapshot taken when the request was authenticated.
    async fn begin(&self, actor: Actor<'_>) -> AppResult<(DatabaseTransaction, Fresh)> {
        let txn = rustcms_core::db::begin_write(self.db).await?;
        if txn.get_database_backend() == DbBackend::Postgres {
            // A stuck lock holder must not pin every RBAC request (and the pool) forever.
            txn.execute_unprepared("SET LOCAL lock_timeout = '10s'").await?;
        }
        lock(&txn).await?;
        let fresh = match actor {
            Actor::System => Fresh(None),
            Actor::User(p) => {
                let id: i64 = p.id.parse().map_err(|_| AppError::Forbidden)?;
                // The actor's row is held (shared) until commit, so revoking their
                // credentials waits for this change, or this change sees it.
                let mut find = user::Entity::find_by_id(id);
                if txn.get_database_backend() == DbBackend::Postgres {
                    find = find.lock_shared();
                }
                let user = find
                    .one(&txn)
                    .await?
                    // Fail closed: a principal without its credential version cannot prove it is current.
                    .filter(|u| u.is_active && p.auth_version == Some(u.auth_version))
                    .ok_or(AppError::Forbidden)?;
                let grants = load_grants(&txn, &p.guard, user.id).await?;
                Fresh(Some(Principal { permissions: grants.permissions, super_admin: grants.super_admin, ..p.clone() }))
            }
        };
        Ok((txn, fresh))
    }

    /// Creates a role in `guard` with `permissions`.
    pub async fn create_role(
        &self,
        actor: Actor<'_>,
        guard: &str,
        name: &str,
        permissions: &[String],
    ) -> AppResult<role::Model> {
        let (txn, fresh) = self.begin(actor).await?;
        let actor = fresh.actor();
        require(actor, "roles.create")?;
        same_guard(actor, guard)?;
        let name = name.trim();
        if name.is_empty() {
            return invalid("name", "is required");
        }
        if name.chars().count() > 120 || name.chars().any(char::is_control) {
            return invalid("name", "must be at most 120 characters, without control characters");
        }
        // Names differing only in case would be confusing to assign.
        let taken = role::Entity::find()
            .filter(role::Column::GuardName.eq(guard))
            .all(&txn)
            .await?
            .iter()
            .any(|r| r.name.to_lowercase() == name.to_lowercase());
        if taken {
            return invalid("name", "has already been taken");
        }
        let wanted = permission_ids(&txn, guard, permissions).await?;
        grantable(actor, guard, wanted.keys())?;
        let role = role::ActiveModel {
            name: Set(name.to_owned()),
            guard_name: Set(guard.to_owned()),
            is_system: Set(false),
            is_privileged: Set(false),
            ..Default::default()
        }
        .insert(&txn)
        .await?;
        for id in wanted.values() {
            role_perm::ActiveModel { role_id: Set(role.id), permission_id: Set(*id) }.insert(&txn).await?;
        }
        self.commit(txn).await?;
        Ok(role)
    }

    /// Replaces the permissions of a role. Added and removed permissions must both be held by the actor.
    pub async fn set_role_permissions(&self, actor: Actor<'_>, role_id: i64, permissions: &[String]) -> AppResult<()> {
        self.set_role_permissions_if(actor, role_id, permissions, None).await
    }

    /// Like [`Self::set_role_permissions`], but only if the role still has exactly
    /// `expected` (the permissions the editor started from): a stale draft can
    /// never undo a change made meanwhile. Orphaned grants are kept untouched.
    pub async fn set_role_permissions_if(
        &self,
        actor: Actor<'_>,
        role_id: i64,
        permissions: &[String],
        expected: Option<&[String]>,
    ) -> AppResult<()> {
        let (txn, fresh) = self.begin(actor).await?;
        let actor = fresh.actor();
        require(actor, "roles.update")?;
        // Comparing with `expected` answers "stale" or not: only for callers who may read the role.
        if expected.is_some() {
            require_any(actor, &["roles.view", "roles.view_any"])?;
        }
        let role = find_role(&txn, actor, role_id).await?;
        protected(actor, &role)?;
        if let Actor::User(principal) = actor
            && holds_role(&txn, &principal.id, role.id).await?
        {
            return forbidden();
        }
        let wanted = permission_ids(&txn, &role.guard_name, permissions).await?;
        let current = active_permissions(&txn, role.id).await?;
        let current_names: BTreeSet<String> = current.keys().cloned().collect();
        if let Some(expected) = expected
            && expected.iter().cloned().collect::<BTreeSet<_>>() != current_names
        {
            return stale();
        }
        let wanted_names: BTreeSet<String> = wanted.keys().cloned().collect();
        let changed: Vec<&String> = wanted_names.symmetric_difference(&current_names).collect();
        grantable(actor, &role.guard_name, changed)?;

        // Only active grants are replaced: orphaned ones stay, ready for a redeclaration.
        role_perm::Entity::delete_many()
            .filter(role_perm::Column::RoleId.eq(role.id))
            .filter(role_perm::Column::PermissionId.is_in(current.values().copied()))
            .exec(&txn)
            .await?;
        for id in wanted.values() {
            role_perm::ActiveModel { role_id: Set(role.id), permission_id: Set(*id) }.insert(&txn).await?;
        }
        self.commit(txn).await?;
        Ok(())
    }

    /// Deletes a role. System roles cannot be deleted.
    pub async fn delete_role(&self, actor: Actor<'_>, role_id: i64) -> AppResult<()> {
        let (txn, fresh) = self.begin(actor).await?;
        let actor = fresh.actor();
        require(actor, "roles.delete")?;
        let role = role::Entity::find_by_id(role_id).one(&txn).await?.ok_or(AppError::NotFound)?;
        same_guard(actor, &role.guard_name)?;
        if role.is_system {
            return forbidden();
        }
        protected(actor, &role)?;
        // Nobody changes their own access, deleting a role they hold included.
        if let Actor::User(principal) = actor
            && holds_role(&txn, &principal.id, role.id).await?
        {
            return forbidden();
        }
        let permissions = active_permissions(&txn, role.id).await?;
        grantable(actor, &role.guard_name, permissions.keys())?;
        user_role::Entity::delete_many().filter(user_role::Column::RoleId.eq(role.id)).exec(&txn).await?;
        role::Entity::delete_by_id(role.id).exec(&txn).await?;
        self.commit(txn).await?;
        Ok(())
    }

    /// Assigns a role to a user.
    pub async fn assign_role(&self, actor: Actor<'_>, user_id: i64, role_id: i64) -> AppResult<()> {
        let (txn, fresh) = self.begin(actor).await?;
        let actor = fresh.actor();
        require(actor, "roles.assign")?;
        not_self(actor, user_id)?;
        user::Entity::find_by_id(user_id).one(&txn).await?.ok_or(AppError::NotFound)?;
        let role = role::Entity::find_by_id(role_id).one(&txn).await?.ok_or(AppError::NotFound)?;
        same_guard(actor, &role.guard_name)?;
        protected(actor, &role)?;
        grantable(actor, &role.guard_name, active_permissions(&txn, role.id).await?.keys())?;
        if !holds_role(&txn, &user_id.to_string(), role.id).await? {
            dormant_grants(&txn, actor, role.id).await?;
            user_role::ActiveModel {
                role_id: Set(role.id),
                model_type: Set(USER_MODEL.into()),
                model_id: Set(user_id.to_string()),
            }
            .insert(&txn)
            .await?;
        }
        self.commit(txn).await?;
        Ok(())
    }

    /// Revokes a role from a user. Keeps at least one active super admin per guard.
    pub async fn revoke_role(&self, actor: Actor<'_>, user_id: i64, role_id: i64) -> AppResult<()> {
        let (txn, fresh) = self.begin(actor).await?;
        let actor = fresh.actor();
        require(actor, "roles.assign")?;
        not_self(actor, user_id)?;
        let role = role::Entity::find_by_id(role_id).one(&txn).await?.ok_or(AppError::NotFound)?;
        same_guard(actor, &role.guard_name)?;
        protected(actor, &role)?;
        grantable(actor, &role.guard_name, active_permissions(&txn, role.id).await?.keys())?;
        user_role::Entity::delete_many()
            .filter(user_role::Column::RoleId.eq(role.id))
            .filter(user_role::Column::ModelType.eq(USER_MODEL))
            .filter(user_role::Column::ModelId.eq(user_id.to_string()))
            .exec(&txn)
            .await?;
        if role.is_super_admin() {
            ensure_super_admin_remains(&txn, &role.guard_name).await?;
        }
        self.commit(txn).await?;
        Ok(())
    }

    /// Grants a permission directly to a user.
    pub async fn grant_permission(&self, actor: Actor<'_>, user_id: i64, guard: &str, name: &str) -> AppResult<()> {
        let (txn, fresh) = self.begin(actor).await?;
        let actor = fresh.actor();
        require(actor, "permissions.assign")?;
        not_self(actor, user_id)?;
        same_guard(actor, guard)?;
        user::Entity::find_by_id(user_id).one(&txn).await?.ok_or(AppError::NotFound)?;
        let ids = permission_ids(&txn, guard, &[name.to_owned()]).await?;
        grantable(actor, guard, ids.keys())?;
        let permission_id = ids[name];
        let exists = user_perm::Entity::find_by_id((permission_id, USER_MODEL.to_owned(), user_id.to_string()))
            .one(&txn)
            .await?
            .is_some();
        if !exists {
            user_perm::ActiveModel {
                permission_id: Set(permission_id),
                model_type: Set(USER_MODEL.into()),
                model_id: Set(user_id.to_string()),
            }
            .insert(&txn)
            .await?;
        }
        self.commit(txn).await?;
        Ok(())
    }

    /// Revokes a permission granted directly to a user.
    pub async fn revoke_permission(&self, actor: Actor<'_>, user_id: i64, guard: &str, name: &str) -> AppResult<()> {
        let (txn, fresh) = self.begin(actor).await?;
        let actor = fresh.actor();
        require(actor, "permissions.assign")?;
        not_self(actor, user_id)?;
        same_guard(actor, guard)?;
        let ids = permission_ids(&txn, guard, &[name.to_owned()]).await?;
        grantable(actor, guard, ids.keys())?;
        user_perm::Entity::delete_many()
            .filter(user_perm::Column::PermissionId.eq(ids[name]))
            .filter(user_perm::Column::ModelType.eq(USER_MODEL))
            .filter(user_perm::Column::ModelId.eq(user_id.to_string()))
            .exec(&txn)
            .await?;
        self.commit(txn).await?;
        Ok(())
    }

    /// Enables or disables a user. Disabling signs them out everywhere and revokes their tokens.
    pub async fn set_user_active(&self, actor: Actor<'_>, user_id: i64, active: bool) -> AppResult<()> {
        self.set_user_active_if(actor, user_id, active, None).await
    }

    /// Like [`Self::set_user_active`], but only if the user is still `expected`
    /// active or not (what the editor saw). Users holding protected roles or
    /// super-admin roles (in any guard) are managed only by super admins of every guard.
    pub async fn set_user_active_if(
        &self,
        actor: Actor<'_>,
        user_id: i64,
        active: bool,
        expected: Option<bool>,
    ) -> AppResult<()> {
        let (txn, fresh) = self.begin(actor).await?;
        let actor = fresh.actor();
        require(actor, "users.update")?;
        // Comparing with `expected` reveals the current status.
        if expected.is_some() {
            require_any(actor, &["users.view", "users.view_any"])?;
        }
        not_self(actor, user_id)?;
        let target = lock_user(&txn, user_id).await?;
        guard_target(&txn, actor, user_id).await?;
        if expected.is_some_and(|e| e != target.is_active) {
            return stale();
        }
        let guards = super_admin_guards(&txn, user_id).await?;
        let mut model: user::ActiveModel = target.into();
        model.is_active = Set(active);
        model.update(&txn).await?;
        if !active {
            crate::credentials::revoke_all(&txn, user_id, true).await?;
        }
        for guard in &guards {
            ensure_super_admin_remains(&txn, guard).await?;
        }
        self.commit(txn).await?;
        Ok(())
    }

    /// Deletes a user with their role and permission assignments (tokens cascade).
    pub async fn delete_user(&self, actor: Actor<'_>, user_id: i64) -> AppResult<()> {
        let (txn, fresh) = self.begin(actor).await?;
        let actor = fresh.actor();
        require(actor, "users.delete")?;
        not_self(actor, user_id)?;
        lock_user(&txn, user_id).await?;
        guard_target(&txn, actor, user_id).await?;
        let guards = super_admin_guards(&txn, user_id).await?;
        let model_id = user_id.to_string();
        user_role::Entity::delete_many()
            .filter(user_role::Column::ModelType.eq(USER_MODEL))
            .filter(user_role::Column::ModelId.eq(&model_id))
            .exec(&txn)
            .await?;
        user_perm::Entity::delete_many()
            .filter(user_perm::Column::ModelType.eq(USER_MODEL))
            .filter(user_perm::Column::ModelId.eq(&model_id))
            .exec(&txn)
            .await?;
        user::Entity::delete_by_id(user_id).exec(&txn).await?;
        for guard in &guards {
            ensure_super_admin_remains(&txn, guard).await?;
        }
        self.commit(txn).await?;
        Ok(())
    }

    /// Creates a user (and optionally makes them a super admin of `super_admin_guard`)
    /// in one transaction: on any failure, nothing is created.
    pub async fn create_user(&self, actor: Actor<'_>, new: NewUser<'_>) -> AppResult<user::Model> {
        let (txn, fresh) = self.begin(actor).await?;
        let user = create_user_in(&txn, fresh.actor(), new).await?;
        self.commit(txn).await?;
        Ok(user)
    }

    /// Creates a user holding `role_ids` (roles of `guard`), all or nothing. Assigning
    /// roles needs `roles.assign` and follows [`Self::set_user_roles`]'s rules.
    pub async fn create_user_with_roles(
        &self,
        actor: Actor<'_>,
        new: NewUser<'_>,
        guard: &str,
        role_ids: &[i64],
    ) -> AppResult<user::Model> {
        let (txn, fresh) = self.begin(actor).await?;
        let actor = fresh.actor();
        let user = create_user_in(&txn, actor, new).await?;
        if !role_ids.is_empty() {
            apply_roles(&txn, actor, user.id, guard, role_ids).await?;
        }
        self.commit(txn).await?;
        Ok(user)
    }

    /// Replaces the roles a user holds in `guard` with `role_ids`. Every role added
    /// or removed is checked like [`Self::assign_role`] / [`Self::revoke_role`].
    pub async fn set_user_roles(&self, actor: Actor<'_>, user_id: i64, guard: &str, role_ids: &[i64]) -> AppResult<()> {
        self.set_user_roles_if(actor, user_id, guard, role_ids, None).await
    }

    /// Like [`Self::set_user_roles`], but only if the user still holds exactly
    /// `expected` in `guard` (what the editor started from), so a stale draft
    /// cannot restore a role revoked meanwhile.
    pub async fn set_user_roles_if(
        &self,
        actor: Actor<'_>,
        user_id: i64,
        guard: &str,
        role_ids: &[i64],
        expected: Option<&[i64]>,
    ) -> AppResult<()> {
        let (txn, fresh) = self.begin(actor).await?;
        let actor = fresh.actor();
        // Authorize before reading anything about the target: unauthorized callers
        // learn nothing (existence, current roles) from the answer.
        require(actor, "roles.assign")?;
        // Comparing with `expected` reveals the current roles: only for callers who may read them.
        if expected.is_some() {
            require_any(actor, &["users.view", "users.view_any"])?;
        }
        same_guard(actor, guard)?;
        not_self(actor, user_id)?;
        lock_user(&txn, user_id).await?;
        if let Some(expected) = expected
            && expected.iter().copied().collect::<BTreeSet<_>>() != held_roles(&txn, user_id, guard).await?
        {
            return stale();
        }
        apply_roles(&txn, actor, user_id, guard, role_ids).await?;
        self.commit(txn).await?;
        Ok(())
    }

    /// Changes a user's name and email (`users.update`; super admins' profiles
    /// only by super admins). Users may edit their own profile.
    pub async fn update_profile(
        &self,
        actor: Actor<'_>,
        user_id: i64,
        name: &str,
        email: &str,
    ) -> AppResult<user::Model> {
        self.update_profile_if(actor, user_id, name, email, None, false).await
    }

    /// Like [`Self::update_profile`], but only if the profile still has the
    /// `expected` `(name, email)` the editor started from: a stale draft never
    /// restores an old sign-in email. Users changing their own email must have
    /// confirmed their password (`password_confirmed`).
    pub async fn update_profile_if(
        &self,
        actor: Actor<'_>,
        user_id: i64,
        name: &str,
        email: &str,
        expected: Option<(&str, &str)>,
        password_confirmed: bool,
    ) -> AppResult<user::Model> {
        let (txn, fresh) = self.begin(actor).await?;
        let actor = fresh.actor();
        let own = matches!(actor, Actor::User(p) if p.id == user_id.to_string());
        // Authorization first: nothing about the target leaks to callers who may not edit it.
        if !own {
            require(actor, "users.update")?;
            // Comparing with `expected` reveals the current profile.
            if expected.is_some() {
                require_any(actor, &["users.view", "users.view_any"])?;
            }
        }
        let email = valid_profile(name, email)?;
        let target = lock_user(&txn, user_id).await?;
        if !own {
            guard_target(&txn, actor, user_id).await?;
        }
        let email_changed = email != target.email;
        // The email is a credential: changing someone else's is a takeover.
        if !own && email_changed {
            covers(&txn, actor, user_id).await?;
        }
        if let Some((name, email)) = expected
            && (name != target.name || email.trim().to_lowercase() != target.email)
        {
            return stale();
        }
        // Changing one's own sign-in email needs a confirmed password (decided here,
        // against the stored email, so concurrent requests cannot slip past it).
        if own && email_changed && !password_confirmed {
            return invalid("current_password", "is required to change your email");
        }
        if user::Entity::find()
            .filter(user::Column::Email.eq(&email))
            .filter(user::Column::Id.ne(user_id))
            .one(&txn)
            .await?
            .is_some()
        {
            return invalid("email", "has already been taken");
        }
        let mut model: user::ActiveModel = target.into();
        model.name = Set(name.trim().to_owned());
        model.email = Set(email);
        // A new address is not verified, whoever changed it.
        if email_changed {
            model.email_verified_at = Set(None);
        }
        model.updated_at = Set(chrono::Utc::now());
        let mut user = model.update(&txn).await.map_err(email_conflict)?;
        // A credential changed: other sessions and tokens end (the caller's own
        // session is re-issued by the handler with the new version).
        if email_changed {
            user.auth_version = crate::credentials::revoke_all(&txn, user_id, true).await?;
        }
        self.commit(txn).await?;
        Ok(user)
    }

    /// Read-only preflight of [`Self::reset_password`] (no lock): lets callers
    /// refuse before costly hashing. The change itself checks everything again.
    pub async fn check_reset_password(&self, actor: Actor<'_>, user_id: i64) -> AppResult<()> {
        require(actor, "users.update")?;
        not_self(actor, user_id)?;
        // Before the lookup: callers who may not reset anyone learn nothing about ids.
        covers(self.db, actor, user_id).await?;
        user::Entity::find_by_id(user_id).one(self.db).await?.ok_or(AppError::NotFound)?;
        Ok(())
    }

    /// Read-only preflight of [`Self::create_user_with_roles`] (no lock): input,
    /// duplicate email and role checks before costly hashing.
    pub async fn check_new_user(
        &self,
        actor: Actor<'_>,
        name: &str,
        email: &str,
        guard: &str,
        role_ids: &[i64],
    ) -> AppResult<()> {
        require(actor, "users.create")?;
        if !role_ids.is_empty() {
            require(actor, "roles.assign")?;
            same_guard(actor, guard)?;
        }
        let email = valid_profile(name, email)?;
        if user::Entity::find().filter(user::Column::Email.eq(&email)).one(self.db).await?.is_some() {
            return invalid("email", "has already been taken");
        }
        check_role_ids(self.db, guard, role_ids).await
    }

    /// Whether `actor` is a super admin of every guard (may replace credentials).
    pub async fn covers_all_guards(&self, actor: Actor<'_>) -> AppResult<bool> {
        Ok(covers(self.db, actor, 0).await.is_ok())
    }

    /// Sets another user's password (`users.update`; a super admin's only by a super
    /// admin). Their sessions end and their API tokens are revoked. Users change
    /// their own password with the current one (`/auth/password`), never here.
    pub async fn reset_password(&self, actor: Actor<'_>, user_id: i64, password_hash: String) -> AppResult<()> {
        let (txn, fresh) = self.begin(actor).await?;
        let actor = fresh.actor();
        require(actor, "users.update")?;
        not_self(actor, user_id)?;
        // Choosing someone's password is signing in as them: never for an
        // account with more access than the actor, in any guard.
        covers(&txn, actor, user_id).await?;
        let target = lock_user(&txn, user_id).await?;
        let mut model: user::ActiveModel = target.into();
        model.password = Set(password_hash);
        model.update(&txn).await?;
        crate::credentials::revoke_all(&txn, user_id, true).await?;
        self.commit(txn).await?;
        Ok(())
    }

    /// Creates declared permissions and marks undeclared ones orphaned (never renames or deletes).
    pub async fn sync_permissions(
        &self,
        declared: &BTreeMap<(String, String), Option<String>>,
    ) -> AppResult<SyncReport> {
        let (txn, _) = self.begin(Actor::System).await?;
        let existing = permission::Entity::find().all(&txn).await?;
        let mut report = SyncReport::default();
        for ((guard, name), group) in declared {
            match existing.iter().find(|p| &p.guard_name == guard && &p.name == name) {
                Some(found) if found.orphaned || &found.group != group => {
                    let mut model: permission::ActiveModel = found.clone().into();
                    model.orphaned = Set(false);
                    model.group = Set(group.clone());
                    model.update(&txn).await?;
                }
                Some(_) => {}
                None => {
                    permission::ActiveModel {
                        name: Set(name.clone()),
                        guard_name: Set(guard.clone()),
                        group: Set(group.clone()),
                        orphaned: Set(false),
                        ..Default::default()
                    }
                    .insert(&txn)
                    .await?;
                    report.created.push(format!("{guard}:{name}"));
                }
            }
        }
        for stale in
            existing.iter().filter(|p| !p.orphaned && !declared.contains_key(&(p.guard_name.clone(), p.name.clone())))
        {
            let mut model: permission::ActiveModel = stale.clone().into();
            model.orphaned = Set(true);
            model.update(&txn).await?;
            report.orphaned.push(format!("{}:{}", stale.guard_name, stale.name));
        }
        self.commit(txn).await?;
        Ok(report)
    }
}

/// Creates a user inside `txn` (see [`RbacService::create_user`]).
async fn create_user_in(txn: &DatabaseTransaction, actor: Actor<'_>, new: NewUser<'_>) -> AppResult<user::Model> {
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

/// The actor may replace another user's credentials (password, email), which
/// is signing in as them, everywhere: the same credentials open the admin
/// panel and issue API tokens, and an application may grant access by identity
/// (gates, ownership) without any role. So only a super admin of *every* guard
/// may do it; [`Actor::System`] always may.
async fn covers<C: ConnectionTrait>(txn: &C, actor: Actor<'_>, _user_id: i64) -> AppResult<()> {
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

fn stale<T>() -> AppResult<T> {
    let mut errors = ValidationErrors::new();
    errors.add_form("This was changed by someone else meanwhile. Reload and try again.");
    Err(errors.into())
}

/// Active (not orphaned) permissions granted to a role, by name.
async fn active_permissions<C: ConnectionTrait>(db: &C, role_id: i64) -> AppResult<BTreeMap<String, i64>> {
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
async fn held_roles<C: ConnectionTrait>(db: &C, user_id: i64, guard: &str) -> AppResult<BTreeSet<i64>> {
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

/// Validated, normalized email for a profile with `name`.
fn valid_profile(name: &str, email: &str) -> AppResult<String> {
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
fn valid_email(email: &str) -> bool {
    let Some((local, domain)) = email.split_once('@') else { return false };
    !local.is_empty()
        && !domain.is_empty()
        && !domain.contains('@')
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !email.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// Makes `role_ids` the user's roles in `guard`: each added or removed role is
/// checked (`roles.assign`, same guard, protected roles by super admins, only
/// permissions the actor holds), and a guard keeps an active super admin.
async fn apply_roles(
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

/// Largest list of ids or names one change accepts.
const MAX_ITEMS: usize = 1000;

fn require_any(actor: Actor<'_>, permissions: &[&str]) -> AppResult<()> {
    match actor {
        Actor::System => Ok(()),
        Actor::User(p) if permissions.iter().any(|name| p.can(name)) => Ok(()),
        Actor::User(_) => forbidden(),
    }
}

/// A role of the actor's guard (other guards' roles look missing, like unknown ids).
async fn find_role<C: ConnectionTrait>(db: &C, actor: Actor<'_>, role_id: i64) -> AppResult<role::Model> {
    let mut find = role::Entity::find_by_id(role_id);
    if let Actor::User(p) = actor {
        find = find.filter(role::Column::GuardName.eq(&p.guard));
    }
    find.one(db).await?.ok_or(AppError::NotFound)
}

/// The user row, locked until commit on PostgreSQL (SQLite writers are already serialized),
/// so checks and writes see the same version even against writers outside RBAC.
async fn lock_user(txn: &DatabaseTransaction, user_id: i64) -> AppResult<user::Model> {
    let mut find = user::Entity::find_by_id(user_id);
    if txn.get_database_backend() == DbBackend::Postgres {
        find = find.lock_exclusive();
    }
    find.one(txn).await?.ok_or(AppError::NotFound)
}

/// Assigning a role that carries orphaned grants would hand them out, dormant,
/// to come back on a redeclaration nobody checked: super admins only.
async fn dormant_grants<C: ConnectionTrait>(db: &C, actor: Actor<'_>, role_id: i64) -> AppResult<()> {
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
async fn guard_target(txn: &DatabaseTransaction, actor: Actor<'_>, user_id: i64) -> AppResult<()> {
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

/// A unique violation on `users.email` (a concurrent writer outside RBAC) is a 422.
fn email_conflict(err: sea_orm::DbErr) -> AppError {
    if matches!(err.sql_err(), Some(sea_orm::SqlErr::UniqueConstraintViolation(_))) {
        ValidationErrors::field("email", "has already been taken").into()
    } else {
        err.into()
    }
}

/// Role ids exist in `guard` (and are not too many).
async fn check_role_ids<C: ConnectionTrait>(db: &C, guard: &str, role_ids: &[i64]) -> AppResult<()> {
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

fn require(actor: Actor<'_>, permission: &str) -> AppResult<()> {
    match actor {
        Actor::System => Ok(()),
        Actor::User(p) if p.can(permission) => Ok(()),
        Actor::User(_) => forbidden(),
    }
}

/// Users manage only their own guard's roles and permissions.
fn same_guard(actor: Actor<'_>, guard: &str) -> AppResult<()> {
    match actor {
        Actor::User(p) if p.guard != guard => forbidden(),
        _ => Ok(()),
    }
}

fn not_self(actor: Actor<'_>, user_id: i64) -> AppResult<()> {
    match actor {
        Actor::User(p) if p.id == user_id.to_string() => forbidden(),
        _ => Ok(()),
    }
}

fn is_super_admin_actor(actor: Actor<'_>) -> bool {
    match actor {
        Actor::System => true,
        Actor::User(p) => p.super_admin && p.token_abilities.as_ref().is_none_or(|a| a.contains("*")),
    }
}

fn protected(actor: Actor<'_>, role: &role::Model) -> AppResult<()> {
    if role.is_protected() && !is_super_admin_actor(actor) { forbidden() } else { Ok(()) }
}

/// The actor must hold every permission it grants or takes away.
fn grantable<'p>(actor: Actor<'_>, guard: &str, permissions: impl IntoIterator<Item = &'p String>) -> AppResult<()> {
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

/// Resolves permission names of `guard`; unknown or orphaned names are a 422.
async fn permission_ids<C: ConnectionTrait>(db: &C, guard: &str, names: &[String]) -> AppResult<BTreeMap<String, i64>> {
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

async fn holds_role<C: ConnectionTrait>(db: &C, model_id: &str, role_id: i64) -> AppResult<bool> {
    Ok(user_role::Entity::find_by_id((role_id, USER_MODEL.to_owned(), model_id.to_owned())).one(db).await?.is_some())
}

/// Guards in which `user_id` holds the system super-admin role.
async fn super_admin_guards<C: ConnectionTrait>(db: &C, user_id: i64) -> AppResult<Vec<String>> {
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

/// Advisory lock id serializing every RBAC change on PostgreSQL.
const RBAC_LOCK: i64 = 0x7262_6163_5f6c_6f63;

/// Serializes RBAC changes for the rest of the transaction: a transaction-scoped
/// advisory lock on PostgreSQL; on SQLite the [`rustcms_core::db::begin_write`]
/// transaction already holds the database write lock. Every [`RbacService`]
/// change takes it first, so checks and writes of concurrent changes never
/// interleave (READ COMMITTED reads after the lock see every earlier change).
pub async fn lock<C: ConnectionTrait>(db: &C) -> AppResult<()> {
    if db.get_database_backend() == DbBackend::Postgres {
        db.execute_unprepared(&format!("SELECT pg_advisory_xact_lock({RBAC_LOCK})")).await?;
    }
    Ok(())
}

/// The actor as of now, read under the RBAC lock.
struct Fresh(Option<Principal>);

impl Fresh {
    fn actor(&self) -> Actor<'_> {
        self.0.as_ref().map_or(Actor::System, Actor::User)
    }
}

async fn ensure_super_admin_remains<C: ConnectionTrait>(db: &C, guard: &str) -> AppResult<()> {
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
        .count(db)
        .await?;
    if active == 0 {
        let mut errors = ValidationErrors::new();
        errors.add_form(format!("At least one active super admin is required for the `{guard}` guard."));
        return Err(errors.into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use tokio::sync::oneshot;

    use super::*;

    fn grants(permission: &str) -> Grants {
        Grants { permissions: HashSet::from([permission.to_owned()]), super_admin: false }
    }

    #[tokio::test]
    async fn eviction_wins_over_an_in_flight_load() {
        let resolver = PermissionResolver::new(Duration::from_secs(600));
        let (release, released) = oneshot::channel::<()>();
        let released = Mutex::new(Some(released));

        // The first load reads the old grants, then stalls before caching them;
        // any later load sees the committed change.
        let stale = resolver.get_or_load("admin", 1, || {
            let wait = released.lock().unwrap().take();
            async move {
                match wait {
                    Some(wait) => {
                        wait.await.ok();
                        Ok(grants("old"))
                    }
                    None => Ok(grants("new")),
                }
            }
        });
        // …while an RBAC change commits and evicts.
        let change = async {
            tokio::task::yield_now().await;
            resolver.evict_all();
            release.send(()).unwrap();
        };
        let (stale, ()) = tokio::join!(stale, change);
        stale.unwrap();

        let fresh = resolver.get_or_load("admin", 1, || async { Ok(grants("newest")) }).await.unwrap();
        assert!(!fresh.permissions.contains("old"), "a stale in-flight load must not survive eviction");
    }

    #[tokio::test]
    async fn caches_within_a_generation() {
        let resolver = PermissionResolver::new(Duration::from_secs(600));
        resolver.get_or_load("admin", 1, || async { Ok(grants("a")) }).await.unwrap();
        let cached = resolver.get_or_load("admin", 1, || async { Ok(grants("b")) }).await.unwrap();
        assert!(cached.permissions.contains("a"));
        resolver.evict_all();
        let reloaded = resolver.get_or_load("admin", 1, || async { Ok(grants("b")) }).await.unwrap();
        assert!(reloaded.permissions.contains("b"));
    }
}
