use rustcms::migration::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    fn use_transaction(&self) -> Option<bool> {
        Some(true)
    }

    /// The tag a series or an event is part of (a topic, a series), chosen
    /// from options computed on the server (`options_fn`).
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        add_column(manager, Tags::Table, ColumnDef::new(Tags::ParentId).integer().null()).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_column(manager, Tags::Table, Tags::ParentId).await
    }
}

#[derive(DeriveIden)]
enum Tags {
    Table,
    ParentId,
}
