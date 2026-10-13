//! Database connection.

use sea_orm::{
    AccessMode, ConnectOptions, ConnectionTrait, Database, DatabaseConnection, DatabaseTransaction, DbBackend, DbErr,
    IsolationLevel, SqliteTransactionMode, TransactionOptions, TransactionTrait,
};

use crate::config::DatabaseConfig;

/// Opens the connection pool.
///
/// SQLite connections always enable foreign keys, WAL journaling and a 5s busy
/// timeout, so behaviour matches PostgreSQL as closely as possible.
pub async fn connect(config: &DatabaseConfig) -> Result<DatabaseConnection, DbErr> {
    let mut options = ConnectOptions::new(&config.url);
    options.max_connections(config.max_connections).sqlx_logging(config.log_statements);
    #[cfg(feature = "sqlite")]
    options.map_sqlx_sqlite_opts(|o| {
        o.foreign_keys(true)
            .journal_mode(sea_orm::sqlx::sqlite::SqliteJournalMode::Wal)
            .busy_timeout(std::time::Duration::from_secs(5))
    });
    Database::connect(options).await
}

/// Drops every table and runs all migrations (`migrate:fresh`).
///
/// SQLite checks foreign keys on `DROP TABLE`: dropping every table in any
/// order needs them off. The pragma is per connection, so everything runs on
/// one connection (a pool could run the drops elsewhere).
pub async fn migrate_fresh<M: sea_orm_migration::MigratorTrait>(config: &DatabaseConfig) -> Result<(), DbErr> {
    let db = connect(&DatabaseConfig { max_connections: 1, ..config.clone() }).await?;
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

/// Starts a transaction that will write.
///
/// On SQLite it takes the write lock up front (`BEGIN IMMEDIATE`): concurrent
/// writers then wait for the busy timeout instead of failing with "database is
/// locked" when a read-then-write transaction tries to upgrade. Other databases
/// ignore the option.
pub async fn begin_write(db: &DatabaseConnection) -> Result<DatabaseTransaction, DbErr> {
    let options =
        TransactionOptions { sqlite_transaction_mode: Some(SqliteTransactionMode::Immediate), ..Default::default() };
    db.begin_with_options(options).await
}

/// Starts a read-only transaction whose reads all see one consistent snapshot,
/// for multi-query scans (batched pagination): `REPEATABLE READ` on PostgreSQL;
/// on SQLite (WAL) a read transaction keeps its snapshot from its first read.
pub async fn begin_snapshot(db: &DatabaseConnection) -> Result<DatabaseTransaction, DbErr> {
    if db.get_database_backend() == DbBackend::Postgres {
        db.begin_with_config(Some(IsolationLevel::RepeatableRead), Some(AccessMode::ReadOnly)).await
    } else {
        db.begin().await
    }
}
