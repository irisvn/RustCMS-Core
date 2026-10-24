//! Alternative text of media files (`NULL`: none given). Idempotent, so a
//! run cut on MySQL (no transactional DDL) is completed by the next.

use sea_orm_migration::prelude::*;

use crate::schema::{add_column, drop_column};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[sea_orm_migration::async_trait::async_trait]
impl MigrationTrait for Migration {
    fn use_transaction(&self) -> Option<bool> {
        Some(true)
    }

    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        add_column(manager, Media::Table, ColumnDef::new(Media::Alt).string_len(500).null()).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_column(manager, Media::Table, Media::Alt).await
    }
}

#[derive(DeriveIden)]
enum Media {
    Table,
    Alt,
}
