//! `section_links` table: a link of a category section.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "section_links")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub section_id: i32,
    pub label: String,
    pub url: String,
    pub note: Option<String>,
    pub position: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
