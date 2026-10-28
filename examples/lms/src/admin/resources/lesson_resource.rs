//! Lessons of a course, in order.

use rustcms::AppResult;
use rustcms::admin::*;
use rustcms::audit::AuditEntry;
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseTransaction, EntityTrait, QueryFilter, QueryOrder, Select, Set};
use serde_json::json;

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

    /// "Move up" / "Move down": swaps a lesson with its neighbour in the course.
    fn actions() -> Vec<Action<lesson::Entity>> {
        vec![moving("move_up", true).label("Move up"), moving("move_down", false).label("Move down")]
    }

    // #region scope
    /// Instructors see the lessons of their courses.
    fn scope(query: Select<lesson::Entity>, ctx: &Ctx) -> Select<lesson::Entity> {
        match instructor(ctx) {
            None => query,
            Some(me) => query.filter(course::taught_by(me, (lesson::Entity, lesson::Column::CourseId))),
        }
    }
    // #endregion scope
}

fn moving(name: &'static str, up: bool) -> Action<lesson::Entity> {
    Action::<lesson::Entity>::new(name, move |ctx, txn, lessons, _| {
        Box::pin(async move {
            for lesson in lessons {
                reorder(ctx, txn, &lesson, up).await?;
            }
            Ok(())
        })
    })
    .ability("update")
}

// #region reorder
/// Moves `lesson` one place among the lessons of its course the user may see
/// (the course is locked by the action: moves of one course never interleave).
/// Positions are renumbered 1…n first (ties in id order), so the order is
/// always total; every lesson whose position changes is recorded.
async fn reorder(ctx: &Ctx, txn: &DatabaseTransaction, lesson: &lesson::Model, up: bool) -> AppResult<()> {
    let siblings = LessonResource::scope(lesson::Entity::find(), ctx)
        .filter(lesson::Column::CourseId.eq(lesson.course_id))
        .order_by_asc(lesson::Column::Position)
        .order_by_asc(lesson::Column::Id)
        .all(txn)
        .await?;
    let mut order: Vec<&lesson::Model> = siblings.iter().collect();
    let Some(index) = order.iter().position(|l| l.id == lesson.id) else { return Ok(()) };
    let neighbour = if up { index.checked_sub(1) } else { Some(index + 1).filter(|n| *n < order.len()) };
    let Some(neighbour) = neighbour else { return Ok(()) };
    order.swap(index, neighbour);
    for (n, row) in order.into_iter().enumerate() {
        let position = i32::try_from(n + 1).unwrap_or(i32::MAX);
        if row.position == position {
            continue;
        }
        let mut model: lesson::ActiveModel = row.clone().into();
        model.position = Set(position);
        model.update(txn).await?;
        let entry = AuditEntry::new("updated", LessonResource::SLUG)
            .key(row.id)
            .changes(json!({ "position": [row.position, position] }));
        rustcms::audit::record(txn, &ctx.audit_context, entry).await?;
    }
    Ok(())
}
// #endregion reorder
