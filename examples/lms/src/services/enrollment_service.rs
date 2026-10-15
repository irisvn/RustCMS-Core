//! Who may sit in a course: one enrollment per learner and course, and no
//! more active ones than the course's capacity.
//!
//! The checks read the course's enrollments, so they run under the course's
//! row lock: the admin panel takes it before any enrollment write
//! (`ResourceMeta::lock_parent`), as does an action on the course (it locks
//! the courses it runs on); [`enroll`] takes it itself. Two writers can then
//! never both take the last seat.

use rustcms::{AppResult, ValidationErrors};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, DatabaseTransaction, DbBackend, EntityTrait,
    PaginatorTrait, QueryFilter, QuerySelect, Set,
};

use crate::jobs::send_welcome::SendWelcome;
use crate::models::{course, enrollment};

/// Why an enrollment cannot be written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    NoCourse,
    AlreadyEnrolled,
    NoSeat,
    UnknownStatus,
}

impl Refusal {
    /// The field it concerns and its message (English source, translated by
    /// the caller: `ctx.t` in the admin panel).
    pub fn reason(self) -> (&'static str, &'static str) {
        match self {
            Self::NoCourse => ("course_id", "is not a course"),
            Self::AlreadyEnrolled => ("learner_id", "is already enrolled in this course"),
            Self::NoSeat => ("course_id", "has no seats left"),
            Self::UnknownStatus => ("status", "must be active or withdrawn"),
        }
    }

    /// As validation errors, messages passed through `translate`.
    pub fn errors(self, translate: impl Fn(&str) -> String) -> ValidationErrors {
        let (field, message) = self.reason();
        ValidationErrors::field(field, translate(message))
    }
}

/// Locks the course's row until `txn` ends (SQLite: the write transaction
/// already holds the database lock).
pub async fn lock_course(txn: &DatabaseTransaction, course_id: i64) -> AppResult<Option<course::Model>> {
    let select = course::Entity::find_by_id(course_id);
    let select = if txn.get_database_backend() == DbBackend::Sqlite { select } else { select.lock_exclusive() };
    Ok(select.one(txn).await?)
}

/// Whether an enrollment of `learner_id` in `course_id` with `status` may be
/// written; `current` is the enrollment being changed, if any. The caller
/// holds the course's lock.
pub async fn check(
    txn: &DatabaseTransaction,
    course_id: i64,
    learner_id: i64,
    status: &str,
    current: Option<i64>,
) -> AppResult<Result<(), Refusal>> {
    if status != enrollment::ACTIVE && status != enrollment::WITHDRAWN {
        return Ok(Err(Refusal::UnknownStatus));
    }
    let Some(course) = course::Entity::find_by_id(course_id).one(txn).await? else {
        return Ok(Err(Refusal::NoCourse));
    };
    let mut others = enrollment::Entity::find().filter(enrollment::Column::CourseId.eq(course_id));
    if let Some(id) = current {
        others = others.filter(enrollment::Column::Id.ne(id));
    }
    // Already in: said before the seats, which would not help.
    if others.clone().filter(enrollment::Column::LearnerId.eq(learner_id)).count(txn).await? > 0 {
        return Ok(Err(Refusal::AlreadyEnrolled));
    }
    if status == enrollment::ACTIVE {
        let taken = others.filter(enrollment::Column::Status.eq(enrollment::ACTIVE)).count(txn).await?;
        if taken >= u64::try_from(course.capacity).unwrap_or(0) {
            return Ok(Err(Refusal::NoSeat));
        }
    }
    Ok(Ok(()))
}

/// Active enrollments of a course (under its lock, an exact count).
pub async fn active_count(txn: &impl ConnectionTrait, course_id: i64) -> AppResult<u64> {
    Ok(enrollment::Entity::find()
        .filter(enrollment::Column::CourseId.eq(course_id))
        .filter(enrollment::Column::Status.eq(enrollment::ACTIVE))
        .count(txn)
        .await?)
}

/// Enrolls `learner_id` in `course_id` (active) in a transaction of its own,
/// queuing the welcome with it. For code outside the admin panel (seeders,
/// jobs, an API).
pub async fn enroll(
    db: &DatabaseConnection,
    course_id: i64,
    learner_id: i64,
) -> AppResult<Result<enrollment::Model, Refusal>> {
    let txn = rustcms::db::begin_write(db).await?;
    lock_course(&txn, course_id).await?;
    match enroll_in(&txn, course_id, learner_id).await? {
        Ok(created) => {
            txn.commit().await?;
            Ok(Ok(created))
        }
        Err(refusal) => {
            txn.rollback().await?;
            Ok(Err(refusal))
        }
    }
}

/// Enrolls `learner_id` in `course_id` (active) in `txn`, whose caller holds
/// the course's lock (an action on the course, or [`enroll`]), queuing the
/// welcome with it.
pub async fn enroll_in(
    txn: &DatabaseTransaction,
    course_id: i64,
    learner_id: i64,
) -> AppResult<Result<enrollment::Model, Refusal>> {
    if let Err(refusal) = check(txn, course_id, learner_id, enrollment::ACTIVE, None).await? {
        return Ok(Err(refusal));
    }
    let created = enrollment::ActiveModel {
        course_id: Set(course_id),
        learner_id: Set(learner_id),
        status: Set(enrollment::ACTIVE.into()),
        progress: Set(0),
        enrolled_at: Set(chrono::Utc::now()),
        ..Default::default()
    }
    .insert(txn)
    .await?;
    rustcms::queue::dispatch(txn, &SendWelcome { course_id, learner_id }).await?;
    Ok(Ok(created))
}
