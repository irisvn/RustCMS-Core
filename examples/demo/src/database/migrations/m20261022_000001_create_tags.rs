use rustcms::migration::*;

/// Tags, and the `post_tags` pivot linking posts to them (a belongs-to-many
/// relation): one row per link, deleted with either side.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            manager,
            Table::create()
                .table(Tags::Table)
                .col(ColumnDef::new(Tags::Id).integer().not_null().auto_increment().primary_key())
                .col(ColumnDef::new(Tags::Name).string_len(60).not_null())
                .index(Index::create().name("tags_name_key").col(Tags::Name).unique())
                .to_owned(),
        )
        .await?;
        // #region post-tags
        create_table(
            manager,
            Table::create()
                .table(PostTags::Table)
                .col(ColumnDef::new(PostTags::PostId).big_integer().not_null())
                .col(ColumnDef::new(PostTags::TagId).integer().not_null())
                .col(ColumnDef::new(PostTags::Position).integer().not_null().default(0))
                .primary_key(Index::create().col(PostTags::PostId).col(PostTags::TagId))
                .foreign_key(
                    ForeignKey::create()
                        .name("post_tags_post_id_fkey")
                        .from(PostTags::Table, PostTags::PostId)
                        .to(Posts::Table, Posts::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("post_tags_tag_id_fkey")
                        .from(PostTags::Table, PostTags::TagId)
                        .to(Tags::Table, Tags::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await
        // #endregion post-tags
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_table(manager, PostTags::Table).await?;
        drop_table(manager, Tags::Table).await
    }
}

#[derive(DeriveIden)]
enum Tags {
    Table,
    Id,
    Name,
}

#[derive(DeriveIden)]
enum PostTags {
    Table,
    PostId,
    TagId,
    Position,
}

#[derive(DeriveIden)]
enum Posts {
    Table,
    Id,
}
