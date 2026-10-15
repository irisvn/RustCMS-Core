use sea_orm_migration::prelude::*;

use crate::schema::{ColumnDefExt, create_index, create_table};

/// Imports of CSV files into a resource (ADR 0021): who uploaded it, with
/// which credentials version, the file, its header, the column mapping fixed
/// when it runs, progress and the rows that failed (row, field, message).
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
                .table(Imports::Table)
                .if_not_exists()
                .col(ColumnDef::new(Imports::Id).uuid().not_null().primary_key())
                .col(ColumnDef::new(Imports::UserId).big_integer().not_null())
                .col(ColumnDef::new(Imports::AuthVersion).big_integer().not_null())
                .col(ColumnDef::new(Imports::Resource).string_len(64).not_null())
                .col(ColumnDef::new(Imports::Disk).string_len(16).not_null())
                .col(ColumnDef::new(Imports::StorageKey).string_len(128).not_null())
                .col(ColumnDef::new(Imports::Sha256).string_len(64).not_null())
                .col(ColumnDef::new(Imports::Header).json().not_null())
                .col(ColumnDef::new(Imports::Rows).integer().not_null())
                .col(ColumnDef::new(Imports::Mapping).json())
                .col(ColumnDef::new(Imports::Status).string_len(16).not_null())
                .col(ColumnDef::new(Imports::Created).integer().not_null().default(0))
                .col(ColumnDef::new(Imports::Updated).integer().not_null().default(0))
                .col(ColumnDef::new(Imports::Failed).integer().not_null().default(0))
                .col(ColumnDef::new(Imports::Report).json())
                .col(ColumnDef::new(Imports::Error).string_len(255))
                .col(ColumnDef::new(Imports::CreatedAt).instant(manager).not_null())
                .col(ColumnDef::new(Imports::ExpiresAt).instant(manager).not_null())
                .to_owned(),
        )
        .await?;
        create_index(
            manager,
            Imports::Table,
            Index::create()
                .if_not_exists()
                .name("imports_user_id_idx")
                .table(Imports::Table)
                .col(Imports::UserId)
                .to_owned(),
        )
        .await?;
        create_index(
            manager,
            Imports::Table,
            Index::create()
                .if_not_exists()
                .name("imports_expires_at_idx")
                .table(Imports::Table)
                .col(Imports::ExpiresAt)
                .to_owned(),
        )
        .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(Imports::Table).if_exists().to_owned()).await
    }
}

#[derive(DeriveIden)]
enum Imports {
    Table,
    Id,
    UserId,
    AuthVersion,
    Resource,
    Disk,
    StorageKey,
    Sha256,
    Header,
    Rows,
    Mapping,
    Status,
    Created,
    Updated,
    Failed,
    Report,
    Error,
    CreatedAt,
    ExpiresAt,
}
