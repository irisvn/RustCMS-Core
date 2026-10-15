//! Thirty posts by the demo accounts, found by title (after the user and
//! category seeders).

use rustcms::database::Seeder;
use rustcms::{AppState, async_trait};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};

use super::category_seeder::CATEGORIES;
use super::user_seeder::account;
use crate::models::{category, post};

pub struct PostSeeder;

#[async_trait]
impl Seeder for PostSeeder {
    fn name(&self) -> &'static str {
        "post"
    }

    async fn run(&self, state: &AppState) -> anyhow::Result<()> {
        let db = state.db();
        let admin = account(state, "Ada Admin", "admin@example.test").await?;
        let editor = account(state, "Eddie Editor", "editor@example.test").await?;
        let mut categories = Vec::new();
        for (_, slug) in CATEGORIES {
            let found = category::Entity::find().filter(category::Column::Slug.eq(slug)).one(db).await?;
            let found = found.ok_or_else(|| anyhow::anyhow!("category `{slug}` missing: run the category seeder"))?;
            categories.push(found.id);
        }
        let now = chrono::Utc::now();
        for i in 1..=30 {
            let title = format!("Post number {i}");
            if post::Entity::find().filter(post::Column::Title.eq(&title)).one(db).await?.is_some() {
                continue;
            }
            post::ActiveModel {
                title: Set(title),
                body: Set(Some(format!("Body of post {i}."))),
                category_id: Set(Some(categories[i % categories.len()])),
                author_id: Set(Some(if i % 3 == 0 { editor.id } else { admin.id })),
                published: Set(i % 2 == 0),
                published_at: Set((i % 2 == 0).then(|| now - chrono::Duration::days(i as i64))),
                attributes: Set(Some(serde_json::json!({
                    "reading_time": format!("{} min", 2 + i % 7),
                    "audience": if i % 2 == 0 { "everyone" } else { "developers" },
                }))),
                created_at: Set(now),
                updated_at: Set(now),
                ..Default::default()
            }
            .insert(db)
            .await?;
        }
        Ok(())
    }
}
