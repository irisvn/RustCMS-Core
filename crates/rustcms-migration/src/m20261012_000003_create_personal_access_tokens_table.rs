use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[sea_orm_migration::async_trait::async_trait]
impl MigrationTrait for Migration {
    fn use_transaction(&self) -> Option<bool> {
        Some(true)
    }

    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Tokens::Table)
                    .col(ColumnDef::new(Tokens::Id).big_integer().not_null().auto_increment().primary_key())
                    .col(ColumnDef::new(Tokens::UserId).big_integer().not_null())
                    .col(ColumnDef::new(Tokens::Name).string_len(255).not_null())
                    .col(ColumnDef::new(Tokens::TokenHash).string_len(64).not_null())
                    .col(ColumnDef::new(Tokens::Abilities).text().not_null())
                    .col(ColumnDef::new(Tokens::LastUsedAt).timestamp_with_time_zone().null())
                    .col(ColumnDef::new(Tokens::ExpiresAt).timestamp_with_time_zone().null())
                    .col(ColumnDef::new(Tokens::RevokedAt).timestamp_with_time_zone().null())
                    .col(
                        ColumnDef::new(Tokens::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(Tokens::Table, Tokens::UserId)
                            .to(Users::Table, Users::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("personal_access_tokens_user_id_idx")
                    .table(Tokens::Table)
                    .col(Tokens::UserId)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(Tokens::Table).to_owned()).await
    }
}

#[derive(DeriveIden)]
enum Tokens {
    #[sea_orm(iden = "personal_access_tokens")]
    Table,
    Id,
    UserId,
    Name,
    TokenHash,
    Abilities,
    LastUsedAt,
    ExpiresAt,
    RevokedAt,
    CreatedAt,
}

#[derive(DeriveIden)]
enum Users {
    Table,
    Id,
}
