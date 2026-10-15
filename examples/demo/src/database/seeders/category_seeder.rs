//! Three categories, found by slug.

use rustcms::database::Seeder;
use rustcms::{AppState, async_trait};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};

use crate::models::category;

pub const CATEGORIES: [(&str, &str); 3] = [("News", "news"), ("Guides", "guides"), ("Releases", "releases")];

pub struct CategorySeeder;

#[async_trait]
impl Seeder for CategorySeeder {
    fn name(&self) -> &'static str {
        "category"
    }

    async fn run(&self, state: &AppState) -> anyhow::Result<()> {
        let db = state.db();
        for (name, slug) in CATEGORIES {
            if category::Entity::find().filter(category::Column::Slug.eq(slug)).one(db).await?.is_none() {
                category::ActiveModel { name: Set(name.into()), slug: Set(slug.into()), ..Default::default() }
                    .insert(db)
                    .await?;
            }
        }
        Ok(())
    }
}
