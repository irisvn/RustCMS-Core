//! Database connection.

use sea_orm::{
    AccessMode, ConnectOptions, ConnectionTrait, Database, DatabaseConnection, DatabaseTransaction, DbBackend, DbErr,
    IsolationLevel, SqliteTransactionMode, Statement, TransactionOptions, TransactionTrait,
};

use crate::config::DatabaseConfig;

/// The only MySQL collation rustcms accepts: case-, accent- and
/// trailing-space-sensitive (NO PAD), so strings compare as on PostgreSQL.
pub const MYSQL_COLLATION: &str = "utf8mb4_0900_bin";

/// Opens the connection pool.
///
/// SQLite connections always enable foreign keys, WAL journaling and a 5s busy
/// timeout, so behaviour matches PostgreSQL as closely as possible. MySQL
/// connections always use UTC (`DATETIME` columns hold UTC wall time) and
/// [`MYSQL_COLLATION`], whatever the URL says.
pub async fn connect(config: &DatabaseConfig) -> Result<DatabaseConnection, DbErr> {
    let mut options = ConnectOptions::new(&config.url);
    options.max_connections(config.max_connections).sqlx_logging(config.log_statements);
    #[cfg(feature = "sqlite")]
    options.map_sqlx_sqlite_opts(|o| {
        o.foreign_keys(true)
            .journal_mode(sea_orm::sqlx::sqlite::SqliteJournalMode::Wal)
            .busy_timeout(std::time::Duration::from_secs(5))
    });
    #[cfg(feature = "mysql")]
    options
        .map_sqlx_mysql_opts(|o| o.timezone(Some("+00:00".to_owned())).charset("utf8mb4").collation(MYSQL_COLLATION));
    Database::connect(options).await
}

/// Refuses a MySQL database whose settings would make rustcms behave unlike on
/// PostgreSQL: string columns (and the database default) must use
/// [`MYSQL_COLLATION`], tables must be InnoDB (transactions, row locks), and
/// `sql_mode` must be strict (without `NO_BACKSLASH_ESCAPES`, which changes
/// how string literals read), globally and on this session. The connection must
/// use UTC and [`MYSQL_COLLATION`] (pools from [`connect`] always do; a pool
/// built elsewhere must too). The error lists the SQL that fixes it. Other
/// databases pass. With `tables = false` only the database itself is checked
/// (before `migrate:fresh` recreates every table).
pub async fn check(db: &impl ConnectionTrait, tables: bool) -> anyhow::Result<()> {
    if db.get_database_backend() != DbBackend::MySql {
        return Ok(());
    }
    let rows = |sql: &str| db.query_all_raw(Statement::from_string(DbBackend::MySql, sql.to_owned()));
    let mut problems = Vec::new();
    let settings = rows(
        "SELECT s.schema_name AS db, s.default_character_set_name AS charset, \
         s.default_collation_name AS collation, @@SESSION.sql_mode AS mode, @@GLOBAL.sql_mode AS global_mode, \
         @@SESSION.time_zone AS zone, @@SESSION.collation_connection AS connection \
         FROM information_schema.schemata s WHERE s.schema_name = DATABASE()",
    )
    .await?;
    let Some(settings) = settings.first() else { anyhow::bail!("MySQL: no database selected") };
    let name: String = settings.try_get("", "db")?;
    let collation: String = settings.try_get("", "collation")?;
    if settings.try_get::<String>("", "charset")? != "utf8mb4" || collation != MYSQL_COLLATION {
        problems.push(format!("ALTER DATABASE `{name}` CHARACTER SET utf8mb4 COLLATE {MYSQL_COLLATION};"));
    }
    for column in ["mode", "global_mode"] {
        let mode: String = settings.try_get("", column)?;
        let modes: Vec<&str> = mode.split(',').collect();
        if !modes.contains(&"STRICT_TRANS_TABLES") || modes.contains(&"NO_BACKSLASH_ESCAPES") {
            problems.push(
                "SET PERSIST sql_mode = '…'; -- with STRICT_TRANS_TABLES, without NO_BACKSLASH_ESCAPES (then reconnect)"
                    .into(),
            );
            break;
        }
    }
    let zone: String = settings.try_get("", "zone")?;
    let connection: String = settings.try_get("", "connection")?;
    if zone != "+00:00" || connection != MYSQL_COLLATION {
        problems.push(format!(
            "-- connections need time_zone '+00:00' and collation {MYSQL_COLLATION} (open the pool with rustcms_core::db::connect)"
        ));
    }
    if tables {
        let columns = rows(&format!(
            "SELECT DISTINCT table_name AS t FROM information_schema.columns \
             WHERE table_schema = DATABASE() AND collation_name IS NOT NULL AND collation_name <> '{MYSQL_COLLATION}' \
             ORDER BY t"
        ))
        .await?;
        for row in columns {
            let table: String = row.try_get("", "t")?;
            problems.push(format!("ALTER TABLE `{table}` CONVERT TO CHARACTER SET utf8mb4 COLLATE {MYSQL_COLLATION};"));
        }
        let engines = rows(
            "SELECT table_name AS t FROM information_schema.tables \
             WHERE table_schema = DATABASE() AND table_type = 'BASE TABLE' AND engine <> 'InnoDB' ORDER BY t",
        )
        .await?;
        for row in engines {
            let table: String = row.try_get("", "t")?;
            problems.push(format!("ALTER TABLE `{table}` ENGINE = InnoDB;"));
        }
    }
    if problems.is_empty() {
        return Ok(());
    }
    anyhow::bail!(
        "MySQL database `{name}` is not set up for rustcms (see the configuration guide). Run:\n  {}",
        problems.join("\n  ")
    )
}

/// Drops every table and runs all migrations (`migrate:fresh`).
///
/// SQLite checks foreign keys on `DROP TABLE`: dropping every table in any
/// order needs them off. The pragma is per connection, so everything runs on
/// one connection (a pool could run the drops elsewhere).
pub async fn migrate_fresh<M: sea_orm_migration::MigratorTrait>(config: &DatabaseConfig) -> Result<(), DbErr> {
    let db = connect(&DatabaseConfig { max_connections: 1, ..config.clone() }).await?;
    check(&db, false).await.map_err(|e| DbErr::Custom(e.to_string()))?;
    let sqlite = db.get_database_backend() == DbBackend::Sqlite;
    if sqlite {
        db.execute_unprepared("PRAGMA foreign_keys = OFF").await?;
    }
    let result = M::fresh(&db).await;
    if sqlite {
        db.execute_unprepared("PRAGMA foreign_keys = ON").await?;
    }
    result
}

/// Returns `true` when the database answers a trivial query.
pub async fn ping(db: &DatabaseConnection) -> bool {
    db.execute_unprepared("SELECT 1").await.is_ok()
}

/// How long a MySQL write waits for a row lock before giving up (1205 → 409),
/// like PostgreSQL's `lock_timeout` where rustcms sets one.
pub const MYSQL_LOCK_WAIT_SECONDS: u64 = 10;

/// Starts a transaction that will write.
///
/// - SQLite: takes the write lock up front (`BEGIN IMMEDIATE`): concurrent
///   writers then wait for the busy timeout instead of failing with "database
///   is locked" when a read-then-write transaction tries to upgrade.
/// - MySQL: `READ COMMITTED`, as on PostgreSQL (reads after a lock see what
///   committed meanwhile), and the session's lock wait reset to
///   [`MYSQL_LOCK_WAIT_SECONDS`] (a claim wait may have shortened it).
pub async fn begin_write(db: &DatabaseConnection) -> Result<DatabaseTransaction, DbErr> {
    if db.get_database_backend() == DbBackend::MySql {
        let txn = db.begin_with_config(Some(IsolationLevel::ReadCommitted), None).await?;
        set_lock_wait(&txn, MYSQL_LOCK_WAIT_SECONDS).await?;
        return Ok(txn);
    }
    let options =
        TransactionOptions { sqlite_transaction_mode: Some(SqliteTransactionMode::Immediate), ..Default::default() };
    db.begin_with_options(options).await
}

/// MySQL: the session's row lock wait, in whole seconds (at least 1). It is a
/// session setting: [`begin_write`] restores the default for every write.
pub async fn set_lock_wait(txn: &DatabaseTransaction, seconds: u64) -> Result<(), DbErr> {
    if txn.get_database_backend() == DbBackend::MySql {
        txn.execute_unprepared(&format!("SET SESSION innodb_lock_wait_timeout = {}", seconds.max(1))).await?;
    }
    Ok(())
}

/// Number of lock slots per hashed namespace (see [`xact_lock`]).
pub const LOCK_SLOTS: u64 = 1024;

/// Serializes transactions on `name` until this one ends.
///
/// - PostgreSQL: a transaction-scoped advisory lock on a hash of `name`.
/// - MySQL: the row `name` of the preseeded `rustcms_locks` table, locked with
///   `FOR UPDATE` (names are `rbac`, `lineage:<slot>`, `idem:<slot>` with
///   slots below [`LOCK_SLOTS`]; see [`lock_slot`]).
/// - SQLite: nothing; [`begin_write`] already holds the database write lock.
///
/// Lock order: a transaction takes at most one such lock, as its first
/// statement, before any row lock. A missing row is an error, never a silent
/// skip.
pub async fn xact_lock(txn: &DatabaseTransaction, name: &str) -> Result<(), DbErr> {
    match txn.get_database_backend() {
        DbBackend::Postgres => {
            let key = i64::from_str_radix(&format!("{:016x}", fnv1a(name.as_bytes()))[..15], 16).unwrap_or_default();
            txn.execute_unprepared(&format!("SELECT pg_advisory_xact_lock({key})")).await?;
        }
        DbBackend::MySql => {
            let locked = txn
                .query_one_raw(Statement::from_sql_and_values(
                    DbBackend::MySql,
                    "SELECT name FROM rustcms_locks WHERE name = ? FOR UPDATE",
                    [name.into()],
                ))
                .await?;
            if locked.is_none() {
                return Err(DbErr::Custom(format!("lock `{name}` is missing from rustcms_locks (run the migrations)")));
            }
        }
        _ => {}
    }
    Ok(())
}

/// The lock name for `value` in a hashed namespace (`lineage`, `idem`): one of
/// [`LOCK_SLOTS`] rows, so the lock table stays bounded. Different values may
/// share a slot; they then wait for each other briefly.
pub fn lock_slot(namespace: &str, value: &str) -> String {
    format!("{namespace}:{}", fnv1a(value.as_bytes()) % LOCK_SLOTS)
}

/// FNV-1a: stable across runs and platforms.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3))
}

/// Starts a read-only transaction whose reads all see one consistent snapshot,
/// for multi-query scans (batched pagination): `REPEATABLE READ` on PostgreSQL and MySQL;
/// on SQLite (WAL) a read transaction keeps its snapshot from its first read.
pub async fn begin_snapshot(db: &DatabaseConnection) -> Result<DatabaseTransaction, DbErr> {
    if matches!(db.get_database_backend(), DbBackend::Postgres | DbBackend::MySql) {
        db.begin_with_config(Some(IsolationLevel::RepeatableRead), Some(AccessMode::ReadOnly)).await
    } else {
        db.begin().await
    }
}
