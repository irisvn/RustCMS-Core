//! Each learner in one course (round robin), through the enrollment service:
//! the seat and duplicate rules hold here too, and each enrollment queues
//! its welcome.

use rustcms::database::Seeder;
use rustcms::{AppState, async_trait};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

use super::course_seeder::COURSES;
use super::learner_seeder::{LEARNERS, email};
use crate::models::{course, learner};
use crate::services::enrollment_service::{self, Refusal};

pub struct EnrollmentSeeder;

#[async_trait]
impl Seeder for EnrollmentSeeder {
    fn name(&self) -> &'static str {
        "enrollment"
    }

    async fn run(&self, state: &AppState) -> anyhow::Result<()> {
        let db = state.db();
        for i in 1..=LEARNERS {
            let (_, slug, _) = COURSES[i % COURSES.len()];
            let course = course::Entity::find().filter(course::Column::Slug.eq(slug)).one(db).await?;
            let learner = learner::Entity::find().filter(learner::Column::Email.eq(email(i))).one(db).await?;
            let (Some(course), Some(learner)) = (course, learner) else {
                anyhow::bail!("run the course and learner seeders first");
            };
            match enrollment_service::enroll(db, course.id, learner.id).await? {
                // Seeded before: nothing to do.
                Ok(_) | Err(Refusal::AlreadyEnrolled) => {}
                Err(Refusal::NoSeat) => println!("`{slug}` is full: learner {i} not enrolled"),
                Err(other) => anyhow::bail!("cannot enroll learner {i} in `{slug}`: {:?}", other.reason()),
            }
        }
        Ok(())
    }
}
