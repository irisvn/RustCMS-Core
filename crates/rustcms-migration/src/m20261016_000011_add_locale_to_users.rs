//! The admin interface language a user chose (`NULL`: follow the browser).

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
        add_column(manager, Users::Table, ColumnDef::new(Users::Locale).string_len(35).null()).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_column(manager, Users::Table, Users::Locale).await
    }
}

#[derive(DeriveIden)]
enum Users {
    Table,
    Locale,
}
