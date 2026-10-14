//! What rustcms expects of each database, checked on all three: setup
//! checks (MySQL), named locks, instants, string identity, lock timeouts.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use chrono::{DateTime, Utc};
use rustcms_core::AppError;
use rustcms_core::models::user;
use rustcms_migration::Migrator;
use rustcms_testing::{Backend, TestDb, db_test};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, QuerySelect, Set, TransactionTrait,
};

async fn new_user(db: &TestDb, email: &str) -> user::Model {
    user::ActiveModel {
        name: Set("User".into()),
        email: Set(email.into()),
        password: Set("hash".into()),
        ..Default::default()
    }
    .insert(db.conn())
    .await
    .unwrap()
}

fn instant(text: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(text).unwrap().with_timezone(&Utc)
}

db_test!(mysql_setup_is_checked, migrate = Migrator, |db: TestDb| async move {
    let conn = db.conn();
    rustcms_core::db::check(conn, true).await.expect("a database set up as documented passes");
    if db.backend() != Backend::Mysql {
        return;
    }
    // A table with a case-insensitive collation, then one that is not InnoDB.
    conn.execute_unprepared("CREATE TABLE loose (name varchar(20)) COLLATE utf8mb4_0900_ai_ci").await.unwrap();
    let err = rustcms_core::db::check(conn, true).await.unwrap_err().to_string();
    assert!(err.contains("ALTER TABLE `loose` CONVERT TO CHARACTER SET utf8mb4 COLLATE utf8mb4_0900_bin"), "{err}");
    rustcms_core::db::check(conn, false).await.expect("the database itself is fine");
    conn.execute_unprepared("DROP TABLE loose").await.unwrap();
    conn.execute_unprepared("CREATE TABLE heap (id int) ENGINE = MEMORY").await.unwrap();
    let err = rustcms_core::db::check(conn, true).await.unwrap_err().to_string();
    assert!(err.contains("ALTER TABLE `heap` ENGINE = InnoDB"), "{err}");
    conn.execute_unprepared("DROP TABLE heap").await.unwrap();
    // The database default (for tables created later).
    conn.execute_unprepared("ALTER DATABASE CHARACTER SET utf8mb4 COLLATE utf8mb4_0900_ai_ci").await.unwrap();
    let err = rustcms_core::db::check(conn, false).await.unwrap_err().to_string();
    assert!(err.contains("COLLATE utf8mb4_0900_bin;"), "{err}");
    conn.execute_unprepared("ALTER DATABASE CHARACTER SET utf8mb4 COLLATE utf8mb4_0900_bin").await.unwrap();
    rustcms_core::db::check(conn, true).await.unwrap();
});

db_test!(named_locks_serialize_transactions, migrate = Migrator, |db: TestDb| async move {
    if db.backend() == Backend::Sqlite {
        return; // Writers already serialize on the database lock.
    }
    let first = rustcms_core::db::begin_write(db.conn()).await.unwrap();
    rustcms_core::db::xact_lock(&first, "rbac").await.unwrap();
    let released = Arc::new(AtomicBool::new(false));
    let (conn, seen) = (db.conn().clone(), released.clone());
    let second = tokio::spawn(async move {
        let txn = rustcms_core::db::begin_write(&conn).await.unwrap();
        rustcms_core::db::xact_lock(&txn, "rbac").await.unwrap();
        let after_release = seen.load(Ordering::SeqCst);
        txn.commit().await.unwrap();
        after_release
    });
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    released.store(true, Ordering::SeqCst);
    first.commit().await.unwrap();
    assert!(second.await.unwrap(), "the second transaction waited for the first");
    // Slots are bounded and stable; a missing name is an error, never a silent skip.
    assert_eq!(rustcms_core::db::lock_slot("idem", "admin:1"), rustcms_core::db::lock_slot("idem", "admin:1"));
    let txn = rustcms_core::db::begin_write(db.conn()).await.unwrap();
    if db.backend() == Backend::Mysql {
        assert!(rustcms_core::db::xact_lock(&txn, "unknown").await.is_err());
    }
});

db_test!(instants_keep_microseconds_after_2038, migrate = Migrator, |db: TestDb| async move {
    let user = new_user(&db, "time@example.test").await;
    let later = instant("2045-06-01T10:20:30.123456Z");
    let mut active: user::ActiveModel = user.into();
    active.email_verified_at = Set(Some(later));
    let user = active.update(db.conn()).await.unwrap();
    let read = user::Entity::find_by_id(user.id).one(db.conn()).await.unwrap().unwrap();
    assert_eq!(read.email_verified_at, Some(later));
    // Compared in the database too (UTC on every backend).
    let found = user::Entity::find().filter(user::Column::EmailVerifiedAt.gt(Utc::now())).all(db.conn()).await.unwrap();
    assert_eq!(found.len(), 1);
    // The default is the database's own clock: recent, within a margin for
    // clocks that differ slightly (a server in a VM or container).
    assert!((read.created_at - Utc::now()).num_seconds().abs() < 300, "{}", read.created_at);
    if db.backend() == Backend::Mysql {
        // Below a microsecond, values are truncated (not rounded).
        let mut active: user::ActiveModel = read.into();
        active.email_verified_at = Set(Some(instant("2045-06-01T10:20:30.1234569Z")));
        let user = active.update(db.conn()).await.unwrap();
        let read = user::Entity::find_by_id(user.id).one(db.conn()).await.unwrap().unwrap();
        assert_eq!(read.email_verified_at, Some(later));
    }
});

db_test!(strings_compare_exactly, migrate = Migrator, |db: TestDb| async move {
    // Case, accents and trailing spaces all make a different value (NO PAD on MySQL).
    for email in ["a@example.test", "A@example.test", "a@example.test ", "á@example.test"] {
        new_user(&db, email).await;
    }
    for email in ["a@example.test", "a@example.test "] {
        let found = user::Entity::find().filter(user::Column::Email.eq(email)).all(db.conn()).await.unwrap();
        assert_eq!(found.len(), 1, "{email:?}");
        assert_eq!(found[0].email, email);
    }
});

db_test!(lock_timeouts_roll_the_whole_write_back, migrate = Migrator, |db: TestDb| async move {
    if db.backend() == Backend::Sqlite {
        return; // A busy SQLite database is covered by the auth tests.
    }
    let (held, other) = (new_user(&db, "held@example.test").await, new_user(&db, "other@example.test").await);
    let holder = db.conn().begin().await.unwrap();
    user::Entity::find_by_id(held.id).lock_exclusive().one(&holder).await.unwrap();

    let txn = rustcms_core::db::begin_write(db.conn()).await.unwrap();
    match db.backend() {
        Backend::Mysql => rustcms_core::db::set_lock_wait(&txn, 1).await.unwrap(),
        _ => {
            txn.execute_unprepared("SET LOCAL lock_timeout = '1s'").await.unwrap();
        }
    }
    let mut renamed: user::ActiveModel = other.clone().into();
    renamed.name = Set("Renamed".into());
    renamed.update(&txn).await.unwrap();
    let err: AppError = user::Entity::find_by_id(held.id).lock_exclusive().one(&txn).await.unwrap_err().into();
    assert!(matches!(err, AppError::Conflict { code: "write_conflict", .. }), "{err:?}");
    drop(txn);
    holder.rollback().await.unwrap();
    let other = user::Entity::find_by_id(other.id).one(db.conn()).await.unwrap().unwrap();
    assert_eq!(other.name, "User", "the earlier write of the failed transaction is gone too");
});

db_test!(deadlocks_answer_409_to_one_writer, migrate = Migrator, |db: TestDb| async move {
    if db.backend() == Backend::Sqlite {
        return; // Writers never hold row locks concurrently.
    }
    let (a, b) = (new_user(&db, "a@example.test").await, new_user(&db, "b@example.test").await);
    let first = rustcms_core::db::begin_write(db.conn()).await.unwrap();
    let second = rustcms_core::db::begin_write(db.conn()).await.unwrap();
    user::Entity::find_by_id(a.id).lock_exclusive().one(&first).await.unwrap();
    user::Entity::find_by_id(b.id).lock_exclusive().one(&second).await.unwrap();
    // Opposite order: one of them is chosen as the deadlock victim.
    let crossing = tokio::spawn(async move {
        let result = user::Entity::find_by_id(b.id).lock_exclusive().one(&first).await;
        (result.map(|_| ()), first)
    });
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let other = user::Entity::find_by_id(a.id).lock_exclusive().one(&second).await.map(|_| ());
    let mut errors = Vec::new();
    match other {
        Ok(()) => {}
        Err(err) => {
            errors.push(AppError::from(err));
            drop(second);
        }
    }
    let (crossed, first) = crossing.await.unwrap();
    if let Err(err) = crossed {
        errors.push(AppError::from(err));
    }
    drop(first);
    assert_eq!(errors.len(), 1, "exactly one victim");
    assert!(matches!(errors[0], AppError::Conflict { code: "write_conflict", .. }), "{:?}", errors[0]);
});

db_test!(column_helpers_keep_postgres_and_sqlite_ddl, |db: TestDb| async move {
    use rustcms_migration::schema::ColumnDefExt;
    use sea_orm::sea_query::{Alias, ColumnDef, Expr, PostgresQueryBuilder, SqliteQueryBuilder, Table};
    use sea_orm_migration::SchemaManager;
    if db.backend() == Backend::Mysql {
        return; // MySQL is the database the helpers change.
    }
    let manager = SchemaManager::new(db.conn());
    let render = |table: &sea_orm::sea_query::TableCreateStatement| match db.backend() {
        Backend::Postgres => table.to_string(PostgresQueryBuilder),
        _ => table.to_string(SqliteQueryBuilder),
    };
    let with_helpers = Table::create()
        .table(Alias::new("t"))
        .col(ColumnDef::new(Alias::new("a")).instant(&manager).null())
        .col(ColumnDef::new(Alias::new("b")).instant_now(&manager).not_null())
        .col(ColumnDef::new(Alias::new("c")).local_date_time(&manager))
        .to_owned();
    let before = Table::create()
        .table(Alias::new("t"))
        .col(ColumnDef::new(Alias::new("a")).timestamp_with_time_zone().null())
        .col(ColumnDef::new(Alias::new("b")).timestamp_with_time_zone().default(Expr::current_timestamp()).not_null())
        .col(ColumnDef::new(Alias::new("c")).date_time())
        .to_owned();
    assert_eq!(render(&with_helpers), render(&before));
});

db_test!(mysql_copies_point_their_foreign_keys_at_themselves, migrate = Migrator, |db: TestDb| async move {
    if db.backend() != Backend::Mysql {
        return;
    }
    let count = |sql: &str| {
        let conn = db.conn().clone();
        let sql = sql.to_owned();
        async move {
            conn.query_one_raw(sea_orm::Statement::from_string(sea_orm::DbBackend::MySql, sql))
                .await
                .unwrap()
                .unwrap()
                .try_get::<i64>("", "n")
                .unwrap()
        }
    };
    let base = "SELECT COUNT(*) AS n FROM information_schema.key_column_usage \
                WHERE table_schema = DATABASE() AND referenced_table_name IS NOT NULL";
    assert!(count(base).await > 0, "the framework tables have foreign keys");
    assert_eq!(count(&format!("{base} AND referenced_table_schema <> DATABASE()")).await, 0);
});

db_test!(mysql_connections_must_be_utc_and_strict, migrate = Migrator, |db: TestDb| async move {
    if db.backend() != Backend::Mysql {
        return;
    }
    // A pool opened elsewhere, in another time zone.
    let mut options = sea_orm::ConnectOptions::new(format!("{}?timezone=%2B07:00", db.url()));
    options.max_connections(1);
    let foreign = sea_orm::Database::connect(options).await.unwrap();
    let err = rustcms_core::db::check(&foreign, false).await.unwrap_err().to_string();
    assert!(err.contains("time_zone '+00:00'"), "{err}");
    // A session that reads backslashes literally.
    let literal = rustcms_core::db::connect(&rustcms_core::config::DatabaseConfig {
        url: db.url().to_owned(),
        max_connections: 1,
        log_statements: false,
    })
    .await
    .unwrap();
    literal.execute_unprepared("SET SESSION sql_mode = CONCAT(@@sql_mode, ',NO_BACKSLASH_ESCAPES')").await.unwrap();
    let err = rustcms_core::db::check(&literal, false).await.unwrap_err().to_string();
    assert!(err.contains("NO_BACKSLASH_ESCAPES"), "{err}");
});
