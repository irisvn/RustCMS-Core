use rustcms::migration::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    fn use_transaction(&self) -> Option<bool> {
        Some(true)
    }

    /// What a tag groups (`topic`, `series`, `event`), shown by its label in the tags list.
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        add_column(manager, Tags::Table, ColumnDef::new(Tags::Kind).string_len(16).null()).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_column(manager, Tags::Table, Tags::Kind).await
    }
}

#[derive(DeriveIden)]
enum Tags {
    Table,
    Kind,
}
