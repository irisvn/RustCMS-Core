use rustcms::migration::*;

/// A category's landing page: sections, each with its links (edited in the
/// category's form, rows of rows). Deleted with their category or section.
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
                .table(CategorySections::Table)
                .col(ColumnDef::new(CategorySections::Id).integer().not_null().auto_increment().primary_key())
                .col(ColumnDef::new(CategorySections::CategoryId).integer().not_null())
                .col(ColumnDef::new(CategorySections::Title).string_len(120).not_null())
                .col(ColumnDef::new(CategorySections::Style).string_len(20).not_null().default("compact"))
                .col(ColumnDef::new(CategorySections::Position).integer().not_null())
                .foreign_key(
                    ForeignKey::create()
                        .name("category_sections_category_id_fkey")
                        .from(CategorySections::Table, CategorySections::CategoryId)
                        .to(Categories::Table, Categories::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;
        create_table(
            manager,
            Table::create()
                .table(SectionLinks::Table)
                .col(ColumnDef::new(SectionLinks::Id).integer().not_null().auto_increment().primary_key())
                .col(ColumnDef::new(SectionLinks::SectionId).integer().not_null())
                .col(ColumnDef::new(SectionLinks::Label).string_len(80).not_null())
                .col(ColumnDef::new(SectionLinks::Url).string_len(255).not_null())
                .col(ColumnDef::new(SectionLinks::Note).string_len(255).null())
                .col(ColumnDef::new(SectionLinks::Position).integer().not_null())
                .foreign_key(
                    ForeignKey::create()
                        .name("section_links_section_id_fkey")
                        .from(SectionLinks::Table, SectionLinks::SectionId)
                        .to(CategorySections::Table, CategorySections::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_table(manager, SectionLinks::Table).await?;
        drop_table(manager, CategorySections::Table).await
    }
}

#[derive(DeriveIden)]
enum CategorySections {
    Table,
    Id,
    CategoryId,
    Title,
    Style,
    Position,
}

#[derive(DeriveIden)]
enum SectionLinks {
    Table,
    Id,
    SectionId,
    Label,
    Url,
    Note,
    Position,
}

#[derive(DeriveIden)]
enum Categories {
    Table,
    Id,
}
