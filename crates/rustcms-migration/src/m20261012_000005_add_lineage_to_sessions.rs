//! Groups a session with every session re-issued from it (`lineage` = id of the
//! first one), so signing out ends re-issues created concurrently. `NULL` (rows
//! created before this migration) means the session is its own lineage.

use sea_orm_migration::prelude::*;

use crate::schema::{add_column, create_index};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[sea_orm_migration::async_trait::async_trait]
impl MigrationTrait for Migration {
    fn use_transaction(&self) -> Option<bool> {
        Some(true)
    }

    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        add_column(manager, Sessions::Table, ColumnDef::new(Sessions::Lineage).string_len(64).null()).await?;
        create_index(
            manager,
            Sessions::Table,
            Index::create().name("sessions_lineage_idx").table(Sessions::Table).col(Sessions::Lineage).to_owned(),
        )
        .await
    }

    /// Each step is skipped when done (MySQL commits DDL statement by statement).
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if manager.has_index("sessions", "sessions_lineage_idx").await? {
            manager.drop_index(Index::drop().name("sessions_lineage_idx").table(Sessions::Table).to_owned()).await?;
        }
        if manager.has_column("sessions", "lineage").await? {
            manager
                .alter_table(Table::alter().table(Sessions::Table).drop_column(Sessions::Lineage).to_owned())
                .await?;
        }
        Ok(())
    }
}

#[derive(DeriveIden)]
enum Sessions {
    Table,
    Lineage,
}
