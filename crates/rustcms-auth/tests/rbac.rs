//! Adversarial RBAC scenarios: every way to escalate privileges must fail.

mod support;

use std::collections::HashSet;
use std::future::Future;

use rustcms_auth::entities::role;
use rustcms_auth::{ADMIN_GUARD, API_GUARD, Actor, RbacService};
use rustcms_core::{AppError, Principal};
use rustcms_migration::Migrator;
use rustcms_testing::{Factory, TestDb, UserFactory, db_test};
use sea_orm::EntityTrait;
use support::{auth, principal, setup, super_admin_role};

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|n| (*n).to_owned()).collect()
}

fn forbidden<T: std::fmt::Debug>(result: Result<T, AppError>) {
    assert!(matches!(result, Err(AppError::Forbidden)), "expected 403, got {result:?}");
}

fn invalid<T: std::fmt::Debug>(result: Result<T, AppError>) {
    assert!(matches!(result, Err(AppError::Validation(_))), "expected 422, got {result:?}");
}

db_test!(assignment_needs_its_own_permission, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup(db).await;
    let auth = auth(&app);
    let rbac = RbacService::new(app.state().db(), &auth.resolver);

    // users.update alone does not allow assigning roles, even a role whose
    // permissions the actor holds. Grants live in the database (the service
    // reloads them), so the actor gets them through a real role.
    let without = names(&["users.view", "users.update", "posts.view", "posts.delete"]);
    let mut with = without.clone();
    with.push("roles.assign".into());
    for (name, permissions, allowed) in [("updater", without, false), ("assigner", with, true)] {
        let role = rbac.create_role(Actor::System, ADMIN_GUARD, name, &permissions).await.unwrap();
        let user = UserFactory::new().create(app.state().db()).await;
        rbac.assign_role(Actor::System, user.id, role.id).await.unwrap();
        let actor = principal(&app, ADMIN_GUARD, &user).await;
        let result = rbac.assign_role(Actor::User(&actor), seed.member.id, seed.moderator_role.id).await;
        if allowed {
            result.expect("reverse check: with roles.assign the same assignment works");
            rbac.revoke_role(Actor::System, seed.member.id, seed.moderator_role.id).await.unwrap();
        } else {
            forbidden(result);
        }
    }

    // roles.assign does not allow assigning a role with permissions the actor lacks (posts.delete).
    let editor = principal(&app, ADMIN_GUARD, &seed.editor).await;
    forbidden(rbac.assign_role(Actor::User(&editor), seed.member.id, seed.moderator_role.id).await);

    // A role within the editor's own permissions is fine.
    let viewer = rbac.create_role(Actor::System, ADMIN_GUARD, "viewer", &names(&["posts.view"])).await.unwrap();
    rbac.assign_role(Actor::User(&editor), seed.member.id, viewer.id).await.unwrap();
    assert!(principal(&app, ADMIN_GUARD, &seed.member).await.permissions.contains("posts.view"));

    // Privileged roles need a super admin.
    let super_admin = super_admin_role(&app, ADMIN_GUARD).await;
    forbidden(rbac.assign_role(Actor::User(&editor), seed.member.id, super_admin.id).await);
    let admin = principal(&app, ADMIN_GUARD, &seed.admin).await;
    rbac.assign_role(Actor::User(&admin), seed.member.id, super_admin.id).await.unwrap();
});

db_test!(nobody_changes_their_own_access, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup(db).await;
    let auth = auth(&app);
    let rbac = RbacService::new(app.state().db(), &auth.resolver);
    let editor = principal(&app, ADMIN_GUARD, &seed.editor).await;
    let viewer = rbac.create_role(Actor::System, ADMIN_GUARD, "viewer", &names(&["posts.view"])).await.unwrap();

    forbidden(rbac.assign_role(Actor::User(&editor), seed.editor.id, viewer.id).await);
    forbidden(rbac.revoke_role(Actor::User(&editor), seed.editor.id, seed.editor_role.id).await);
    forbidden(rbac.grant_permission(Actor::User(&editor), seed.editor.id, ADMIN_GUARD, "posts.view").await);
    forbidden(rbac.set_user_active(Actor::User(&editor), seed.editor.id, false).await);

    // Editing a role one holds is refused even for permissions one has.
    forbidden(rbac.set_role_permissions(Actor::User(&editor), seed.editor_role.id, &names(&["posts.view"])).await);

    // Even super admins cannot change their own roles.
    let admin = principal(&app, ADMIN_GUARD, &seed.admin).await;
    forbidden(rbac.revoke_role(Actor::User(&admin), seed.admin.id, super_admin_role(&app, ADMIN_GUARD).await.id).await);
});

db_test!(role_definitions_cannot_be_used_to_escalate, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup(db).await;
    let auth = auth(&app);
    let rbac = RbacService::new(app.state().db(), &auth.resolver);
    let editor = principal(&app, ADMIN_GUARD, &seed.editor).await;
    let viewer = rbac.create_role(Actor::System, ADMIN_GUARD, "viewer", &names(&["posts.view"])).await.unwrap();

    // Adding a permission the editor lacks to a role (held by someone else).
    forbidden(
        rbac.set_role_permissions(Actor::User(&editor), viewer.id, &names(&["posts.view", "posts.delete"])).await,
    );
    // Removing a permission the editor lacks from a role.
    forbidden(rbac.set_role_permissions(Actor::User(&editor), seed.moderator_role.id, &names(&["posts.view"])).await);
    // Within the editor's permissions it works.
    rbac.set_role_permissions(Actor::User(&editor), viewer.id, &names(&[])).await.unwrap();

    // System and privileged roles: super admins only; system roles never deleted.
    let system = super_admin_role(&app, ADMIN_GUARD).await;
    forbidden(rbac.set_role_permissions(Actor::User(&editor), system.id, &names(&["posts.view"])).await);
    let admin = principal(&app, ADMIN_GUARD, &seed.admin).await;
    forbidden(rbac.delete_role(Actor::User(&admin), system.id).await);
    forbidden(rbac.delete_role(Actor::System, system.id).await);
    let unchanged = role::Entity::find_by_id(system.id).one(app.state().db()).await.unwrap().unwrap();
    assert_eq!(unchanged, system, "system role untouched");

    // Unknown permission names are rejected, not silently ignored.
    invalid(rbac.set_role_permissions(Actor::System, viewer.id, &names(&["posts.everything"])).await);
});

db_test!(guards_and_tokens_bound_the_actor, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup(db).await;
    let auth = auth(&app);
    let rbac = RbacService::new(app.state().db(), &auth.resolver);

    // An admin-guard super admin cannot manage api-guard roles from the panel.
    let admin = principal(&app, ADMIN_GUARD, &seed.admin).await;
    forbidden(rbac.assign_role(Actor::User(&admin), seed.member.id, seed.reader_role.id).await);
    forbidden(rbac.create_role(Actor::User(&admin), API_GUARD, "writer", &names(&["posts.create"])).await);

    // A super admin acting through a narrow token is not a super admin for protected roles.
    let narrow = Principal { token_abilities: Some(HashSet::from(["roles.assign".to_owned()])), ..admin.clone() };
    forbidden(
        rbac.assign_role(Actor::User(&narrow), seed.member.id, super_admin_role(&app, ADMIN_GUARD).await.id).await,
    );
    let narrow = Principal { token_abilities: Some(HashSet::from(["posts.view".to_owned()])), ..admin };
    forbidden(rbac.assign_role(Actor::User(&narrow), seed.member.id, seed.moderator_role.id).await);
});

db_test!(last_super_admin_cannot_be_removed, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup(db).await;
    let auth = auth(&app);
    let rbac = RbacService::new(app.state().db(), &auth.resolver);
    let api_role = super_admin_role(&app, API_GUARD).await;

    // `admin` is the only api-guard super admin.
    invalid(rbac.revoke_role(Actor::System, seed.admin.id, api_role.id).await);
    invalid(rbac.set_user_active(Actor::System, seed.admin.id, false).await);
    invalid(rbac.delete_user(Actor::System, seed.admin.id).await);
    assert!(principal(&app, API_GUARD, &seed.admin).await.super_admin, "rolled back");

    // Non-super-admins cannot disable or delete a super admin at all.
    let editor = principal(&app, ADMIN_GUARD, &seed.editor).await;
    forbidden(rbac.set_user_active(Actor::User(&editor), seed.admin2.id, false).await);

    // Disabled super admins do not count.
    let spare = UserFactory::new().create(app.state().db()).await;
    rbac.assign_role(Actor::System, spare.id, api_role.id).await.unwrap();
    rbac.set_user_active(Actor::System, spare.id, false).await.unwrap();
    invalid(rbac.revoke_role(Actor::System, seed.admin.id, api_role.id).await);
});

db_test!(concurrent_removals_keep_one_super_admin, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup(db).await;
    let auth = auth(&app);
    let role = super_admin_role(&app, ADMIN_GUARD).await;
    // `admin` and `admin2` are the two admin-guard super admins; remove both at once.
    let first = RbacService::new(app.state().db(), &auth.resolver);
    let second = RbacService::new(app.state().db(), &auth.resolver);
    let (a, b) = tokio::join!(
        first.revoke_role(Actor::System, seed.admin.id, role.id),
        second.revoke_role(Actor::System, seed.admin2.id, role.id),
    );
    assert!(a.is_ok() != b.is_ok() || (a.is_err() && b.is_err()), "at most one succeeds: {a:?} {b:?}");
    let remaining = [principal(&app, ADMIN_GUARD, &seed.admin).await, principal(&app, ADMIN_GUARD, &seed.admin2).await]
        .iter()
        .filter(|p| p.super_admin)
        .count();
    assert!(remaining >= 1, "a super admin remains");
});

db_test!(permission_sync_is_idempotent_and_orphans, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup(db).await;
    let auth = auth(&app);
    let rbac = RbacService::new(app.state().db(), &auth.resolver);
    let again = rbac.sync_permissions(&auth.permissions.all()).await.unwrap();
    assert!(again.created.is_empty() && again.orphaned.is_empty(), "{again:?}");

    let mut declared = auth.permissions.all();
    declared.remove(&(ADMIN_GUARD.to_owned(), "posts.delete".to_owned()));
    let report = rbac.sync_permissions(&declared).await.unwrap();
    assert_eq!(report.orphaned, ["admin:posts.delete"]);

    // Orphaned permissions stop granting access, and cannot be granted.
    let moderator = UserFactory::new().create(app.state().db()).await;
    rbac.assign_role(Actor::System, moderator.id, seed.moderator_role.id).await.unwrap();
    let grants = principal(&app, ADMIN_GUARD, &moderator).await;
    assert!(grants.permissions.contains("posts.view") && !grants.permissions.contains("posts.delete"));
    invalid(rbac.grant_permission(Actor::System, moderator.id, ADMIN_GUARD, "posts.delete").await);

    // Declaring it again restores it.
    rbac.sync_permissions(&auth.permissions.all()).await.unwrap();
    assert!(principal(&app, ADMIN_GUARD, &moderator).await.permissions.contains("posts.delete"));
});

// Read-then-write transactions racing each other (and a background writer)
// must wait for the lock, not fail with "database is locked" on SQLite.
db_test!(concurrent_writers_wait_for_each_other, migrate = Migrator, |db: TestDb| async move {
    let (app, _seed) = setup(db).await;
    let mut tasks = tokio::task::JoinSet::new();
    for i in 0..20 {
        let db = app.state().db().clone();
        let auth = auth(&app);
        tasks.spawn(async move {
            RbacService::new(&db, &auth.resolver)
                .create_role(Actor::System, ADMIN_GUARD, &format!("role-{i}"), &names(&["posts.view"]))
                .await
        });
        let db = app.state().db().clone();
        tasks.spawn(async move {
            rustcms_auth::session::prune_expired(&db).await.map(|_| role::Model {
                id: 0,
                name: String::new(),
                guard_name: String::new(),
                is_system: false,
                is_privileged: false,
                created_at: chrono::Utc::now(),
            })
        });
    }
    while let Some(result) = tasks.join_next().await {
        result.unwrap().expect("concurrent write succeeds");
    }
});

/// Runs `change` while a concurrent RBAC transaction holds the lock, then commits
/// that transaction (after `concurrent` wrote in it) and returns `change`'s result.
async fn interleave<T: Send + 'static>(
    app: &rustcms_testing::TestApp,
    concurrent: impl AsyncFnOnce(&sea_orm::DatabaseTransaction),
    change: impl Future<Output = T> + Send + 'static,
) -> T {
    let blocker = rustcms_core::db::begin_write(app.state().db()).await.unwrap();
    rustcms_auth::rbac::lock(&blocker).await.unwrap();
    concurrent(&blocker).await;
    let change = tokio::spawn(change);
    // Give `change` time to run its reads; with the lock it waits instead.
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    blocker.commit().await.unwrap();
    change.await.unwrap()
}

db_test!(role_edit_cannot_restore_a_concurrently_revoked_permission, migrate = Migrator, |db: TestDb| async move {
    use rustcms_auth::entities::{permission, role_has_permissions as role_perm};
    use sea_orm::{ColumnTrait, QueryFilter};
    let (app, seed) = setup(db).await;
    let db = app.state().db().clone();
    let delete = permission::Entity::find()
        .filter(permission::Column::Name.eq("posts.delete"))
        .filter(permission::Column::GuardName.eq(ADMIN_GUARD))
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    let moderator = seed.moderator_role.id;

    // The editor (no posts.delete) keeps posts.delete in the list and adds roles.assign,
    // which it may grant, while an administrator revokes posts.delete from the role.
    let editor = principal(&app, ADMIN_GUARD, &seed.editor).await;
    let auth = auth(&app);
    let (db2, auth2) = (db.clone(), auth.clone());
    let result = interleave(
        &app,
        async |txn| {
            role_perm::Entity::delete_many()
                .filter(role_perm::Column::RoleId.eq(moderator))
                .filter(role_perm::Column::PermissionId.eq(delete.id))
                .exec(txn)
                .await
                .unwrap();
        },
        async move {
            RbacService::new(&db2, &auth2.resolver)
                .set_role_permissions(
                    Actor::User(&editor),
                    moderator,
                    &names(&["posts.view", "posts.delete", "roles.assign"]),
                )
                .await
        },
    )
    .await;

    forbidden(result);
    let kept = role_perm::Entity::find().filter(role_perm::Column::RoleId.eq(moderator)).all(&db).await.unwrap();
    assert!(kept.iter().all(|r| r.permission_id != delete.id), "revoked permission stays revoked");
});

db_test!(disabling_a_concurrently_promoted_user_keeps_a_super_admin, migrate = Migrator, |db: TestDb| async move {
    use rustcms_auth::entities::{USER_MODEL, model_has_roles as user_role};
    use sea_orm::{ActiveModelTrait, ColumnTrait, QueryFilter, Set};
    let (app, seed) = setup(db).await;
    let db = app.state().db().clone();
    let role = super_admin_role(&app, ADMIN_GUARD).await;
    let member = seed.member.id;
    let auth = auth(&app);
    let (db2, auth2) = (db.clone(), auth.clone());

    // Concurrently: `member` becomes the only admin-guard super admin.
    let result = interleave(
        &app,
        async |txn| {
            user_role::ActiveModel {
                role_id: Set(role.id),
                model_type: Set(USER_MODEL.into()),
                model_id: Set(member.to_string()),
            }
            .insert(txn)
            .await
            .unwrap();
            user_role::Entity::delete_many()
                .filter(user_role::Column::RoleId.eq(role.id))
                .filter(user_role::Column::ModelId.is_in([seed.admin.id.to_string(), seed.admin2.id.to_string()]))
                .exec(txn)
                .await
                .unwrap();
        },
        async move { RbacService::new(&db2, &auth2.resolver).set_user_active(Actor::System, member, false).await },
    )
    .await;

    invalid(result);
    assert!(support::reload(&app, &seed.member).await.is_active, "the last super admin stays active");
});

db_test!(user_creation_is_all_or_nothing, migrate = Migrator, |db: TestDb| async move {
    use rustcms_auth::NewUser;
    use rustcms_core::models::user;
    use sea_orm::{ColumnTrait, QueryFilter};
    let (app, seed) = setup(db).await;
    let auth = auth(&app);
    let rbac = RbacService::new(app.state().db(), &auth.resolver);
    let new = |guard: Option<&'static str>| NewUser {
        name: "Root",
        email: "Root@Example.test",
        password_hash: "hash".into(),
        super_admin_guard: guard,
    };
    let exists = || async {
        user::Entity::find()
            .filter(user::Column::Email.eq("root@example.test"))
            .one(app.state().db())
            .await
            .unwrap()
            .is_some()
    };

    // Unknown guard: nothing is created, so the command can simply be retried.
    invalid(rbac.create_user(Actor::System, new(Some("nope"))).await);
    assert!(!exists().await, "no half-created user");

    let created = rbac.create_user(Actor::System, new(Some(ADMIN_GUARD))).await.unwrap();
    assert_eq!(created.email, "root@example.test");
    assert!(principal(&app, ADMIN_GUARD, &created).await.super_admin);
    invalid(rbac.create_user(Actor::System, new(None)).await);

    // Through the panel: users.create is required, and only super admins create super admins.
    let editor = principal(&app, ADMIN_GUARD, &seed.editor).await;
    forbidden(rbac.create_user(Actor::User(&editor), NewUser { email: "x@example.test", ..new(None) }).await);
    let mut creator = editor.clone();
    creator.permissions.insert("users.create".into());
    rustcms_auth::RbacService::new(app.state().db(), &auth.resolver)
        .grant_permission(Actor::System, seed.editor.id, ADMIN_GUARD, "users.create")
        .await
        .unwrap();
    rbac.create_user(Actor::User(&creator), NewUser { email: "y@example.test", ..new(None) }).await.unwrap();
    forbidden(
        rbac.create_user(Actor::User(&creator), NewUser { email: "z@example.test", ..new(Some(ADMIN_GUARD)) }).await,
    );
});
