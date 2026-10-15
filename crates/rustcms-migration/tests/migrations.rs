//! Framework migrations on SQLite and PostgreSQL: upgrades keep data, rollbacks are clean.

use rustcms_core::models::user;
use rustcms_migration::Migrator;
use rustcms_testing::{Factory, TestDb, UserFactory, db_test};
use sea_orm::{ActiveModelTrait, ConnectionTrait, EntityTrait, Set};
use sea_orm_migration::{MigratorTrait, SchemaManager};

async fn has_table(db: &TestDb, table: &str) -> bool {
    SchemaManager::new(db.conn()).has_table(table).await.unwrap()
}

/// A user row written with the columns every schema version has (the entity
/// describes the latest schema only).
async fn insert_user(db: &TestDb, email: &str) {
    let sql = format!("INSERT INTO users (name, email, password) VALUES ('Ada', '{email}', 'hash')");
    db.conn().execute_unprepared(&sql).await.unwrap();
}

/// Number of migrations up to (excluding) the one whose name contains `name`.
fn before(name: &str) -> u32 {
    Migrator::migrations().iter().position(|m| m.name().contains(name)).expect("migration exists") as u32
}

/// Number of migrations applied after (and including) the one named `name`.
fn from(name: &str) -> u32 {
    Migrator::migrations().len() as u32 - before(name)
}

async fn find_user(db: &TestDb, email: &str) -> user::Model {
    use sea_orm::{ColumnTrait, QueryFilter};
    user::Entity::find().filter(user::Column::Email.eq(email)).one(db.conn()).await.unwrap().unwrap()
}

db_test!(upgrade_keeps_existing_users, |db: TestDb| async move {
    // Version N: only the users table.
    Migrator::up(db.conn(), Some(1)).await.unwrap();
    assert!(has_table(&db, "users").await);
    assert!(!has_table(&db, "sessions").await);

    // Seeded with only the required columns: defaults must fill the rest.
    insert_user(&db, "ada@example.test").await;

    // Upgrade to the latest version.
    Migrator::up(db.conn(), None).await.unwrap();
    assert!(has_table(&db, "sessions").await);
    let found = find_user(&db, "ada@example.test").await;
    assert_eq!((found.name.as_str(), found.password.as_str()), ("Ada", "hash"), "upgrade preserves rows");
    assert!(found.is_active);
    assert_eq!((found.auth_version, found.locale), (0, None));
});

// 0.2 → 0.3: users keep working and follow the browser's language until they choose.
db_test!(upgrade_from_0_2_adds_user_locale, |db: TestDb| async move {
    Migrator::up(db.conn(), Some(before("add_locale_to_users"))).await.unwrap();
    insert_user(&db, "ada@example.test").await;
    Migrator::up(db.conn(), None).await.unwrap();
    assert_eq!(find_user(&db, "ada@example.test").await.locale, None);
    // Re-runnable and reversible.
    Migrator::down(db.conn(), Some(from("add_locale_to_users"))).await.unwrap();
    assert!(!SchemaManager::new(db.conn()).has_column("users", "locale").await.unwrap());
    Migrator::up(db.conn(), None).await.unwrap();
    let mut user: user::ActiveModel = find_user(&db, "ada@example.test").await.into();
    user.locale = Set(Some("vi".into()));
    user.update(db.conn()).await.unwrap();
    assert_eq!(find_user(&db, "ada@example.test").await.locale.as_deref(), Some("vi"));
});

// M12 → M13: the audit log table appears empty, and rolls back cleanly.
db_test!(upgrade_adds_audit_logs, |db: TestDb| async move {
    Migrator::up(db.conn(), Some(before("create_audit_logs_table"))).await.unwrap();
    assert!(!has_table(&db, "audit_logs").await);
    insert_user(&db, "ada@example.test").await;
    Migrator::up(db.conn(), None).await.unwrap();
    assert!(has_table(&db, "audit_logs").await);
    assert!(rustcms_core::models::audit_log::Entity::find().all(db.conn()).await.unwrap().is_empty());
    Migrator::down(db.conn(), Some(from("create_audit_logs_table"))).await.unwrap();
    assert!(!has_table(&db, "audit_logs").await);
    assert!(find_user(&db, "ada@example.test").await.is_active, "rollback keeps users");
    Migrator::up(db.conn(), None).await.unwrap();
    assert!(has_table(&db, "audit_logs").await);
});

// M13 → M14: the settings table appears empty, and rolls back cleanly.
db_test!(upgrade_adds_settings, |db: TestDb| async move {
    Migrator::up(db.conn(), Some(before("create_settings_table"))).await.unwrap();
    assert!(!has_table(&db, "settings").await);
    Migrator::up(db.conn(), None).await.unwrap();
    assert!(rustcms_core::models::setting::Entity::find().all(db.conn()).await.unwrap().is_empty());
    Migrator::down(db.conn(), Some(from("create_settings_table"))).await.unwrap();
    assert!(!has_table(&db, "settings").await);
    assert!(has_table(&db, "audit_logs").await, "earlier tables are kept");
    Migrator::up(db.conn(), None).await.unwrap();
    assert!(has_table(&db, "settings").await);
});

// M14 → M15: the queue's tables appear empty, and roll back cleanly.
db_test!(upgrade_adds_jobs, |db: TestDb| async move {
    Migrator::up(db.conn(), Some(before("create_jobs_tables"))).await.unwrap();
    assert!(!has_table(&db, "jobs").await);
    Migrator::up(db.conn(), None).await.unwrap();
    assert!(has_table(&db, "jobs").await && has_table(&db, "job_receipts").await);
    Migrator::down(db.conn(), Some(from("create_jobs_tables"))).await.unwrap();
    assert!(!has_table(&db, "jobs").await && !has_table(&db, "job_receipts").await);
    assert!(has_table(&db, "settings").await, "earlier tables are kept");
    Migrator::up(db.conn(), None).await.unwrap();
});

// M1 → M2: users created before the auth tables keep working; system roles are seeded.
db_test!(upgrade_from_m1_adds_auth_tables, |db: TestDb| async move {
    Migrator::up(db.conn(), Some(2)).await.unwrap();
    insert_user(&db, "m1@example.test").await;
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
    assert!(find_user(&db, "m1@example.test").await.is_active);
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
    for table in [
        "users",
        "sessions",
        "personal_access_tokens",
        "roles",
        "permissions",
        "model_has_roles",
        "audit_logs",
        "settings",
        "jobs",
        "job_receipts",
    ] {
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

db_test!(foreign_keys_are_enforced, |db: TestDb| async move {
    let conn = db.conn();
    conn.execute_unprepared("CREATE TABLE parent (id INTEGER PRIMARY KEY)").await.unwrap();
    // A table constraint: MySQL ignores `REFERENCES` written on the column.
    conn.execute_unprepared(
        "CREATE TABLE child (id INTEGER PRIMARY KEY, parent_id INTEGER NOT NULL, \
         FOREIGN KEY (parent_id) REFERENCES parent(id))",
    )
    .await
    .unwrap();
    assert!(conn.execute_unprepared("INSERT INTO child (id, parent_id) VALUES (1, 42)").await.is_err());
});

// MySQL commits DDL statement by statement: a migration that failed half-way is
// run again on a partly changed schema. Every framework migration copes, here
// in the worst case (everything done, nothing recorded).
db_test!(framework_migrations_run_again_after_a_partial_failure, migrate = Migrator, |db: TestDb| async move {
    let manager = SchemaManager::new(db.conn());
    for migration in rustcms_migration::migrations() {
        migration.up(&manager).await.unwrap_or_else(|e| panic!("{} cannot run again: {e}", migration.name()));
    }
    // The seeds were not duplicated.
    let roles = db
        .conn()
        .query_one_raw(sea_orm::Statement::from_string(
            db.conn().get_database_backend(),
            "SELECT COUNT(*) AS n FROM roles WHERE name = 'super_admin'",
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<i64>("", "n")
        .unwrap();
    assert_eq!(roles, 2, "one super_admin per guard");
});

// The same for rollbacks: a `down` interrupted half-way runs again.
db_test!(framework_rollbacks_run_again_after_a_partial_failure, migrate = Migrator, |db: TestDb| async move {
    let manager = SchemaManager::new(db.conn());
    for migration in rustcms_migration::migrations().into_iter().rev() {
        for attempt in 1..=2 {
            migration.down(&manager).await.unwrap_or_else(|e| panic!("{} down #{attempt}: {e}", migration.name()));
        }
    }
    assert!(!has_table(&db, "users").await);
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
            // Re-runnable: MySQL keeps the table of a failed run (DDL commits).
            rustcms_migration::schema::create_table(
                manager,
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
    // MySQL commits each DDL statement: the table stays, and the retry must cope.
    let mysql = db.backend() == rustcms_testing::Backend::Mysql;
    assert_eq!(has_table(&db, "half_done").await, mysql, "partial schema change rolled back (except on MySQL)");
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
