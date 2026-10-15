//! Learners with unique emails (`learner<n>@factory.test`).

use rustcms::database::Factory;
use sea_orm::ActiveValue::Set;

use crate::models::learner;

pub struct LearnerFactory;

impl Factory for LearnerFactory {
    type Entity = learner::Entity;

    fn definition(&self, n: u64) -> learner::ActiveModel {
        learner::ActiveModel {
            name: Set(format!("Learner {n}")),
            email: Set(format!("learner{n}@factory.test")),
            phone: Set(None),
            created_at: Set(chrono::Utc::now()),
            ..Default::default()
        }
    }
}
