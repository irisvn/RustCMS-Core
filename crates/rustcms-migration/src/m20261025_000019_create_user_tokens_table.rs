//! One-time tokens of a user (ADR 0023): password resets and invitations.
//! At most one per user and purpose; only an HMAC of the secret is stored.

use sea_orm_migration::prelude::*;

use crate::schema::{ColumnDefExt, create_index, create_table};

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
                .table(UserTokens::Table)
                .if_not_exists()
                .col(ColumnDef::new(UserTokens::Id).uuid().not_null().primary_key())
                .col(ColumnDef::new(UserTokens::UserId).big_integer().not_null())
                .col(ColumnDef::new(UserTokens::Purpose).string_len(32).not_null())
                .col(ColumnDef::new(UserTokens::Email).string_len(255).not_null())
                .col(ColumnDef::new(UserTokens::AuthVersion).big_integer().not_null())
                .col(ColumnDef::new(UserTokens::TokenHash).string_len(64).not_null())
                .col(ColumnDef::new(UserTokens::RequestId).uuid().not_null())
                .col(ColumnDef::new(UserTokens::RequestedAt).instant(manager).not_null())
                .col(ColumnDef::new(UserTokens::IssuedAt).instant(manager).not_null())
                .col(ColumnDef::new(UserTokens::ExpiresAt).instant(manager).not_null())
                .col(ColumnDef::new(UserTokens::SentAt).instant(manager).null())
                .foreign_key(
                    ForeignKey::create()
                        .from(UserTokens::Table, UserTokens::UserId)
                        .to(Users::Table, Users::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;
        create_index(
            manager,
            UserTokens::Table,
            Index::create()
                .if_not_exists()
                .name("user_tokens_user_purpose_key")
                .table(UserTokens::Table)
                .col(UserTokens::UserId)
                .col(UserTokens::Purpose)
                .unique()
                .to_owned(),
        )
        .await?;
        create_index(
            manager,
            UserTokens::Table,
            Index::create()
                .if_not_exists()
                .name("user_tokens_expires_at_idx")
                .table(UserTokens::Table)
                .col(UserTokens::ExpiresAt)
                .to_owned(),
        )
        .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(UserTokens::Table).if_exists().to_owned()).await
    }
}

#[derive(DeriveIden)]
enum UserTokens {
    Table,
    Id,
    UserId,
    Purpose,
    Email,
    AuthVersion,
    TokenHash,
    RequestId,
    RequestedAt,
    IssuedAt,
    ExpiresAt,
    SentAt,
}

#[derive(DeriveIden)]
enum Users {
    Table,
    Id,
}
