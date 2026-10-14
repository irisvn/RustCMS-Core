//! Several application instances on one database: what one commits, the
//! others see on their next request. All databases.

mod support;

use rustcms_auth::{API_GUARD, Actor, RbacService};
use rustcms_migration::Migrator;
use rustcms_testing::{TestApp, TestDb, db_test};
use support::{auth, build, principal, setup};

db_test!(rbac_changes_reach_every_instance_at_once, migrate = Migrator, |db: TestDb| async move {
    let pool = db.reopen().await;
    let (a, seed) = setup(db).await;
    // A second instance: same configuration (same APP_KEY), its own pool and caches.
    let b = TestApp::with_config(build, a.state().config().clone(), pool).await;
    assert!(principal(&b, API_GUARD, &seed.reader).await.permissions.contains("posts.view"), "cached on B");

    // Instance A takes the permission away.
    let (state_a, state_b) = (auth(&a), auth(&b));
    let rbac = RbacService::new(a.state().db(), &state_a.resolver);
    rbac.set_role_permissions(Actor::System, seed.reader_role.id, &[]).await.unwrap();

    // B's cache entry is from an older generation: never used again.
    assert!(!principal(&b, API_GUARD, &seed.reader).await.permissions.contains("posts.view"));
    // And B's own changes reach A the same way.
    let warm = principal(&a, API_GUARD, &seed.reader).await;
    assert!(warm.permissions.is_empty());
    RbacService::new(b.state().db(), &state_b.resolver)
        .set_role_permissions(Actor::System, seed.reader_role.id, &["posts.view".to_owned()])
        .await
        .unwrap();
    assert!(principal(&a, API_GUARD, &seed.reader).await.permissions.contains("posts.view"));
});
