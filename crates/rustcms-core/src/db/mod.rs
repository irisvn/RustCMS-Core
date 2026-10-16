//! Database connection.

mod check;
mod locks;
mod transactions;

pub use check::*;
pub use locks::*;
pub use transactions::*;

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

/// One connection for running migrations. With a pool, SQLite connections
/// other than the one that changed the schema may still see the old one
/// (a column reported present after it was dropped, and skipped when added
/// again), so every step of a migration runs on the same connection.
pub async fn connect_for_migrations(config: &DatabaseConfig) -> Result<DatabaseConnection, DbErr> {
    connect(&DatabaseConfig { max_connections: 1, ..config.clone() }).await
}

/// Drops every table and runs all migrations (`migrate:fresh`).
///
/// SQLite checks foreign keys on `DROP TABLE`: dropping every table in any
/// order needs them off. The pragma is per connection, so everything runs on
/// one connection (a pool could run the drops elsewhere).
pub async fn migrate_fresh<M: sea_orm_migration::MigratorTrait>(config: &DatabaseConfig) -> Result<(), DbErr> {
    let db = connect_for_migrations(config).await?;
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
