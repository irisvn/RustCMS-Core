use rustcms::migration::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
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
                .table(Posts::Table)
                .col(ColumnDef::new(Posts::Id).big_integer().not_null().auto_increment().primary_key())
                .col(ColumnDef::new(Posts::CreatedAt).instant_now(manager).not_null())
                .col(ColumnDef::new(Posts::UpdatedAt).instant_now(manager).not_null())
                .to_owned(),
        )
        .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(Posts::Table).to_owned()).await
    }
}

#[derive(DeriveIden)]
enum Posts {
    Table,
    Id,
    CreatedAt,
    UpdatedAt,
}
