use rustcms::migration::*;

/// An image and an explanation (rich text) for each quiz option.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    fn use_transaction(&self) -> Option<bool> {
        Some(true)
    }

    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        add_column(manager, QuizOptions::Table, ColumnDef::new(QuizOptions::Image).uuid().null()).await?;
        add_column(manager, QuizOptions::Table, ColumnDef::new(QuizOptions::Explanation).json().null()).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_column(manager, QuizOptions::Table, QuizOptions::Explanation).await?;
        drop_column(manager, QuizOptions::Table, QuizOptions::Image).await
    }
}

#[derive(DeriveIden)]
enum QuizOptions {
    Table,
    Image,
    Explanation,
}
