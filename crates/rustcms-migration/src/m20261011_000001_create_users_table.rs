use sea_orm_migration::prelude::*;

use crate::schema::{ColumnDefExt, create_index, create_table};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[sea_orm_migration::async_trait::async_trait]
impl MigrationTrait for Migration {
    /// Schema change and migration record commit together, also on SQLite
    /// (whose default is no transaction), so a failed run can be retried.
    fn use_transaction(&self) -> Option<bool> {
        Some(true)
    }

    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            manager,
            Table::create()
                .table(Users::Table)
                .col(ColumnDef::new(Users::Id).big_integer().not_null().auto_increment().primary_key())
                .col(ColumnDef::new(Users::Name).string_len(255).not_null())
                .col(ColumnDef::new(Users::Email).string_len(255).not_null())
                .col(ColumnDef::new(Users::Password).string_len(255).not_null())
                .col(ColumnDef::new(Users::IsActive).boolean().not_null().default(true))
                .col(ColumnDef::new(Users::AuthVersion).big_integer().not_null().default(0))
                .col(ColumnDef::new(Users::EmailVerifiedAt).instant(manager).null())
                .col(ColumnDef::new(Users::CreatedAt).instant_now(manager).not_null())
                .col(ColumnDef::new(Users::UpdatedAt).instant_now(manager).not_null())
                .to_owned(),
        )
        .await?;
        create_index(
            manager,
            Users::Table,
            Index::create().name("users_email_key").table(Users::Table).col(Users::Email).unique().to_owned(),
        )
        .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(Users::Table).if_exists().to_owned()).await
    }
}

#[derive(DeriveIden)]
enum Users {
    Table,
    Id,
    Name,
    Email,
    Password,
    IsActive,
    AuthVersion,
    EmailVerifiedAt,
    CreatedAt,
    UpdatedAt,
}
