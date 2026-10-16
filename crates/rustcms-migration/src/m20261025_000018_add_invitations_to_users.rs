//! Invitations (ADR 0023): a user invited by an administrator, who sets a
//! password from the invitation mail. Each column is added on its own, so a
//! run cut on MySQL (no transactional DDL) is completed by the next.

use sea_orm_migration::prelude::*;

use crate::schema::{ColumnDefExt, add_column, drop_column};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[sea_orm_migration::async_trait::async_trait]
impl MigrationTrait for Migration {
    fn use_transaction(&self) -> Option<bool> {
        Some(true)
    }

    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        add_column(manager, Users::Table, ColumnDef::new(Users::InvitationPending).boolean().not_null().default(false))
            .await?;
        add_column(manager, Users::Table, ColumnDef::new(Users::InvitedAt).instant(manager).null()).await?;
        // No foreign key: the user who invited may be deleted.
        add_column(manager, Users::Table, ColumnDef::new(Users::InvitedBy).big_integer().null()).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_column(manager, Users::Table, Users::InvitedBy).await?;
        drop_column(manager, Users::Table, Users::InvitedAt).await?;
        drop_column(manager, Users::Table, Users::InvitationPending).await
    }
}

#[derive(DeriveIden)]
enum Users {
    Table,
    InvitationPending,
    InvitedAt,
    InvitedBy,
}
