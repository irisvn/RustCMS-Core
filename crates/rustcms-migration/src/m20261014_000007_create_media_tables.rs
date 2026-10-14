use sea_orm_migration::prelude::*;

use crate::schema::{ColumnDefExt, create_index, create_table};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[sea_orm_migration::async_trait::async_trait]
impl MigrationTrait for Migration {
    fn use_transaction(&self) -> Option<bool> {
        Some(true)
    }

    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            manager,
            Table::create()
                .table(Media::Table)
                .if_not_exists()
                .col(ColumnDef::new(Media::Id).uuid().not_null().primary_key())
                .col(ColumnDef::new(Media::Disk).string_len(16).not_null())
                .col(ColumnDef::new(Media::Key).string_len(128).not_null())
                .col(ColumnDef::new(Media::Name).string_len(255).not_null())
                .col(ColumnDef::new(Media::Mime).string_len(64).not_null())
                .col(ColumnDef::new(Media::Size).big_integer().not_null())
                .col(ColumnDef::new(Media::Sha256).string_len(64).not_null())
                .col(ColumnDef::new(Media::Width).integer())
                .col(ColumnDef::new(Media::Height).integer())
                .col(ColumnDef::new(Media::OwnerId).big_integer().not_null())
                .col(ColumnDef::new(Media::CreatedAt).instant(manager).not_null())
                .col(ColumnDef::new(Media::DeletedAt).instant(manager))
                .to_owned(),
        )
        .await?;
        create_index(
            manager,
            Media::Table,
            Index::create()
                .if_not_exists()
                .name("media_owner_id_idx")
                .table(Media::Table)
                .col(Media::OwnerId)
                .to_owned(),
        )
        .await?;
        // Objects being stored: an intent is committed before the object is
        // written and removed with the media row, so a crash in between leaves
        // a record of what to clean up.
        create_table(
            manager,
            Table::create()
                .table(MediaUploads::Table)
                .if_not_exists()
                .col(ColumnDef::new(MediaUploads::Id).uuid().not_null().primary_key())
                .col(ColumnDef::new(MediaUploads::Disk).string_len(16).not_null())
                .col(ColumnDef::new(MediaUploads::Key).string_len(128).not_null())
                .col(ColumnDef::new(MediaUploads::CreatedAt).instant(manager).not_null())
                .to_owned(),
        )
        .await?;
        // Objects to delete (queued with the row's deletion, retried until done).
        create_table(
            manager,
            Table::create()
                .table(MediaDeletions::Table)
                .if_not_exists()
                .col(ColumnDef::new(MediaDeletions::Id).uuid().not_null().primary_key())
                .col(ColumnDef::new(MediaDeletions::Disk).string_len(16).not_null())
                .col(ColumnDef::new(MediaDeletions::Key).string_len(128).not_null())
                .col(ColumnDef::new(MediaDeletions::Attempts).integer().not_null().default(0))
                .col(ColumnDef::new(MediaDeletions::NextAt).instant(manager).not_null())
                .to_owned(),
        )
        .await?;
        create_index(
            manager,
            MediaDeletions::Table,
            Index::create()
                .if_not_exists()
                .name("media_deletions_next_at_idx")
                .table(MediaDeletions::Table)
                .col(MediaDeletions::NextAt)
                .to_owned(),
        )
        .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for table in [MediaDeletions::Table.into_iden(), MediaUploads::Table.into_iden(), Media::Table.into_iden()] {
            manager.drop_table(Table::drop().table(table).if_exists().to_owned()).await?;
        }
        Ok(())
    }
}

#[derive(DeriveIden)]
enum Media {
    Table,
    Id,
    Disk,
    Key,
    Name,
    Mime,
    Size,
    Sha256,
    Width,
    Height,
    OwnerId,
    CreatedAt,
    DeletedAt,
}

#[derive(DeriveIden)]
enum MediaUploads {
    Table,
    Id,
    Disk,
    Key,
    CreatedAt,
}

#[derive(DeriveIden)]
enum MediaDeletions {
    Table,
    Id,
    Disk,
    Key,
    Attempts,
    NextAt,
}
