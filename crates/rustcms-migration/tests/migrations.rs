//! Framework migrations on SQLite and PostgreSQL: upgrades keep data, rollbacks are clean.

use rustcms_core::models::user;
use rustcms_migration::Migrator;
use rustcms_testing::{Factory, TestDb, UserFactory, db_test};
use sea_orm::{ActiveModelTrait, ConnectionTrait, EntityTrait, Set};
use sea_orm_migration::{MigratorTrait, SchemaManager};

async fn has_table(db: &TestDb, table: &str) -> bool {
    SchemaManager::new(db.conn()).has_table(table).await.unwrap()
}

db_test!(upgrade_keeps_existing_users, |db: TestDb| async move {
    // Version N: only the users table.
    Migrator::up(db.conn(), Some(1)).await.unwrap();
    assert!(has_table(&db, "users").await);
    assert!(!has_table(&db, "sessions").await);

    // Seed with only the required columns: defaults must fill the rest.
    let seeded = user::ActiveModel {
        name: Set("Ada".into()),
        email: Set("ada@example.test".into()),
        password: Set("hash".into()),
        ..Default::default()
    }
    .insert(db.conn())
    .await
    .unwrap();
    assert!(seeded.is_active);
    assert_eq!(seeded.auth_version, 0);

    // Upgrade to N+1.
    Migrator::up(db.conn(), None).await.unwrap();
    assert!(has_table(&db, "sessions").await);
    let found = user::Entity::find_by_id(seeded.id).one(db.conn()).await.unwrap().unwrap();
    assert_eq!(found, seeded, "upgrade preserves rows");
});

// M1 → M2: users created before the auth tables keep working; system roles are seeded.
db_test!(upgrade_from_m1_adds_auth_tables, |db: TestDb| async move {
    Migrator::up(db.conn(), Some(2)).await.unwrap();
    let user = UserFactory::new().create(db.conn()).await;
    Migrator::up(db.conn(), None).await.unwrap();

    for table in [
        "personal_access_tokens",
        "permissions",
        "roles",
        "role_has_permissions",
        "model_has_roles",
        "model_has_permissions",
    ] {
        assert!(has_table(&db, table).await, "{table}");
    }
    assert_eq!(user::Entity::find_by_id(user.id).one(db.conn()).await.unwrap().unwrap(), user);
    let roles = db
        .conn()
        .query_all_raw(sea_orm::Statement::from_string(
            db.conn().get_database_backend(),
            "SELECT name, guard_name FROM roles WHERE is_system = true AND is_privileged = true ORDER BY guard_name",
        ))
        .await
        .unwrap();
    let roles: Vec<(String, String)> =
        roles.iter().map(|r| (r.try_get("", "name").unwrap(), r.try_get("", "guard_name").unwrap())).collect();
    assert_eq!(roles, [("super_admin".into(), "admin".into()), ("super_admin".into(), "api".into())]);
});

db_test!(down_then_up_is_clean, |db: TestDb| async move {
    Migrator::up(db.conn(), None).await.unwrap();
    UserFactory::new().create(db.conn()).await;
    Migrator::down(db.conn(), None).await.unwrap();
    for table in ["users", "sessions", "personal_access_tokens", "roles", "permissions", "model_has_roles"] {
        assert!(!has_table(&db, table).await, "{table} dropped");
    }
    Migrator::up(db.conn(), None).await.unwrap();
    assert!(user::Entity::find().all(db.conn()).await.unwrap().is_empty());
});

db_test!(email_is_unique, migrate = Migrator, |db: TestDb| async move {
    UserFactory::new().email("dup@example.test").create(db.conn()).await;
    let duplicate = user::ActiveModel {
        name: Set("Other".into()),
        email: Set("dup@example.test".into()),
        password: Set("hash".into()),
        ..Default::default()
    }
    .insert(db.conn())
    .await;
    assert!(duplicate.is_err());
});

db_test!(sqlite_enforces_foreign_keys, |db: TestDb| async move {
    let conn = db.conn();
    conn.execute_unprepared("CREATE TABLE parent (id INTEGER PRIMARY KEY)").await.unwrap();
    conn.execute_unprepared(
        "CREATE TABLE child (id INTEGER PRIMARY KEY, parent_id INTEGER NOT NULL REFERENCES parent(id))",
    )
    .await
    .unwrap();
    assert!(conn.execute_unprepared("INSERT INTO child (id, parent_id) VALUES (1, 42)").await.is_err());
});

#[test]
fn every_framework_migration_is_transactional() {
    for migration in rustcms_migration::migrations() {
        assert_eq!(migration.use_transaction(), Some(true), "{} must run in a transaction", migration.name());
    }
}

mod failing {
    use std::cell::Cell;

    use sea_orm_migration::prelude::*;

    thread_local! {
        /// Each `#[tokio::test]` runs on its own thread, so this is per test.
        pub static FAIL: Cell<bool> = const { Cell::new(true) };
    }

    #[derive(DeriveMigrationName)]
    pub struct Migration;

    #[sea_orm_migration::async_trait::async_trait]
    impl MigrationTrait for Migration {
        fn use_transaction(&self) -> Option<bool> {
            Some(true)
        }

        async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
            manager
                .create_table(
                    Table::create().table("half_done").col(ColumnDef::new("id").integer().primary_key()).to_owned(),
                )
                .await?;
            if FAIL.get() {
                return Err(DbErr::Migration("injected failure after CREATE TABLE".into()));
            }
            Ok(())
        }

        async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
            manager.drop_table(Table::drop().table("half_done").to_owned()).await
        }
    }

    pub struct Migrator;

    impl MigratorTrait for Migrator {
        fn migrations() -> Vec<Box<dyn MigrationTrait>> {
            let mut all = rustcms_migration::migrations();
            all.push(Box::new(Migration));
            all
        }
    }
}

db_test!(failed_migration_rolls_back_and_can_be_retried, |db: TestDb| async move {
    failing::FAIL.set(true);
    assert!(failing::Migrator::up(db.conn(), None).await.is_err());
    assert!(!has_table(&db, "half_done").await, "partial schema change rolled back");
    let applied = failing::Migrator::get_applied_migrations(db.conn()).await.unwrap();
    assert_eq!(
        applied.len(),
        rustcms_migration::migrations().len(),
        "framework migrations applied, failing one not recorded"
    );

    failing::FAIL.set(false);
    failing::Migrator::up(db.conn(), None).await.expect("retry succeeds");
    assert!(has_table(&db, "half_done").await);
});

// `migrate:fresh` drops every table, including application tables whose foreign
// keys would refuse the drop (SQLite checks them on `DROP TABLE`, per connection;
// the parent sorts first, so it is dropped while the child still references it).
// The configured pool has two connections: the command must not use them.
db_test!(fresh_drops_tables_with_foreign_keys, |db: TestDb| async move {
    let config =
        rustcms_core::config::DatabaseConfig { url: db.url().to_owned(), max_connections: 2, log_statements: false };
    let conn = rustcms_core::db::connect(&config).await.unwrap();
    Migrator::up(&conn, None).await.unwrap();
    for sql in [
        "CREATE TABLE a_parents (id INTEGER PRIMARY KEY)",
        "CREATE TABLE b_children (id INTEGER PRIMARY KEY, parent_id INTEGER NOT NULL REFERENCES a_parents (id))",
        "INSERT INTO a_parents (id) VALUES (1)",
        "INSERT INTO b_children (id, parent_id) VALUES (1, 1)",
    ] {
        conn.execute_unprepared(sql).await.unwrap();
    }
    UserFactory::new().create(&conn).await;
    conn.close().await.unwrap();

    rustcms_core::db::migrate_fresh::<Migrator>(&config).await.expect("fresh succeeds with foreign keys on");
    assert!(!has_table(&db, "b_children").await && !has_table(&db, "a_parents").await);
    assert!(has_table(&db, "users").await);
    assert!(user::Entity::find().all(db.conn()).await.unwrap().is_empty());
});
