//! A quiz on the first lesson of the first course: a choice question with its
//! options, and a fill-in-the-blank question whose blanks have choices.

use rustcms::database::Seeder;
use rustcms::{AppState, async_trait};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, QueryOrder, Set};

use super::course_seeder::COURSES;
use crate::models::{course, lesson, quiz_blank, quiz_option, quiz_question};

/// The choice question: its prompt and options (`(content, correct)`).
const CHOICE: (&str, [(&str, bool); 3]) =
    ("Which keyword declares a variable?", [("let", true), ("var", false), ("const", false)]);
/// A blank: its label and choices (`(content, correct)`).
type Blank = (&'static str, [(&'static str, bool); 2]);
/// The blanks question: its prompt and blanks, each with its choices.
const BLANKS: (&str, [Blank; 2]) = (
    "Rust's build tool is ___ and its package registry is ___.",
    [("tool", [("cargo", true), ("rustup", false)]), ("registry", [("crates.io", true), ("npm", false)])],
);

pub struct QuizSeeder;

#[async_trait]
impl Seeder for QuizSeeder {
    fn name(&self) -> &'static str {
        "quiz"
    }

    async fn run(&self, state: &AppState) -> anyhow::Result<()> {
        let db = state.db();
        let Some(course) = course::Entity::find().filter(course::Column::Slug.eq(COURSES[0].1)).one(db).await? else {
            return Ok(());
        };
        let Some(lesson) = lesson::Entity::find()
            .filter(lesson::Column::CourseId.eq(course.id))
            .order_by_asc(lesson::Column::Id)
            .one(db)
            .await?
        else {
            return Ok(());
        };
        let question = |kind: &'static str, prompt: &'static str| async move {
            let found = quiz_question::Entity::find()
                .filter(quiz_question::Column::LessonId.eq(lesson.id))
                .filter(quiz_question::Column::Prompt.eq(prompt))
                .one(db)
                .await?;
            if found.is_some() {
                return Ok::<_, sea_orm::DbErr>(None);
            }
            let model = quiz_question::ActiveModel {
                lesson_id: Set(lesson.id),
                kind: Set(kind.into()),
                prompt: Set(prompt.into()),
                explain: Set(false),
                ..Default::default()
            };
            Ok(Some(model.insert(db).await?.id))
        };
        let option = |question: i64, blank: Option<i64>, position: i32, (content, correct): (&str, bool)| {
            quiz_option::ActiveModel {
                quiz_question_id: Set(question),
                blank_id: Set(blank),
                content: Set(content.into()),
                correct: Set(correct),
                position: Set(position),
                ..Default::default()
            }
        };
        if let Some(id) = question(quiz_question::SINGLE, CHOICE.0).await? {
            for (position, item) in (1..).zip(CHOICE.1) {
                option(id, None, position, item).insert(db).await?;
            }
        }
        if let Some(id) = question(quiz_question::FILL_BLANK, BLANKS.0).await? {
            // One position per question, whatever the blank (the table's unique key).
            let mut next = 0;
            for (position, (label, choices)) in (1..).zip(BLANKS.1) {
                let blank = quiz_blank::ActiveModel {
                    quiz_question_id: Set(id),
                    label: Set(label.into()),
                    position: Set(position),
                    ..Default::default()
                };
                let blank = blank.insert(db).await?.id;
                for item in choices {
                    next += 1;
                    option(id, Some(blank), next, item).insert(db).await?;
                }
            }
        }
        Ok(())
    }
}
