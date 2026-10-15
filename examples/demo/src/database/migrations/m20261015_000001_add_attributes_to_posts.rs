use rustcms::migration::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    fn use_transaction(&self) -> Option<bool> {
        Some(true)
    }

    /// Free-form attributes of a post (`{"reading_time": "5 min"}`), edited as key/value pairs.
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        add_column(manager, Posts::Table, ColumnDef::new(Posts::Attributes).json().null()).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_column(manager, Posts::Table, Posts::Attributes).await
    }
}

#[derive(DeriveIden)]
enum Posts {
    Table,
    Attributes,
}
