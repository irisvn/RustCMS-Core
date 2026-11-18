//! The collection of media files (`NULL`: none), with an index for listing a
//! collection newest first. Idempotent, so a run cut on MySQL (no
//! transactional DDL) is completed by the next.

use sea_orm_migration::prelude::*;

use crate::schema::{add_column, create_index, drop_column, drop_index};

#[derive(DeriveMigrationName)]
pub struct Migration;

/// The index of the collection's listing.
const INDEX: &str = "media_collection_created_at_idx";

#[sea_orm_migration::async_trait::async_trait]
impl MigrationTrait for Migration {
    fn use_transaction(&self) -> Option<bool> {
        Some(true)
    }

    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        add_column(manager, Media::Table, ColumnDef::new(Media::Collection).string_len(64).null()).await?;
        let index =
            Index::create().name(INDEX).table(Media::Table).col(Media::Collection).col(Media::CreatedAt).to_owned();
        create_index(manager, Media::Table, index).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // SQLite refuses to drop a column an index still uses.
        drop_index(manager, Media::Table, INDEX).await?;
        drop_column(manager, Media::Table, Media::Collection).await
    }
}

#[derive(DeriveIden)]
enum Media {
    Table,
    Collection,
    CreatedAt,
}
