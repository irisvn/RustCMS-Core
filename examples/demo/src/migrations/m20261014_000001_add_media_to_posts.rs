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
        manager
            .alter_table(
                Table::alter().table(Posts::Table).add_column(ColumnDef::new(Posts::Cover).uuid().null()).to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter().table(Posts::Table).add_column(ColumnDef::new(Posts::Content).json().null()).to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.alter_table(Table::alter().table(Posts::Table).drop_column(Posts::Content).to_owned()).await?;
        manager.alter_table(Table::alter().table(Posts::Table).drop_column(Posts::Cover).to_owned()).await
    }
}

#[derive(DeriveIden)]
enum Posts {
    Table,
    Cover,
    Content,
}
