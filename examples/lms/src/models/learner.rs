//! `learners` table: people who take courses. Not users: a learner has no
//! sign-in and no roles.

use sea_orm::entity::prelude::*;
use sea_orm::sea_query::{Expr, ExprTrait, IntoColumnRef, Query, SimpleExpr};

use super::{course, enrollment};

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "learners")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub name: String,
    pub email: String,
    pub phone: Option<String>,
    pub created_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

/// Scope: the learner whose id is `learner_id` (a column of the outer query)
/// is enrolled, in any status, in a course `instructor` teaches.
pub fn taught_by(instructor: i64, learner_id: impl IntoColumnRef) -> SimpleExpr {
    Expr::exists(
        Query::select()
            .expr(Expr::val(1))
            .from(enrollment::Entity)
            .and_where(Expr::col((enrollment::Entity, enrollment::Column::LearnerId)).equals(learner_id))
            .and_where(course::taught_by(instructor, (enrollment::Entity, enrollment::Column::CourseId)))
            .to_owned(),
    )
}
