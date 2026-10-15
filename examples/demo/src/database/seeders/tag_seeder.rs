//! A few tags, found by name.

use rustcms::database::Seeder;
use rustcms::{AppState, async_trait};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};

use crate::models::tag;

pub const TAGS: [&str; 4] = ["Rust", "Databases", "Design", "Security"];

pub struct TagSeeder;

#[async_trait]
impl Seeder for TagSeeder {
    fn name(&self) -> &'static str {
        "tag"
    }

    async fn run(&self, state: &AppState) -> anyhow::Result<()> {
        let db = state.db();
        for name in TAGS {
            if tag::Entity::find().filter(tag::Column::Name.eq(name)).one(db).await?.is_none() {
                tag::ActiveModel { name: Set(name.into()), ..Default::default() }.insert(db).await?;
            }
        }
        Ok(())
    }
}
