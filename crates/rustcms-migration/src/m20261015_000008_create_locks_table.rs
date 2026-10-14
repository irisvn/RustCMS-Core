//! Named locks for MySQL (`rustcms_core::db::xact_lock`): one row per name,
//! locked with `FOR UPDATE` until the transaction ends. Every name exists up
//! front, so taking a lock never inserts (no duplicate-key waits or
//! deadlocks). PostgreSQL uses advisory locks and SQLite its write lock; the
//! table exists everywhere so every database has the same schema.

use std::collections::HashSet;

use sea_orm_migration::prelude::*;

use crate::schema::create_table;

/// Lock slots per hashed namespace (`rustcms_core::db::LOCK_SLOTS`).
const SLOTS: u64 = 1024;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[sea_orm_migration::async_trait::async_trait]
impl MigrationTrait for Migration {
    fn use_transaction(&self) -> Option<bool> {
        Some(true)
    }

    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            manager,
            Table::create()
                .table(Locks::Table)
                .col(ColumnDef::new(Locks::Name).string_len(64).not_null().primary_key())
                .to_owned(),
        )
        .await?;
        let db = manager.get_connection();
        let existing: HashSet<String> = db
            .query_all(&Query::select().column(Locks::Name).from(Locks::Table).to_owned())
            .await?
            .iter()
            .map(|row| row.try_get::<String>("", "name"))
            .collect::<Result<_, _>>()?;
        let names: Vec<String> = std::iter::once("rbac".to_owned())
            .chain(["lineage", "idem"].iter().flat_map(|ns| (0..SLOTS).map(move |slot| format!("{ns}:{slot}"))))
            .filter(|name| !existing.contains(name))
            .collect();
        for chunk in names.chunks(256) {
            let mut insert = Query::insert().into_table(Locks::Table).columns([Locks::Name]).to_owned();
            for name in chunk {
                insert.values_panic([name.as_str().into()]);
            }
            manager.exec_stmt(insert).await?;
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(Locks::Table).if_exists().to_owned()).await
    }
}

#[derive(DeriveIden)]
enum Locks {
    #[sea_orm(iden = "rustcms_locks")]
    Table,
    Name,
}
