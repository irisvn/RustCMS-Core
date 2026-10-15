//! Enrollments: a learner in a course, once (deleted with either).

use rustcms::migration::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            manager,
            Table::create()
                .table(Enrollments::Table)
                .col(ColumnDef::new(Enrollments::Id).big_integer().not_null().auto_increment().primary_key())
                .col(ColumnDef::new(Enrollments::CourseId).big_integer().not_null())
                .col(ColumnDef::new(Enrollments::LearnerId).big_integer().not_null())
                .col(ColumnDef::new(Enrollments::Status).string_len(16).not_null().default("active"))
                .col(ColumnDef::new(Enrollments::Progress).integer().not_null().default(0))
                .col(ColumnDef::new(Enrollments::EnrolledAt).instant_now(manager).not_null())
                .index(
                    Index::create()
                        .name("enrollments_course_learner_key")
                        .col(Enrollments::CourseId)
                        .col(Enrollments::LearnerId)
                        .unique(),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("enrollments_course_id_fkey")
                        .from(Enrollments::Table, Enrollments::CourseId)
                        .to(Courses::Table, Courses::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("enrollments_learner_id_fkey")
                        .from(Enrollments::Table, Enrollments::LearnerId)
                        .to(Learners::Table, Learners::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_table(manager, Enrollments::Table).await
    }
}

#[derive(DeriveIden)]
enum Courses {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum Learners {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum Enrollments {
    Table,
    Id,
    CourseId,
    LearnerId,
    Status,
    Progress,
    EnrolledAt,
}
