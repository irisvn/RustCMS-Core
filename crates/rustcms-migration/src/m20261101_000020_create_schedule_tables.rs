//! The scheduler (ADR 0031): one state row per schedule (locked by every claim,
//! never pruned) and one row per claimed slot (pruned after 7 days).

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
                .table(ScheduleStates::Table)
                .col(ColumnDef::new(ScheduleStates::Name).string_len(64).not_null().primary_key())
                .col(ColumnDef::new(ScheduleStates::ActiveJobId).uuid().null())
                .col(ColumnDef::new(ScheduleStates::LastSlotAt).instant(manager).null())
                .col(ColumnDef::new(ScheduleStates::UpdatedAt).instant(manager).not_null())
                .to_owned(),
        )
        .await?;
        create_table(
            manager,
            Table::create()
                .table(ScheduleSlots::Table)
                .col(ColumnDef::new(ScheduleSlots::Name).string_len(64).not_null())
                .col(ColumnDef::new(ScheduleSlots::SlotAt).instant(manager).not_null())
                .col(ColumnDef::new(ScheduleSlots::ClaimedAt).instant(manager).not_null())
                .col(ColumnDef::new(ScheduleSlots::Instance).string_len(64).not_null())
                .col(ColumnDef::new(ScheduleSlots::JobId).uuid().null())
                .primary_key(Index::create().col(ScheduleSlots::Name).col(ScheduleSlots::SlotAt))
                .to_owned(),
        )
        .await?;
        create_index(
            manager,
            ScheduleSlots::Table,
            Index::create()
                .name("schedule_slots_claimed_idx")
                .table(ScheduleSlots::Table)
                .col(ScheduleSlots::ClaimedAt)
                .to_owned(),
        )
        .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_table(manager, ScheduleSlots::Table).await?;
        drop_table(manager, ScheduleStates::Table).await
    }
}

#[derive(DeriveIden, Clone, Copy)]
enum ScheduleStates {
    Table,
    Name,
    ActiveJobId,
    LastSlotAt,
    UpdatedAt,
}

#[derive(DeriveIden, Clone, Copy)]
enum ScheduleSlots {
    Table,
    Name,
    SlotAt,
    ClaimedAt,
    Instance,
    JobId,
}
