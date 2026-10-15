//! Lessons of a course, in order.

use rustcms::admin::*;
use sea_orm::{QueryFilter, Select};

use crate::models::{course, lesson};
use crate::policies::instructor;

pub struct LessonResource;

impl Resource for LessonResource {
    type Entity = lesson::Entity;
    const SLUG: &'static str = "lessons";

    /// Writes lock the course first: one that changes instructor meanwhile
    /// waits, and the instructor's access is checked again under the lock.
    fn meta() -> ResourceMeta {
        ResourceMeta::new("Lesson").title("title").icon("list").group("Learning").lock_parent("course_id", "courses")
    }

    fn fields() -> Vec<Field> {
        vec![
            // Options and writes go through the courses' scope: instructors
            // pick only courses they teach.
            Field::column(lesson::Column::CourseId).belongs_to("courses").required().filterable(),
            Field::column(lesson::Column::Title).required().searchable().sortable().max_length(200),
            Field::column(lesson::Column::Position).sortable(),
            Field::column(lesson::Column::Body),
        ]
    }

    fn table() -> Table {
        Table::new()
            .columns(vec![TextColumn::make("position"), TextColumn::make("title"), TextColumn::make("course_id")])
            .filters(vec![SelectFilter::make("course_id")])
            .default_sort("position")
    }

    /// Instructors see the lessons of their courses.
    fn scope(query: Select<lesson::Entity>, ctx: &Ctx) -> Select<lesson::Entity> {
        match instructor(ctx) {
            None => query,
            Some(me) => query.filter(course::taught_by(me, (lesson::Entity, lesson::Column::CourseId))),
        }
    }
}
