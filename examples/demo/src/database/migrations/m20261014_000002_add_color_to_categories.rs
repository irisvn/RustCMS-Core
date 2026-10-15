use rustcms::migration::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    fn use_transaction(&self) -> Option<bool> {
        Some(true)
    }

    /// A label color for categories, edited with the demo's custom widget.
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        add_column(manager, Categories::Table, ColumnDef::new(Categories::Color).string_len(7).null()).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_column(manager, Categories::Table, Categories::Color).await
    }
}

#[derive(DeriveIden)]
enum Categories {
    Table,
    Color,
}
