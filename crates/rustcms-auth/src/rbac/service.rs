//! `RbacService`: every change to users, roles and permissions, under the
//! RBAC lock and with the actor's current grants.

use super::*;

use rustcms_core::audit::{AuditActor, AuditContext, AuditEntry};
use serde_json::json;

/// The guard a change applies to.
#[derive(serde::Serialize)]
struct GuardMeta<'a> {
    guard: &'a str,
}

/// How many permissions a sync created and orphaned.
#[derive(serde::Serialize)]
struct SyncMeta {
    created: usize,
    orphaned: usize,
    restored: usize,
    regrouped: usize,
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
    audit: Option<AuditContext>,
}

impl<'a> RbacService<'a> {
    /// Service over `db`, evicting `resolver` after each change.
    pub fn new(db: &'a DatabaseConnection, resolver: &'a PermissionResolver) -> Self {
        Self { db, resolver, audit: None }
    }

    /// Records changes with this actor and request in the audit log (without
    /// it: the acting principal, or the application for [`Actor::System`]).
    pub fn audited(mut self, audit: AuditContext) -> Self {
        self.audit = Some(audit);
        self
    }

    /// Records `entry` in the transaction of the change.
    async fn audit(&self, txn: &DatabaseTransaction, actor: Actor<'_>, entry: AuditEntry) -> AppResult<()> {
        let context = self.audit.clone().unwrap_or_else(|| match actor {
            Actor::User(principal) => AuditContext::new(AuditActor::user(principal), Default::default()),
            Actor::System => AuditContext::system(None),
        });
        rustcms_core::audit::record(txn, &context, entry).await
    }

    /// Commits and always evicts cached grants: if COMMIT reached the server but
    /// its answer was lost, the change may be applied, so the cache must not outlive it.
    async fn commit(&self, txn: DatabaseTransaction) -> AppResult<()> {
        // Every instance keys its cache by this counter: the change and its new
        // generation commit together.
        super::resolver::advance_shared_generation(&txn).await?;
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
        begin_locked(self.db, actor).await
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
        let names: Vec<&String> = wanted.keys().collect();
        let entry = AuditEntry::new("role.created", "role").key(role.id).changes(json!({
            "name": [null, role.name], "guard": [null, role.guard_name], "permissions": [null, names],
        }));
        self.audit(&txn, actor, entry).await?;
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
        if wanted_names != current_names {
            let entry = AuditEntry::new("role.permissions_changed", "role")
                .key(role.id)
                .changes(json!({ "permissions": [current_names, wanted_names] }));
            self.audit(&txn, actor, entry).await?;
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
        let entry = AuditEntry::new("role.deleted", "role")
            .key(role.id)
            .changes(json!({ "name": [role.name, null], "guard": [role.guard_name, null] }));
        self.audit(&txn, actor, entry).await?;
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
            let before = held_roles(&txn, user_id, &role.guard_name).await?;
            dormant_grants(&txn, actor, role.id).await?;
            user_role::ActiveModel {
                role_id: Set(role.id),
                model_type: Set(USER_MODEL.into()),
                model_id: Set(user_id.to_string()),
            }
            .insert(&txn)
            .await?;
            let entry = AuditEntry::new("user.roles_changed", "user")
                .key(user_id)
                .changes(json!({ "roles": [before, held_roles(&txn, user_id, &role.guard_name).await?] }))
                .meta(GuardMeta { guard: &role.guard_name });
            self.audit(&txn, actor, entry).await?;
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
        let before = held_roles(&txn, user_id, &role.guard_name).await?;
        let removed = user_role::Entity::delete_many()
            .filter(user_role::Column::RoleId.eq(role.id))
            .filter(user_role::Column::ModelType.eq(USER_MODEL))
            .filter(user_role::Column::ModelId.eq(user_id.to_string()))
            .exec(&txn)
            .await?;
        if role.is_super_admin() {
            ensure_super_admin_remains(&txn, &role.guard_name).await?;
        }
        if removed.rows_affected > 0 {
            let entry = AuditEntry::new("user.roles_changed", "user")
                .key(user_id)
                .changes(json!({ "roles": [before, held_roles(&txn, user_id, &role.guard_name).await?] }))
                .meta(GuardMeta { guard: &role.guard_name });
            self.audit(&txn, actor, entry).await?;
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
            let before = direct_permissions(&txn, user_id, guard).await?;
            user_perm::ActiveModel {
                permission_id: Set(permission_id),
                model_type: Set(USER_MODEL.into()),
                model_id: Set(user_id.to_string()),
            }
            .insert(&txn)
            .await?;
            let entry = AuditEntry::new("user.permissions_changed", "user")
                .key(user_id)
                .changes(json!({ "permissions": [before, direct_permissions(&txn, user_id, guard).await?] }))
                .meta(GuardMeta { guard });
            self.audit(&txn, actor, entry).await?;
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
        let before = direct_permissions(&txn, user_id, guard).await?;
        let removed = user_perm::Entity::delete_many()
            .filter(user_perm::Column::PermissionId.eq(ids[name]))
            .filter(user_perm::Column::ModelType.eq(USER_MODEL))
            .filter(user_perm::Column::ModelId.eq(user_id.to_string()))
            .exec(&txn)
            .await?;
        if removed.rows_affected > 0 {
            let entry = AuditEntry::new("user.permissions_changed", "user")
                .key(user_id)
                .changes(json!({ "permissions": [before, direct_permissions(&txn, user_id, guard).await?] }))
                .meta(GuardMeta { guard });
            self.audit(&txn, actor, entry).await?;
        }
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
        let was_active = target.is_active;
        let mut model: user::ActiveModel = target.into();
        model.is_active = Set(active);
        model.update(&txn).await?;
        if was_active != active {
            let event = if active { "user.activated" } else { "user.deactivated" };
            self.audit(&txn, actor, AuditEntry::new(event, "user").key(user_id)).await?;
        }
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
        let target = lock_user(&txn, user_id).await?;
        guard_target(&txn, actor, user_id).await?;
        let guards = super_admin_guards(&txn, user_id).await?;
        let entry = AuditEntry::new("user.deleted", "user")
            .key(user_id)
            .changes(json!({ "name": [target.name, null], "email": [target.email, null] }));
        self.audit(&txn, actor, entry).await?;
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
        let super_admin = new.super_admin_guard.map(str::to_owned);
        let user = create_user_in(&txn, fresh.actor(), new).await?;
        let mut changes = json!({ "name": [null, user.name], "email": [null, user.email] });
        if let Some(guard) = super_admin {
            changes["super_admin"] = json!([null, guard]);
        }
        self.audit(&txn, fresh.actor(), AuditEntry::new("user.created", "user").key(user.id).changes(changes)).await?;
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
        let mut changes = json!({ "name": [null, user.name], "email": [null, user.email] });
        if !role_ids.is_empty() {
            apply_roles(&txn, actor, user.id, guard, role_ids).await?;
            changes["roles"] = json!([null, held_roles(&txn, user.id, guard).await?]);
        }
        let entry = AuditEntry::new("user.created", "user").key(user.id).changes(changes).meta(GuardMeta { guard });
        self.audit(&txn, actor, entry).await?;
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
        let before = held_roles(&txn, user_id, guard).await?;
        apply_roles(&txn, actor, user_id, guard, role_ids).await?;
        let after = held_roles(&txn, user_id, guard).await?;
        if before != after {
            let entry = AuditEntry::new("user.roles_changed", "user")
                .key(user_id)
                .changes(json!({ "roles": [before, after] }))
                .meta(GuardMeta { guard });
            self.audit(&txn, actor, entry).await?;
        }
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
        let before = json!({ "name": target.name, "email": target.email });
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
        let after = json!({ "name": user.name, "email": user.email });
        let changes = rustcms_core::audit::diff(before.as_object(), after.as_object(), &|_| false);
        if changes.as_object().is_some_and(|c| !c.is_empty()) {
            self.audit(&txn, actor, AuditEntry::new("user.updated", "user").key(user_id).changes(changes)).await?;
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
        self.audit(&txn, actor, AuditEntry::new("user.password_reset", "user").key(user_id)).await?;
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
        // Declared again: its grants apply again (or only its group changed).
        let (mut restored, mut regrouped) = (0, 0);
        for ((guard, name), group) in declared {
            match existing.iter().find(|p| &p.guard_name == guard && &p.name == name) {
                Some(found) if found.orphaned || &found.group != group => {
                    if found.orphaned {
                        restored += 1;
                    } else {
                        regrouped += 1;
                    }
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
        if !report.created.is_empty() || !report.orphaned.is_empty() || restored + regrouped > 0 {
            let entry = AuditEntry::new("permission.synced", "permission").meta(SyncMeta {
                created: report.created.len(),
                orphaned: report.orphaned.len(),
                restored,
                regrouped,
            });
            self.audit(&txn, Actor::System, entry).await?;
        }
        self.commit(txn).await?;
        Ok(report)
    }
}

/// Opens a write transaction holding the RBAC lock (the first lock of the
/// transaction) and reloads `actor`'s grants inside it.
async fn begin_locked(db: &DatabaseConnection, actor: Actor<'_>) -> AppResult<(DatabaseTransaction, Fresh)> {
    let txn = rustcms_core::db::begin_write(db).await?;
    if txn.get_database_backend() == DbBackend::Postgres {
        // A stuck lock holder must not pin every RBAC request (and the pool) forever
        // (MySQL: `begin_write` sets the same wait).
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
            if txn.get_database_backend() != DbBackend::Sqlite {
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

/// Opens a write transaction for a change `principal` makes outside RBAC
/// (settings): it holds the RBAC lock, so no role or permission change runs
/// meanwhile, and returns the principal as it is now (active, same
/// credentials, current grants), or 403. Use the returned principal to
/// authorize the change.
pub async fn begin_as(db: &DatabaseConnection, principal: &Principal) -> AppResult<(DatabaseTransaction, Principal)> {
    let (txn, fresh) = begin_locked(db, Actor::User(principal)).await?;
    let current = fresh.0.ok_or(AppError::Forbidden)?;
    Ok((txn, current))
}

pub(super) fn forbidden<T>() -> AppResult<T> {
    Err(AppError::Forbidden)
}

pub(super) fn invalid<T>(field: &str, message: &str) -> AppResult<T> {
    Err(ValidationErrors::field(field, message).into())
}

/// The actor as of now, read under the RBAC lock.
pub(super) struct Fresh(Option<Principal>);

impl Fresh {
    fn actor(&self) -> Actor<'_> {
        self.0.as_ref().map_or(Actor::System, Actor::User)
    }
}
