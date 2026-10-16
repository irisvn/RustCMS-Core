//! A few categories, tags and news articles (one a draft, one scheduled).

use rustcms::database::Seeder;
use rustcms::{AppState, async_trait};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};

use super::user_seeder;
use crate::admin::resources::news_resource::slugify;
use crate::models::{category, news, news_tag, tag};

/// `(title, category, tags, days from now: published; None: a draft)`.
const NEWS: [(&str, &str, &[&str], Option<i64>); 3] = [
    ("Enrollments are open for the autumn term", "School", &["enrollment"], Some(-3)),
    ("New course: Web services with Axum", "Courses", &["rust", "web"], Some(2)),
    ("Notes from the Rust meetup", "Events", &["rust"], None),
];

pub struct NewsSeeder;

#[async_trait]
impl Seeder for NewsSeeder {
    fn name(&self) -> &'static str {
        "news"
    }

    async fn run(&self, state: &AppState) -> anyhow::Result<()> {
        let db = state.db();
        let author = user_seeder::find(state, "editor@example.test").await?;
        for (title, category_name, tags, days) in NEWS {
            let category =
                match category::Entity::find().filter(category::Column::Name.eq(category_name)).one(db).await? {
                    Some(found) => found,
                    None => {
                        category::ActiveModel { name: Set(category_name.into()), ..Default::default() }
                            .insert(db)
                            .await?
                    }
                };
            let slug = slugify(title);
            if news::Entity::find().filter(news::Column::Slug.eq(&slug)).one(db).await?.is_some() {
                continue;
            }
            // The article and its tags together: an interrupted run leaves none or all.
            let txn = rustcms::db::begin_write(db).await?;
            let article = news::ActiveModel {
                title: Set(title.into()),
                slug: Set(slug),
                category_id: Set(Some(category.id)),
                author_id: Set(author.id),
                published_at: Set(days.map(|d| chrono::Utc::now() + chrono::Duration::days(d))),
                created_at: Set(chrono::Utc::now()),
                ..Default::default()
            }
            .insert(&txn)
            .await?;
            for name in tags {
                let tag = match tag::Entity::find().filter(tag::Column::Name.eq(*name)).one(&txn).await? {
                    Some(found) => found,
                    None => tag::ActiveModel { name: Set((*name).into()), ..Default::default() }.insert(&txn).await?,
                };
                news_tag::ActiveModel { news_id: Set(article.id), tag_id: Set(tag.id) }.insert(&txn).await?;
            }
            txn.commit().await?;
        }
        Ok(())
    }
}
