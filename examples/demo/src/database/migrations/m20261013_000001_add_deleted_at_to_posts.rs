use rustcms::migration::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    fn use_transaction(&self) -> Option<bool> {
        Some(true)
    }

    /// Soft deletes for posts: deleting sets `deleted_at` (see `PostResource`).
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        add_column(manager, Posts::Table, ColumnDef::new(Posts::DeletedAt).instant(manager).null()).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_column(manager, Posts::Table, Posts::DeletedAt).await
    }
}

#[derive(DeriveIden)]
enum Posts {
    Table,
    DeletedAt,
}
