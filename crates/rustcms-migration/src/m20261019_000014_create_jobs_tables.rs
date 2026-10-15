//! The job queue (ADR 0016): jobs waiting, running or failed, and receipts
//! that let a job's database effects apply once.

use sea_orm_migration::prelude::*;

use crate::schema::{ColumnDefExt, create_index, create_table, drop_table};

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
                .table(Jobs::Table)
                .col(ColumnDef::new(Jobs::Id).uuid().not_null().primary_key())
                .col(ColumnDef::new(Jobs::Queue).string_len(64).not_null())
                .col(ColumnDef::new(Jobs::Kind).string_len(64).not_null())
                .col(ColumnDef::new(Jobs::Version).integer().not_null())
                .col(ColumnDef::new(Jobs::Payload).json().not_null())
                .col(ColumnDef::new(Jobs::Attempts).integer().not_null())
                .col(ColumnDef::new(Jobs::MaxAttempts).integer().not_null())
                .col(ColumnDef::new(Jobs::AvailableAt).instant(manager).not_null())
                .col(ColumnDef::new(Jobs::ReservedBy).string_len(64).null())
                .col(ColumnDef::new(Jobs::ReservedUntil).instant(manager).null())
                .col(ColumnDef::new(Jobs::LastError).string_len(500).null())
                .col(ColumnDef::new(Jobs::InitiatedBy).big_integer().null())
                .col(ColumnDef::new(Jobs::CreatedAt).instant(manager).not_null())
                .col(ColumnDef::new(Jobs::FailedAt).instant(manager).null())
                .to_owned(),
        )
        .await?;
        let indexes: [(&str, &[Jobs]); 2] = [
            ("jobs_ready_idx", &[Jobs::Queue, Jobs::FailedAt, Jobs::AvailableAt]),
            ("jobs_reserved_idx", &[Jobs::ReservedUntil]),
        ];
        for (name, columns) in indexes {
            let mut index = Index::create().name(name).table(Jobs::Table).to_owned();
            for column in columns {
                index.col(*column);
            }
            create_index(manager, Jobs::Table, index).await?;
        }
        create_table(
            manager,
            Table::create()
                .table(JobReceipts::Table)
                .col(ColumnDef::new(JobReceipts::Key).string_len(191).not_null().primary_key())
                .col(ColumnDef::new(JobReceipts::CreatedAt).instant(manager).not_null())
                .to_owned(),
        )
        .await?;
        create_index(
            manager,
            JobReceipts::Table,
            Index::create()
                .name("job_receipts_created_idx")
                .table(JobReceipts::Table)
                .col(JobReceipts::CreatedAt)
                .to_owned(),
        )
        .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_table(manager, JobReceipts::Table).await?;
        drop_table(manager, Jobs::Table).await
    }
}

#[derive(DeriveIden, Clone, Copy)]
enum Jobs {
    Table,
    Id,
    Queue,
    Kind,
    Version,
    Payload,
    Attempts,
    MaxAttempts,
    AvailableAt,
    ReservedBy,
    ReservedUntil,
    LastError,
    InitiatedBy,
    CreatedAt,
    FailedAt,
}

#[derive(DeriveIden, Clone, Copy)]
enum JobReceipts {
    Table,
    Key,
    CreatedAt,
}
