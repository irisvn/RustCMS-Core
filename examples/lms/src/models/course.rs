//! `courses` table.

use sea_orm::entity::prelude::*;
use sea_orm::sea_query::{Expr, ExprTrait, IntoColumnRef, Query, SimpleExpr};

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "courses")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub title: String,
    pub slug: String,
    pub summary: Option<String>,
    /// Active enrollments allowed at once.
    pub capacity: i32,
    pub published: bool,
    /// The user who teaches it (a `users` row).
    pub instructor_id: Option<i64>,
    pub created_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

// #region taught-by
/// Scope: the course whose id is `course_id` (a column of the outer query)
/// is taught by `instructor`. `EXISTS` keeps one row per outer row, where a
/// join could repeat them.
pub fn taught_by(instructor: i64, course_id: impl IntoColumnRef) -> SimpleExpr {
    Expr::exists(
        Query::select()
            .expr(Expr::val(1))
            .from(Entity)
            .and_where(Expr::col((Entity, Column::Id)).equals(course_id))
            .and_where(Expr::col((Entity, Column::InstructorId)).eq(instructor))
            .to_owned(),
    )
}
// #endregion taught-by
