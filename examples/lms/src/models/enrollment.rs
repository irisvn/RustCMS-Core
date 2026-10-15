//! `enrollments` table: a learner in a course, once (withdrawing and coming
//! back change `status`, never add a row).

use sea_orm::entity::prelude::*;

/// Taking a seat.
pub const ACTIVE: &str = "active";
/// Left the course; the seat is free.
pub const WITHDRAWN: &str = "withdrawn";

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "enrollments")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub course_id: i64,
    pub learner_id: i64,
    /// [`ACTIVE`] or [`WITHDRAWN`].
    pub status: String,
    /// Percent of the course done (0 to 100).
    pub progress: i32,
    pub enrolled_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
