//! JWT refresh families and their tokens. A family is one sign-in: its
//! refresh tokens rotate on every use, and the used ones are kept (as
//! tombstones) until the family ends, so replaying any of them revokes it.

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
                .table(Families::Table)
                .col(ColumnDef::new(Families::Id).string_len(32).not_null().primary_key())
                .col(ColumnDef::new(Families::UserId).big_integer().not_null())
                .col(ColumnDef::new(Families::Abilities).text().not_null())
                .col(ColumnDef::new(Families::AuthVersion).big_integer().not_null())
                .col(ColumnDef::new(Families::CreatedAt).instant(manager).not_null())
                .col(ColumnDef::new(Families::ExpiresAt).instant(manager).not_null())
                .col(ColumnDef::new(Families::RevokedAt).instant(manager).null())
                .foreign_key(
                    ForeignKey::create()
                        .name("jwt_families_user_id_fkey")
                        .from(Families::Table, Families::UserId)
                        .to(Users::Table, Users::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;
        for (name, column) in
            [("jwt_families_user_id_idx", Families::UserId), ("jwt_families_expires_at_idx", Families::ExpiresAt)]
        {
            create_index(
                manager,
                Families::Table,
                Index::create().name(name).table(Families::Table).col(column).to_owned(),
            )
            .await?;
        }
        create_table(
            manager,
            Table::create()
                .table(Tokens::Table)
                .col(ColumnDef::new(Tokens::Id).big_integer().not_null().auto_increment().primary_key())
                .col(ColumnDef::new(Tokens::FamilyId).string_len(32).not_null())
                .col(ColumnDef::new(Tokens::TokenHash).string_len(64).not_null())
                .col(ColumnDef::new(Tokens::CreatedAt).instant(manager).not_null())
                .col(ColumnDef::new(Tokens::ExpiresAt).instant(manager).not_null())
                .col(ColumnDef::new(Tokens::UsedAt).instant(manager).null())
                .foreign_key(
                    ForeignKey::create()
                        .name("jwt_refresh_tokens_family_id_fkey")
                        .from(Tokens::Table, Tokens::FamilyId)
                        .to(Families::Table, Families::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;
        create_index(
            manager,
            Tokens::Table,
            Index::create()
                .name("jwt_refresh_tokens_family_id_idx")
                .table(Tokens::Table)
                .col(Tokens::FamilyId)
                .to_owned(),
        )
        .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(Tokens::Table).if_exists().to_owned()).await?;
        manager.drop_table(Table::drop().table(Families::Table).if_exists().to_owned()).await
    }
}

#[derive(DeriveIden)]
enum Families {
    #[sea_orm(iden = "jwt_families")]
    Table,
    Id,
    UserId,
    Abilities,
    AuthVersion,
    CreatedAt,
    ExpiresAt,
    RevokedAt,
}

#[derive(DeriveIden)]
enum Tokens {
    #[sea_orm(iden = "jwt_refresh_tokens")]
    Table,
    Id,
    FamilyId,
    TokenHash,
    CreatedAt,
    ExpiresAt,
    UsedAt,
}

#[derive(DeriveIden)]
enum Users {
    Table,
    Id,
}
