use sea_orm_migration::prelude::*;

use crate::schema::{ColumnDefExt, add_column, create_index, create_table, drop_column, drop_table};

/// Receipts of imported rows (one per row written, committed with it), the
/// time an import started running, and every object an export run may have
/// written (ADR 0021).
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
                .table(ImportRows::Table)
                .if_not_exists()
                .col(ColumnDef::new(ImportRows::ImportId).uuid().not_null())
                .col(ColumnDef::new(ImportRows::Row).integer().not_null())
                .col(ColumnDef::new(ImportRows::Outcome).string_len(16).not_null())
                .col(ColumnDef::new(ImportRows::RecordKey).string_len(255).not_null())
                .primary_key(Index::create().col(ImportRows::ImportId).col(ImportRows::Row))
                .foreign_key(
                    ForeignKey::create()
                        .from(ImportRows::Table, ImportRows::ImportId)
                        .to(Imports::Table, Imports::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;
        add_column(manager, Imports::Table, ColumnDef::new(Imports::StartedAt).instant(manager).null()).await?;
        create_table(
            manager,
            Table::create()
                .table(ExportObjects::Table)
                .if_not_exists()
                .col(ColumnDef::new(ExportObjects::StorageKey).string_len(128).not_null().primary_key())
                .col(ColumnDef::new(ExportObjects::ExportId).uuid().not_null())
                .col(ColumnDef::new(ExportObjects::Disk).string_len(16).not_null())
                .col(ColumnDef::new(ExportObjects::CreatedAt).instant(manager).not_null())
                .to_owned(),
        )
        .await?;
        create_index(
            manager,
            ExportObjects::Table,
            Index::create()
                .if_not_exists()
                .name("export_objects_export_id_idx")
                .table(ExportObjects::Table)
                .col(ExportObjects::ExportId)
                .to_owned(),
        )
        .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_table(manager, ExportObjects::Table).await?;
        drop_column(manager, Imports::Table, Imports::StartedAt).await?;
        drop_table(manager, ImportRows::Table).await
    }
}

#[derive(DeriveIden)]
enum ImportRows {
    Table,
    ImportId,
    Row,
    Outcome,
    RecordKey,
}

#[derive(DeriveIden)]
enum Imports {
    Table,
    Id,
    StartedAt,
}

#[derive(DeriveIden)]
enum ExportObjects {
    Table,
    StorageKey,
    ExportId,
    Disk,
    CreatedAt,
}
