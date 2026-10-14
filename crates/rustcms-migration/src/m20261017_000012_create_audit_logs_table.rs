//! The audit log: who did what, written in the transaction of the change.
//! No foreign key on the actor: entries outlive deleted users.

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
                .table(AuditLogs::Table)
                .col(ColumnDef::new(AuditLogs::Id).uuid().not_null().primary_key())
                .col(ColumnDef::new(AuditLogs::At).instant(manager).not_null())
                .col(ColumnDef::new(AuditLogs::Event).string_len(64).not_null())
                .col(ColumnDef::new(AuditLogs::SubjectType).string_len(64).not_null())
                .col(ColumnDef::new(AuditLogs::SubjectKey).string_len(255).null())
                .col(ColumnDef::new(AuditLogs::ActorKind).string_len(16).not_null())
                .col(ColumnDef::new(AuditLogs::ActorUserId).big_integer().null())
                .col(ColumnDef::new(AuditLogs::ActorGuard).string_len(32).null())
                .col(ColumnDef::new(AuditLogs::Credential).string_len(96).null())
                .col(ColumnDef::new(AuditLogs::Ip).string_len(45).null())
                .col(ColumnDef::new(AuditLogs::UserAgent).string_len(255).null())
                .col(ColumnDef::new(AuditLogs::RequestId).string_len(64).null())
                .col(ColumnDef::new(AuditLogs::Changes).json().null())
                .col(ColumnDef::new(AuditLogs::Meta).json().null())
                .to_owned(),
        )
        .await?;
        let indexes: [(&str, &[AuditLogs]); 3] = [
            ("audit_logs_subject_idx", &[AuditLogs::SubjectType, AuditLogs::SubjectKey, AuditLogs::At]),
            ("audit_logs_actor_idx", &[AuditLogs::ActorUserId, AuditLogs::At]),
            ("audit_logs_at_idx", &[AuditLogs::At, AuditLogs::Id]),
        ];
        for (name, columns) in indexes {
            let mut index = Index::create().name(name).table(AuditLogs::Table).to_owned();
            for column in columns {
                index.col(*column);
            }
            create_index(manager, AuditLogs::Table, index).await?;
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_table(manager, AuditLogs::Table).await
    }
}

#[derive(DeriveIden, Clone, Copy)]
enum AuditLogs {
    Table,
    Id,
    At,
    Event,
    SubjectType,
    SubjectKey,
    ActorKind,
    ActorUserId,
    ActorGuard,
    Credential,
    Ip,
    UserAgent,
    RequestId,
    Changes,
    Meta,
}
