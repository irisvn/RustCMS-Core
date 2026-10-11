//! Database connection.

use sea_orm::{
    ConnectOptions, ConnectionTrait, Database, DatabaseConnection, DatabaseTransaction, DbErr, SqliteTransactionMode,
    TransactionOptions, TransactionTrait,
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
