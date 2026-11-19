//! `category_sections` table: a section of a category's landing page.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "category_sections")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub category_id: i32,
    pub title: String,
    /// `compact` (labels only) or `detailed` (each link with a note).
    #[sea_orm(default_value = "compact")]
    pub style: String,
    pub position: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
