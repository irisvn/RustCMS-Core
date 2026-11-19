//! `quiz_blanks`: a blank of a fill-in-the-blank question.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "quiz_blanks")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub quiz_question_id: i64,
    pub label: String,
    pub position: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
