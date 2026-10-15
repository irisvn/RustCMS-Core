//! Courses: an instructor, a capacity, and their lessons and enrollments.

use rustcms::admin::*;
use rustcms::{AppResult, ValidationErrors};
use sea_orm::{ColumnTrait, DatabaseTransaction, EntityTrait, QueryFilter, Select};

use crate::admin::int;
use crate::models::course;
use crate::policies::instructor;
use crate::policies::permissions::ASSIGN_INSTRUCTOR;
use crate::services::enrollment_service;
use crate::settings::lms_settings::MAX_CAPACITY;

pub struct CourseResource;

#[async_trait::async_trait]
impl Resource for CourseResource {
    type Entity = course::Entity;
    const SLUG: &'static str = "courses";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Course").title("title").icon("book-open").group("Learning").unique(
            "courses_slug_key",
            &["slug"],
            "is already taken",
        )
    }

    fn fields() -> Vec<Field> {
        vec![
            Field::column(course::Column::Title).required().searchable().sortable().max_length(200),
            Field::column(course::Column::Slug).required().searchable().max_length(120),
            Field::column(course::Column::Summary).max_length(2000),
            Field::column(course::Column::Capacity).required().sortable(),
            Field::column(course::Column::Published).filterable().sortable(),
            // Who teaches it decides who sees it: only staff choose.
            Field::column(course::Column::InstructorId)
                .label("Instructor")
                .filterable()
                .creatable(|ctx| ctx.can(ASSIGN_INSTRUCTOR))
                .updatable(|ctx| ctx.can(ASSIGN_INSTRUCTOR)),
        ]
    }

    fn form() -> Form {
        Form::new().columns(2).schema(components![
            TextInput::make("title"),
            TextInput::make("slug").help("Lowercase words joined by dashes, e.g. intro-to-rust"),
            NumberInput::make("capacity").help("Active enrollments allowed at once"),
            NumberInput::make("instructor_id").help("Id of the user who teaches the course"),
            Toggle::make("published"),
            Textarea::make("summary").column_span(2),
        ])
    }

    fn table() -> Table {
        Table::new()
            .columns(vec![
                TextColumn::make("title"),
                TextColumn::make("capacity"),
                BooleanColumn::make("published"),
                TextColumn::make("instructor_id"),
            ])
            .filters(vec![TernaryFilter::make("published")])
            .default_sort("title")
    }

    fn relations() -> Vec<Relation> {
        vec![
            Relation::has_many("lessons", "lessons", "course_id"),
            Relation::has_many("enrollments", "enrollments", "course_id"),
        ]
    }

    /// Instructors see the courses they teach.
    fn scope(query: Select<course::Entity>, ctx: &Ctx) -> Select<course::Entity> {
        match instructor(ctx) {
            None => query,
            Some(me) => query.filter(course::Column::InstructorId.eq(me)),
        }
    }

    async fn mutate_before_create(ctx: &Ctx, txn: &DatabaseTransaction, data: Data) -> AppResult<Data> {
        check(ctx, txn, &data, None).await?;
        Ok(data)
    }

    /// The engine holds the course's row lock here, as enrollment writes do
    /// (`lock_parent`): no seat is taken while the capacity changes.
    async fn mutate_before_update(
        ctx: &Ctx,
        txn: &DatabaseTransaction,
        record: &course::Model,
        data: Data,
    ) -> AppResult<Data> {
        check(ctx, txn, &data, Some(record)).await?;
        Ok(data)
    }
}

async fn check(ctx: &Ctx, txn: &DatabaseTransaction, data: &Data, record: Option<&course::Model>) -> AppResult<()> {
    let mut errors = ValidationErrors::new();
    if let Some(slug) = data.get("slug").and_then(|v| v.as_str())
        && !slug.split('-').all(|w| !w.is_empty() && w.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()))
    {
        errors.add("slug", ctx.t("must be lowercase words joined by dashes"));
    }
    if data.contains_key("capacity") {
        match int(data, "capacity") {
            Some(capacity) if (1..=MAX_CAPACITY).contains(&capacity) => {
                if let Some(record) = record
                    && (capacity as u64) < enrollment_service::active_count(txn, record.id).await?
                {
                    errors.add("capacity", ctx.t("is below the number of active enrollments"));
                }
            }
            _ => errors.add("capacity", ctx.t("must be between 1 and 10000")),
        }
    }
    if data.get("instructor_id").is_some_and(|v| !v.is_null()) {
        let user = match int(data, "instructor_id") {
            Some(id) => rustcms::models::user::Entity::find_by_id(id).one(txn).await?,
            None => None,
        };
        if user.is_none() {
            errors.add("instructor_id", ctx.t("is not a user"));
        }
    }
    if errors.is_empty() { Ok(()) } else { Err(errors.into()) }
}
