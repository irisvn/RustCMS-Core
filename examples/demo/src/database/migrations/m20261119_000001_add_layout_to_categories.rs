use rustcms::migration::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    fn use_transaction(&self) -> Option<bool> {
        Some(true)
    }

    /// What a category's page shows: its posts (`posts`, the default) or a link elsewhere (`link`, with `link_url`).
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        add_column(
            manager,
            Categories::Table,
            ColumnDef::new(Categories::Layout).string_len(8).not_null().default("posts"),
        )
        .await?;
        add_column(manager, Categories::Table, ColumnDef::new(Categories::LinkUrl).string_len(255).null()).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_column(manager, Categories::Table, Categories::LinkUrl).await?;
        drop_column(manager, Categories::Table, Categories::Layout).await
    }
}

#[derive(DeriveIden)]
enum Categories {
    Table,
    Layout,
    LinkUrl,
}
