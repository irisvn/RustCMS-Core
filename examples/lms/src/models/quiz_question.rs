//! `quiz_questions`: a question of a lesson's quiz.

use sea_orm::entity::prelude::*;

/// Kinds of question: one correct option, several, or blanks to fill in.
pub const SINGLE: &str = "single";
pub const MULTIPLE: &str = "multiple";
pub const FILL_BLANK: &str = "fill_blank";

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "quiz_questions")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub lesson_id: i64,
    #[sea_orm(default_value = "single")]
    pub kind: String,
    pub prompt: String,
    /// Whether correct options carry a hint shown after answering.
    #[sea_orm(default_value = false)]
    pub explain: bool,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
