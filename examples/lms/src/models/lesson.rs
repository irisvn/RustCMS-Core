//! `lessons` table.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "lessons")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub course_id: i64,
    pub title: String,
    pub position: i32,
    pub body: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
