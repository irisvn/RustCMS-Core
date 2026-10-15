//! Enrollments: a learner in a course, within the course's capacity. The
//! rules live in `crate::services::enrollment_service`; this declares the
//! panel's view of them.

use std::sync::Arc;

use rustcms::admin::*;
use rustcms::{AppResult, ValidationErrors};
use sea_orm::{DatabaseTransaction, QueryFilter, Select};

use crate::admin::int;
use crate::jobs::send_welcome::SendWelcome;
use crate::models::{course, enrollment};
use crate::policies::enrollment_policy::EnrollmentPolicy;
use crate::policies::instructor;
use crate::policies::permissions::UPDATE_PROGRESS;
use crate::services::enrollment_service;

pub struct EnrollmentResource;

const STATUSES: [(&str, &str); 2] = [(enrollment::ACTIVE, "Active"), (enrollment::WITHDRAWN, "Withdrawn")];

#[async_trait::async_trait]
impl Resource for EnrollmentResource {
    type Entity = enrollment::Entity;
    const SLUG: &'static str = "enrollments";

    /// Every write first locks the course (both, when moving): the seat count
    /// and the instructor's access hold until it commits.
    fn meta() -> ResourceMeta {
        ResourceMeta::new("Enrollment")
            .icon("clipboard-check")
            .group("Learning")
            .lock_parent("course_id", "courses")
            .unique(
                "enrollments_course_learner_key",
                &["course_id", "learner_id"],
                "This learner is already enrolled in this course.",
            )
    }

    fn fields() -> Vec<Field> {
        let manage = |ctx: &Ctx| ctx.can("enrollments.update");
        vec![
            Field::column(enrollment::Column::CourseId).belongs_to("courses").required().filterable().updatable(manage),
            Field::column(enrollment::Column::LearnerId)
                .belongs_to("learners")
                .required()
                .filterable()
                .updatable(manage),
            Field::column(enrollment::Column::Status).filterable().max_length(16).updatable(manage),
            // Recorded by whoever teaches; never set when enrolling.
            Field::column(enrollment::Column::Progress)
                .sortable()
                .creatable(|_| false)
                .updatable(|ctx| ctx.can(UPDATE_PROGRESS)),
            Field::column(enrollment::Column::EnrolledAt).guarded().sortable(),
        ]
    }

    fn form() -> Form {
        Form::new().schema(components![
            SelectInput::make("course_id"),
            SelectInput::make("learner_id"),
            SelectInput::make("status").options(STATUSES),
            NumberInput::make("progress").help("Percent of the course done (0 to 100)"),
        ])
    }

    fn table() -> Table {
        Table::new()
            .columns(vec![
                TextColumn::make("course_id"),
                TextColumn::make("learner_id"),
                TextColumn::make("status"),
                TextColumn::make("progress").cell("lms/progress"),
                DateColumn::make("enrolled_at"),
            ])
            .filters(vec![SelectFilter::make("course_id"), SelectFilter::make("status").options(STATUSES)])
            .default_sort("-enrolled_at")
    }

    fn policy() -> Arc<dyn Policy> {
        Arc::new(EnrollmentPolicy)
    }

    /// Instructors see the enrollments of their courses.
    fn scope(query: Select<enrollment::Entity>, ctx: &Ctx) -> Select<enrollment::Entity> {
        match instructor(ctx) {
            None => query,
            Some(me) => query.filter(course::taught_by(me, (enrollment::Entity, enrollment::Column::CourseId))),
        }
    }

    async fn mutate_before_create(ctx: &Ctx, txn: &DatabaseTransaction, data: Data) -> AppResult<Data> {
        let (Some(course_id), Some(learner_id)) = (int(&data, "course_id"), int(&data, "learner_id")) else {
            // The engine reports the missing fields.
            return Ok(data);
        };
        let status = data.get("status").and_then(|v| v.as_str()).unwrap_or(enrollment::ACTIVE).to_owned();
        enrollment_service::check(txn, course_id, learner_id, &status, None)
            .await?
            .map_err(|refusal| refusal.errors(|m| ctx.t(m)))?;
        if status == enrollment::ACTIVE {
            // Queued with the enrollment: no enrollment, no job.
            ctx.dispatch(txn, &SendWelcome { course_id, learner_id }).await?;
        }
        Ok(data)
    }

    async fn mutate_before_update(
        ctx: &Ctx,
        txn: &DatabaseTransaction,
        record: &enrollment::Model,
        data: Data,
    ) -> AppResult<Data> {
        if data.contains_key("progress") && !matches!(int(&data, "progress"), Some(0..=100)) {
            return Err(ValidationErrors::field("progress", ctx.t("must be between 0 and 100")).into());
        }
        let course_id = int(&data, "course_id").unwrap_or(record.course_id);
        let learner_id = int(&data, "learner_id").unwrap_or(record.learner_id);
        let status = data.get("status").and_then(|v| v.as_str()).unwrap_or(&record.status).to_owned();
        // Moving, or coming back: checked again (the engine locked both courses).
        if course_id != record.course_id || learner_id != record.learner_id || status != record.status {
            enrollment_service::check(txn, course_id, learner_id, &status, Some(record.id))
                .await?
                .map_err(|refusal| refusal.errors(|m| ctx.t(m)))?;
        }
        Ok(data)
    }
}
