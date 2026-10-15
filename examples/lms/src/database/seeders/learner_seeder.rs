//! Ten learners, found by email.

use rustcms::database::Seeder;
use rustcms::{AppState, async_trait};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};

use crate::models::learner;

pub const LEARNERS: usize = 10;

/// Email of seeded learner `i` (1-based).
pub fn email(i: usize) -> String {
    format!("learner{i}@example.test")
}

pub struct LearnerSeeder;

#[async_trait]
impl Seeder for LearnerSeeder {
    fn name(&self) -> &'static str {
        "learner"
    }

    async fn run(&self, state: &AppState) -> anyhow::Result<()> {
        let db = state.db();
        for i in 1..=LEARNERS {
            if learner::Entity::find().filter(learner::Column::Email.eq(email(i))).one(db).await?.is_none() {
                learner::ActiveModel {
                    name: Set(format!("Learner {i}")),
                    email: Set(email(i)),
                    phone: Set(None),
                    created_at: Set(chrono::Utc::now()),
                    ..Default::default()
                }
                .insert(db)
                .await?;
            }
        }
        Ok(())
    }
}
