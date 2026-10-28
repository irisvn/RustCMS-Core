use rustcms::migration::*;

/// Integrations: an API token and named secrets, both encrypted with
/// `APP_KEY` (`rustcms::crypto::Encrypted`, stored as text).
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    // #region up
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            manager,
            Table::create()
                .table(Integrations::Table)
                .col(ColumnDef::new(Integrations::Id).integer().not_null().auto_increment().primary_key())
                .col(ColumnDef::new(Integrations::Name).string_len(60).not_null())
                .col(ColumnDef::new(Integrations::ApiToken).text().null())
                .col(ColumnDef::new(Integrations::Env).text().null())
                .to_owned(),
        )
        .await
    }
    // #endregion up

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_table(manager, Integrations::Table).await
    }
}

#[derive(DeriveIden)]
enum Integrations {
    Table,
    Id,
    Name,
    ApiToken,
    Env,
}
