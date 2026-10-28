//! Lessons of a course, in order (deleted with their course).

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
                .table(Lessons::Table)
                .col(ColumnDef::new(Lessons::Id).big_integer().not_null().auto_increment().primary_key())
                .col(ColumnDef::new(Lessons::CourseId).big_integer().not_null())
                .col(ColumnDef::new(Lessons::Title).string_len(200).not_null())
                .col(ColumnDef::new(Lessons::Position).integer().not_null().default(0))
                .col(ColumnDef::new(Lessons::Body).text().null())
                .foreign_key(
                    ForeignKey::create()
                        .name("lessons_course_id_fkey")
                        .from(Lessons::Table, Lessons::CourseId)
                        .to(Courses::Table, Courses::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_table(manager, Lessons::Table).await
    }
}

#[derive(DeriveIden)]
enum Courses {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum Lessons {
    Table,
    Id,
    CourseId,
    Title,
    Position,
    Body,
}
