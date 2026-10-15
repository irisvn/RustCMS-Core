use sea_orm_migration::prelude::*;

use crate::schema::{ColumnDefExt, create_index, create_table};

/// Exports of resource lists made in the background (ADR 0021): who asked,
/// with which credentials version, the query and columns fixed at creation,
/// the records exported (their keys, for checks at download), and the file.
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
                .table(Exports::Table)
                .if_not_exists()
                .col(ColumnDef::new(Exports::Id).uuid().not_null().primary_key())
                .col(ColumnDef::new(Exports::UserId).big_integer().not_null())
                .col(ColumnDef::new(Exports::AuthVersion).big_integer().not_null())
                .col(ColumnDef::new(Exports::Resource).string_len(64).not_null())
                .col(ColumnDef::new(Exports::Query).json().not_null())
                .col(ColumnDef::new(Exports::Columns).json().not_null())
                .col(ColumnDef::new(Exports::Keys).json())
                .col(ColumnDef::new(Exports::Status).string_len(16).not_null())
                .col(ColumnDef::new(Exports::Rows).integer().not_null().default(0))
                .col(ColumnDef::new(Exports::Disk).string_len(16))
                .col(ColumnDef::new(Exports::StorageKey).string_len(128))
                .col(ColumnDef::new(Exports::Error).string_len(255))
                .col(ColumnDef::new(Exports::CreatedAt).instant(manager).not_null())
                .col(ColumnDef::new(Exports::ExpiresAt).instant(manager).not_null())
                .to_owned(),
        )
        .await?;
        create_index(
            manager,
            Exports::Table,
            Index::create()
                .if_not_exists()
                .name("exports_user_id_idx")
                .table(Exports::Table)
                .col(Exports::UserId)
                .to_owned(),
        )
        .await?;
        create_index(
            manager,
            Exports::Table,
            Index::create()
                .if_not_exists()
                .name("exports_expires_at_idx")
                .table(Exports::Table)
                .col(Exports::ExpiresAt)
                .to_owned(),
        )
        .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(Exports::Table).if_exists().to_owned()).await
    }
}

#[derive(DeriveIden)]
enum Exports {
    Table,
    Id,
    UserId,
    AuthVersion,
    Resource,
    Query,
    Columns,
    Keys,
    Status,
    Rows,
    Disk,
    StorageKey,
    Error,
    CreatedAt,
    ExpiresAt,
}
