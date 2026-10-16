use rustcms::migration::*;

/// News of the school: categories, tags, articles (rich text, a cover image,
/// a unique slug, a publication time: future means scheduled) and the
/// `news_tags` pivot.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            manager,
            Table::create()
                .table(Categories::Table)
                .col(ColumnDef::new(Categories::Id).big_integer().not_null().auto_increment().primary_key())
                .col(ColumnDef::new(Categories::Name).string_len(80).not_null())
                .index(Index::create().name("categories_name_key").col(Categories::Name).unique())
                .to_owned(),
        )
        .await?;
        create_table(
            manager,
            Table::create()
                .table(Tags::Table)
                .col(ColumnDef::new(Tags::Id).big_integer().not_null().auto_increment().primary_key())
                .col(ColumnDef::new(Tags::Name).string_len(60).not_null())
                .index(Index::create().name("tags_name_key").col(Tags::Name).unique())
                .to_owned(),
        )
        .await?;
        create_table(
            manager,
            Table::create()
                .table(News::Table)
                .col(ColumnDef::new(News::Id).big_integer().not_null().auto_increment().primary_key())
                .col(ColumnDef::new(News::Title).string_len(200).not_null())
                .col(ColumnDef::new(News::Slug).string_len(200).not_null())
                .col(ColumnDef::new(News::Body).json().null())
                .col(ColumnDef::new(News::Cover).uuid().null())
                .col(ColumnDef::new(News::CategoryId).big_integer().null())
                .col(ColumnDef::new(News::AuthorId).big_integer().not_null())
                .col(ColumnDef::new(News::PublishedAt).instant(manager).null())
                .col(ColumnDef::new(News::CreatedAt).instant(manager).not_null())
                .index(Index::create().name("news_slug_key").col(News::Slug).unique())
                .foreign_key(
                    ForeignKey::create()
                        .name("news_category_id_fkey")
                        .from(News::Table, News::CategoryId)
                        .to(Categories::Table, Categories::Id)
                        .on_delete(ForeignKeyAction::SetNull),
                )
                .to_owned(),
        )
        .await?;
        create_table(
            manager,
            Table::create()
                .table(NewsTags::Table)
                .col(ColumnDef::new(NewsTags::NewsId).big_integer().not_null())
                .col(ColumnDef::new(NewsTags::TagId).big_integer().not_null())
                .primary_key(Index::create().col(NewsTags::NewsId).col(NewsTags::TagId))
                .foreign_key(
                    ForeignKey::create()
                        .name("news_tags_news_id_fkey")
                        .from(NewsTags::Table, NewsTags::NewsId)
                        .to(News::Table, News::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("news_tags_tag_id_fkey")
                        .from(NewsTags::Table, NewsTags::TagId)
                        .to(Tags::Table, Tags::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_table(manager, NewsTags::Table).await?;
        drop_table(manager, News::Table).await?;
        drop_table(manager, Tags::Table).await?;
        drop_table(manager, Categories::Table).await
    }
}

#[derive(DeriveIden)]
enum Categories {
    Table,
    Id,
    Name,
}

#[derive(DeriveIden)]
enum Tags {
    Table,
    Id,
    Name,
}

#[derive(DeriveIden)]
enum News {
    Table,
    Id,
    Title,
    Slug,
    Body,
    Cover,
    CategoryId,
    AuthorId,
    PublishedAt,
    CreatedAt,
}

#[derive(DeriveIden)]
enum NewsTags {
    Table,
    NewsId,
    TagId,
}
