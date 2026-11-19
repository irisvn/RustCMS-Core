use rustcms::migration::*;

/// Quiz questions of a lesson (ADR 0044's example): a question, its blanks,
/// and options, which are the choices of a choice question (`blank_id` null)
/// or of one blank (`blank_id` set; the blank belongs to the same question,
/// a foreign key on both columns).
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    fn use_transaction(&self) -> Option<bool> {
        Some(true)
    }

    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            manager,
            Table::create()
                .table(QuizQuestions::Table)
                .col(ColumnDef::new(QuizQuestions::Id).big_integer().not_null().auto_increment().primary_key())
                .col(ColumnDef::new(QuizQuestions::LessonId).big_integer().not_null())
                .col(ColumnDef::new(QuizQuestions::Kind).string_len(16).not_null().default("single"))
                .col(ColumnDef::new(QuizQuestions::Prompt).string_len(1000).not_null())
                .col(ColumnDef::new(QuizQuestions::Explain).boolean().not_null().default(false))
                .foreign_key(
                    ForeignKey::create()
                        .name("quiz_questions_lesson_id_fkey")
                        .from(QuizQuestions::Table, QuizQuestions::LessonId)
                        .to(Lessons::Table, Lessons::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;
        create_table(
            manager,
            Table::create()
                .table(QuizBlanks::Table)
                .col(ColumnDef::new(QuizBlanks::Id).big_integer().not_null().auto_increment().primary_key())
                .col(ColumnDef::new(QuizBlanks::QuizQuestionId).big_integer().not_null())
                .col(ColumnDef::new(QuizBlanks::Label).string_len(80).not_null())
                .col(ColumnDef::new(QuizBlanks::Position).integer().not_null())
                // What the options' foreign key on both columns references.
                .index(
                    Index::create()
                        .name("quiz_blanks_id_question_key")
                        .col(QuizBlanks::Id)
                        .col(QuizBlanks::QuizQuestionId)
                        .unique(),
                )
                .index(
                    Index::create()
                        .name("quiz_blanks_position_key")
                        .col(QuizBlanks::QuizQuestionId)
                        .col(QuizBlanks::Position)
                        .unique(),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("quiz_blanks_quiz_question_id_fkey")
                        .from(QuizBlanks::Table, QuizBlanks::QuizQuestionId)
                        .to(QuizQuestions::Table, QuizQuestions::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;
        create_table(
            manager,
            Table::create()
                .table(QuizOptions::Table)
                .col(ColumnDef::new(QuizOptions::Id).big_integer().not_null().auto_increment().primary_key())
                .col(ColumnDef::new(QuizOptions::QuizQuestionId).big_integer().not_null())
                .col(ColumnDef::new(QuizOptions::BlankId).big_integer().null())
                .col(ColumnDef::new(QuizOptions::Content).string_len(500).not_null())
                .col(ColumnDef::new(QuizOptions::Correct).boolean().not_null().default(false))
                .col(ColumnDef::new(QuizOptions::Hint).string_len(500).null())
                .col(ColumnDef::new(QuizOptions::Position).integer().not_null())
                // One position per question, whatever the blank (repeaters order every row).
                .index(
                    Index::create()
                        .name("quiz_options_position_key")
                        .col(QuizOptions::QuizQuestionId)
                        .col(QuizOptions::Position)
                        .unique(),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("quiz_options_quiz_question_id_fkey")
                        .from(QuizOptions::Table, QuizOptions::QuizQuestionId)
                        .to(QuizQuestions::Table, QuizQuestions::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("quiz_options_blank_fkey")
                        .from(QuizOptions::Table, (QuizOptions::BlankId, QuizOptions::QuizQuestionId))
                        .to(QuizBlanks::Table, (QuizBlanks::Id, QuizBlanks::QuizQuestionId))
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_table(manager, QuizOptions::Table).await?;
        drop_table(manager, QuizBlanks::Table).await?;
        drop_table(manager, QuizQuestions::Table).await
    }
}

#[derive(DeriveIden)]
enum Lessons {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum QuizQuestions {
    Table,
    Id,
    LessonId,
    Kind,
    Prompt,
    Explain,
}

#[derive(DeriveIden)]
enum QuizBlanks {
    Table,
    Id,
    QuizQuestionId,
    Label,
    Position,
}

#[derive(DeriveIden)]
enum QuizOptions {
    Table,
    Id,
    QuizQuestionId,
    BlankId,
    Content,
    Correct,
    Hint,
    Position,
}
