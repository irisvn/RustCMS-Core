//! Three courses, each with three lessons, at the `lms` settings' default
//! capacity.

use rustcms::database::Seeder;
use rustcms::{AppState, async_trait};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};

use super::user_seeder;
use crate::models::{course, lesson};
use crate::settings::lms_settings::LmsSettings;

/// `(title, slug, instructor's email)`.
pub const COURSES: [(&str, &str, &str); 3] = [
    ("Introduction to Rust", "intro-to-rust", "teacher@example.test"),
    ("Web services with Axum", "web-services-with-axum", "teacher@example.test"),
    ("Databases with SeaORM", "databases-with-seaorm", "tutor@example.test"),
];

pub struct CourseSeeder;

#[async_trait]
impl Seeder for CourseSeeder {
    fn name(&self) -> &'static str {
        "course"
    }

    async fn run(&self, state: &AppState) -> anyhow::Result<()> {
        let db = state.db();
        let capacity = rustcms::settings::get::<LmsSettings>(state).await?.default_capacity;
        for (title, slug, teacher) in COURSES {
            let teacher = user_seeder::find(state, teacher).await?;
            let course = match course::Entity::find().filter(course::Column::Slug.eq(slug)).one(db).await? {
                Some(found) => found,
                None => {
                    course::ActiveModel {
                        title: Set(title.into()),
                        slug: Set(slug.into()),
                        summary: Set(Some(format!("{title}, from the first steps."))),
                        capacity: Set(capacity),
                        published: Set(true),
                        instructor_id: Set(Some(teacher.id)),
                        created_at: Set(chrono::Utc::now()),
                        ..Default::default()
                    }
                    .insert(db)
                    .await?
                }
            };
            for position in 1..=3 {
                let title = format!("Lesson {position}");
                let exists = lesson::Entity::find()
                    .filter(lesson::Column::CourseId.eq(course.id))
                    .filter(lesson::Column::Title.eq(&title))
                    .one(db)
                    .await?
                    .is_some();
                if !exists {
                    lesson::ActiveModel {
                        course_id: Set(course.id),
                        title: Set(title),
                        position: Set(position),
                        body: Set(Some(format!("Part {position} of {}.", course.title))),
                        ..Default::default()
                    }
                    .insert(db)
                    .await?;
                }
            }
        }
        Ok(())
    }
}
