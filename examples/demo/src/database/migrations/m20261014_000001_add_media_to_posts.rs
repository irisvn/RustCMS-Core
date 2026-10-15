use rustcms::migration::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    fn use_transaction(&self) -> Option<bool> {
        Some(true)
    }

    /// A cover image (media id) and rich text content (a JSON document) for posts.
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        add_column(manager, Posts::Table, ColumnDef::new(Posts::Cover).uuid().null()).await?;
        add_column(manager, Posts::Table, ColumnDef::new(Posts::Content).json().null()).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_column(manager, Posts::Table, Posts::Content).await?;
        drop_column(manager, Posts::Table, Posts::Cover).await
    }
}

#[derive(DeriveIden)]
enum Posts {
    Table,
    Cover,
    Content,
}
