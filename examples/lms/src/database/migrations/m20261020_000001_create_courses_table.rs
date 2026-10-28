//! Courses, each with a capacity and an instructor (a user).

use rustcms::migration::*;

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
                .table(Courses::Table)
                .col(ColumnDef::new(Courses::Id).big_integer().not_null().auto_increment().primary_key())
                .col(ColumnDef::new(Courses::Title).string_len(200).not_null())
                .col(ColumnDef::new(Courses::Slug).string_len(120).not_null())
                .col(ColumnDef::new(Courses::Summary).text().null())
                .col(ColumnDef::new(Courses::Capacity).integer().not_null())
                .col(ColumnDef::new(Courses::Published).boolean().not_null().default(false))
                .col(ColumnDef::new(Courses::InstructorId).big_integer().null())
                .col(ColumnDef::new(Courses::CreatedAt).instant_now(manager).not_null())
                .index(Index::create().name("courses_slug_key").col(Courses::Slug).unique())
                .foreign_key(
                    ForeignKey::create()
                        .name("courses_instructor_id_fkey")
                        .from(Courses::Table, Courses::InstructorId)
                        .to(Users::Table, Users::Id)
                        .on_delete(ForeignKeyAction::SetNull),
                )
                .to_owned(),
        )
        .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_table(manager, Courses::Table).await
    }
}

#[derive(DeriveIden)]
enum Users {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum Courses {
    Table,
    Id,
    Title,
    Slug,
    Summary,
    Capacity,
    Published,
    InstructorId,
    CreatedAt,
}
