//! Grants of a user per guard, loaded from the database and cached per
//! generation (evicted on every RBAC change).

use super::*;

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

/// Loads retried before giving up on caching under constant churn.
pub(super) const LOAD_ATTEMPTS: usize = 3;

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
