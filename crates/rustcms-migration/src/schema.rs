//! Schema helpers that give the same result on SQLite, PostgreSQL and MySQL.
//!
//! - Instants: `TIMESTAMPTZ` on PostgreSQL, but `DATETIME(6)` (UTC) on MySQL,
//!   whose `TIMESTAMP` stops in 2038 and drops fractions of a second.
//! - Tables: MySQL tables are InnoDB with the collation rustcms requires.
//! - Re-runs: MySQL commits every DDL statement, so a migration that failed
//!   half-way is retried on a partly changed schema. These helpers skip what
//!   already exists (tables, indexes, columns).
//!
//! ```rust,ignore
//! create_table(manager, Table::create().table(Posts::Table)
//!     .col(ColumnDef::new(Posts::PublishedAt).instant(manager).null())
//!     .col(ColumnDef::new(Posts::CreatedAt).instant_now(manager).not_null())
//!     .to_owned()).await?;
//! ```

use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::DbBackend;

/// The MySQL collation rustcms requires (see `rustcms_core::db::check`).
const MYSQL_COLLATION: &str = "utf8mb4_0900_bin";

/// Backend-aware column types.
pub trait ColumnDefExt {
    /// A point in time: `TIMESTAMPTZ` (PostgreSQL), `DATETIME(6)` in UTC (MySQL).
    fn instant(&mut self, manager: &SchemaManager) -> &mut Self;
    /// [`ColumnDefExt::instant`] defaulting to the current time (with microseconds on MySQL).
    fn instant_now(&mut self, manager: &SchemaManager) -> &mut Self;
    /// A date and time without zone: `DATETIME(6)` on MySQL (no fraction loss).
    fn local_date_time(&mut self, manager: &SchemaManager) -> &mut Self;
}

impl ColumnDefExt for ColumnDef {
    fn instant(&mut self, manager: &SchemaManager) -> &mut Self {
        match manager.get_database_backend() {
            DbBackend::MySql => self.custom(Alias::new("datetime(6)")),
            _ => self.timestamp_with_time_zone(),
        }
    }

    fn instant_now(&mut self, manager: &SchemaManager) -> &mut Self {
        match manager.get_database_backend() {
            DbBackend::MySql => self.custom(Alias::new("datetime(6)")).default(Expr::cust("CURRENT_TIMESTAMP(6)")),
            _ => self.timestamp_with_time_zone().default(Expr::current_timestamp()),
        }
    }

    fn local_date_time(&mut self, manager: &SchemaManager) -> &mut Self {
        match manager.get_database_backend() {
            DbBackend::MySql => self.custom(Alias::new("datetime(6)")),
            _ => self.date_time(),
        }
    }
}

/// Creates the table unless it exists; on MySQL as InnoDB with the required collation.
pub async fn create_table(manager: &SchemaManager<'_>, mut statement: TableCreateStatement) -> Result<(), DbErr> {
    statement.if_not_exists();
    if manager.get_database_backend() == DbBackend::MySql {
        statement.engine("InnoDB").character_set("utf8mb4").collate(MYSQL_COLLATION);
    }
    manager.create_table(statement).await
}

/// Creates the (named) index unless `table` already has it. MySQL ignores
/// `IF NOT EXISTS` on indexes, so it is checked first.
pub async fn create_index(
    manager: &SchemaManager<'_>,
    table: impl IntoIden,
    statement: IndexCreateStatement,
) -> Result<(), DbErr> {
    let name = statement.get_index_spec().get_name().expect("indexes created by migrations are named").to_owned();
    if manager.has_index(table.into_iden().to_string(), name).await? {
        return Ok(());
    }
    manager.create_index(statement).await
}

/// Adds the column unless `table` already has it.
pub async fn add_column(
    manager: &SchemaManager<'_>,
    table: impl IntoIden,
    column: &mut ColumnDef,
) -> Result<(), DbErr> {
    let table = table.into_iden();
    let name = column.get_column_name();
    if manager.has_column(table.to_string(), name).await? {
        return Ok(());
    }
    manager.alter_table(Table::alter().table(table).add_column(column.to_owned()).to_owned()).await
}

/// Drops the column if `table` still has it.
pub async fn drop_column(
    manager: &SchemaManager<'_>,
    table: impl IntoIden,
    column: impl IntoIden,
) -> Result<(), DbErr> {
    let (table, column) = (table.into_iden(), column.into_iden());
    if !manager.has_column(table.to_string(), column.to_string()).await? {
        return Ok(());
    }
    manager.alter_table(Table::alter().table(table).drop_column(column).to_owned()).await
}

/// Drops the table if it exists.
pub async fn drop_table(manager: &SchemaManager<'_>, table: impl IntoIden) -> Result<(), DbErr> {
    manager.drop_table(Table::drop().table(table).if_exists().to_owned()).await
}
