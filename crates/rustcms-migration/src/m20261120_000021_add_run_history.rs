//! Run history (ADR 0016, 0031): when each job attempt started, a job's own
//! description, one row per finished job (`job_runs`) and per schedule run
//! (`schedule_runs`). Each step is idempotent, so a run cut on MySQL (no
//! transactional DDL) is completed by the next.

use sea_orm_migration::prelude::*;

use crate::schema::{ColumnDefExt, add_column, create_index, create_table, drop_column, drop_table};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[sea_orm_migration::async_trait::async_trait]
impl MigrationTrait for Migration {
    fn use_transaction(&self) -> Option<bool> {
        Some(true)
    }

    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Null for jobs claimed before the upgrade.
        add_column(manager, Jobs::Table, ColumnDef::new(Jobs::StartedAt).instant(manager).null()).await?;
        add_column(manager, Jobs::Table, ColumnDef::new(Jobs::Description).string_len(200).null()).await?;
        create_index(
            manager,
            Jobs::Table,
            Index::create().name("jobs_created_idx").table(Jobs::Table).col(Jobs::CreatedAt).col(Jobs::Id).to_owned(),
        )
        .await?;
        create_table(
            manager,
            Table::create()
                .table(JobRuns::Table)
                .col(ColumnDef::new(JobRuns::Id).uuid().not_null().primary_key())
                .col(ColumnDef::new(JobRuns::JobId).uuid().not_null())
                .col(ColumnDef::new(JobRuns::Queue).string_len(64).not_null())
                .col(ColumnDef::new(JobRuns::Kind).string_len(64).not_null())
                .col(ColumnDef::new(JobRuns::Description).string_len(200).null())
                .col(ColumnDef::new(JobRuns::Status).string_len(16).not_null())
                .col(ColumnDef::new(JobRuns::Attempts).integer().not_null())
                .col(ColumnDef::new(JobRuns::CreatedAt).instant(manager).not_null())
                .col(ColumnDef::new(JobRuns::StartedAt).instant(manager).null())
                .col(ColumnDef::new(JobRuns::FinishedAt).instant(manager).not_null())
                .col(ColumnDef::new(JobRuns::DurationMs).big_integer().null())
                .col(ColumnDef::new(JobRuns::LastError).string_len(500).null())
                .col(ColumnDef::new(JobRuns::InitiatedBy).big_integer().null())
                .to_owned(),
        )
        .await?;
        for (name, columns) in [
            ("job_runs_finished_idx", vec![JobRuns::FinishedAt, JobRuns::Id]),
            ("job_runs_job_idx", vec![JobRuns::JobId, JobRuns::FinishedAt]),
            ("job_runs_kind_idx", vec![JobRuns::Kind, JobRuns::FinishedAt, JobRuns::Id]),
        ] {
            let mut index = Index::create();
            index.name(name).table(JobRuns::Table);
            for column in columns {
                index.col(column);
            }
            create_index(manager, JobRuns::Table, index.to_owned()).await?;
        }
        create_table(
            manager,
            Table::create()
                .table(ScheduleRuns::Table)
                .col(ColumnDef::new(ScheduleRuns::Id).uuid().not_null().primary_key())
                .col(ColumnDef::new(ScheduleRuns::Name).string_len(64).not_null())
                .col(ColumnDef::new(ScheduleRuns::SlotAt).instant(manager).null())
                .col(ColumnDef::new(ScheduleRuns::Trigger).string_len(16).not_null())
                .col(ColumnDef::new(ScheduleRuns::StartedAt).instant(manager).not_null())
                .col(ColumnDef::new(ScheduleRuns::FinishedAt).instant(manager).null())
                .col(ColumnDef::new(ScheduleRuns::Status).string_len(16).not_null())
                .col(ColumnDef::new(ScheduleRuns::JobId).uuid().null())
                .col(ColumnDef::new(ScheduleRuns::DurationMs).big_integer().null())
                .col(ColumnDef::new(ScheduleRuns::Summary).string_len(200).null())
                .col(ColumnDef::new(ScheduleRuns::Error).string_len(500).null())
                .col(ColumnDef::new(ScheduleRuns::InitiatedBy).big_integer().null())
                .to_owned(),
        )
        .await?;
        for (name, columns) in [
            ("schedule_runs_name_idx", vec![ScheduleRuns::Name, ScheduleRuns::StartedAt, ScheduleRuns::Id]),
            ("schedule_runs_started_idx", vec![ScheduleRuns::StartedAt, ScheduleRuns::Id]),
            ("schedule_runs_job_idx", vec![ScheduleRuns::JobId]),
        ] {
            let mut index = Index::create();
            index.name(name).table(ScheduleRuns::Table);
            for column in columns {
                index.col(column);
            }
            create_index(manager, ScheduleRuns::Table, index.to_owned()).await?;
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_table(manager, ScheduleRuns::Table).await?;
        drop_table(manager, JobRuns::Table).await?;
        if manager.has_index("jobs", "jobs_created_idx").await? {
            manager.drop_index(Index::drop().name("jobs_created_idx").table(Jobs::Table).to_owned()).await?;
        }
        drop_column(manager, Jobs::Table, Jobs::Description).await?;
        drop_column(manager, Jobs::Table, Jobs::StartedAt).await
    }
}

#[derive(DeriveIden, Clone, Copy)]
enum Jobs {
    Table,
    Id,
    CreatedAt,
    StartedAt,
    Description,
}

#[derive(DeriveIden, Clone, Copy)]
enum JobRuns {
    Table,
    Id,
    JobId,
    Queue,
    Kind,
    Description,
    Status,
    Attempts,
    CreatedAt,
    StartedAt,
    FinishedAt,
    DurationMs,
    LastError,
    InitiatedBy,
}

#[derive(DeriveIden, Clone, Copy)]
enum ScheduleRuns {
    Table,
    Id,
    Name,
    SlotAt,
    Trigger,
    StartedAt,
    FinishedAt,
    Status,
    JobId,
    DurationMs,
    Summary,
    Error,
    InitiatedBy,
}
