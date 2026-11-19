//! `quiz_options`: an option of a choice question (`blank_id` null), or a
//! choice of one of its blanks.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "quiz_options")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub quiz_question_id: i64,
    pub blank_id: Option<i64>,
    pub content: String,
    #[sea_orm(default_value = false)]
    pub correct: bool,
    pub hint: Option<String>,
    pub position: i32,
    pub image: Option<Uuid>,
    /// Why the option is right or wrong (rich text).
    pub explanation: Option<Json>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
