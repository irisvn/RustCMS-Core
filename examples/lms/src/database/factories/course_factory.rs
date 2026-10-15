//! Courses for tests: `CourseFactory::new().capacity(1).create(db)`.

use rustcms::database::Factory;
use sea_orm::ActiveValue::Set;

use crate::models::course;

#[derive(Default)]
pub struct CourseFactory {
    capacity: Option<i32>,
    instructor: Option<i64>,
}

impl CourseFactory {
    pub fn new() -> Self {
        Self::default()
    }

    /// Active enrollments allowed (default 30).
    pub fn capacity(mut self, capacity: i32) -> Self {
        self.capacity = Some(capacity);
        self
    }

    /// Taught by the user `id`.
    pub fn taught_by(mut self, id: i64) -> Self {
        self.instructor = Some(id);
        self
    }
}

impl Factory for CourseFactory {
    type Entity = course::Entity;

    fn definition(&self, n: u64) -> course::ActiveModel {
        course::ActiveModel {
            title: Set(format!("Course {n}")),
            slug: Set(format!("course-{n}")),
            summary: Set(None),
            capacity: Set(self.capacity.unwrap_or(30)),
            published: Set(true),
            instructor_id: Set(self.instructor),
            created_at: Set(chrono::Utc::now()),
            ..Default::default()
        }
    }
}
