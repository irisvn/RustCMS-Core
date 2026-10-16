//! `news`: articles of the school (published once `published_at` has passed).

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "news")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub title: String,
    #[sea_orm(unique)]
    pub slug: String,
    pub body: Option<Json>,
    pub cover: Option<Uuid>,
    pub category_id: Option<i64>,
    pub author_id: i64,
    pub published_at: Option<DateTimeUtc>,
    pub created_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
