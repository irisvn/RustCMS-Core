//! Application settings, one row per group (ADR 0015). Secrets are stored
//! encrypted in their own column, never in `value`.

use sea_orm_migration::prelude::*;

use crate::schema::{ColumnDefExt, create_table, drop_table};

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
                .table(Settings::Table)
                .col(ColumnDef::new(Settings::Name).string_len(32).not_null().primary_key())
                .col(ColumnDef::new(Settings::Value).json().not_null())
                .col(ColumnDef::new(Settings::Secrets).json().null())
                .col(ColumnDef::new(Settings::Version).big_integer().not_null())
                .col(ColumnDef::new(Settings::UpdatedAt).instant(manager).not_null())
                .col(ColumnDef::new(Settings::UpdatedBy).big_integer().null())
                .to_owned(),
        )
        .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_table(manager, Settings::Table).await
    }
}

#[derive(DeriveIden, Clone, Copy)]
enum Settings {
    Table,
    Name,
    Value,
    Secrets,
    Version,
    UpdatedAt,
    UpdatedBy,
}
