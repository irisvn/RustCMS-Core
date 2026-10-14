//! Write and snapshot transactions, and MySQL's row lock wait.

use super::*;

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
