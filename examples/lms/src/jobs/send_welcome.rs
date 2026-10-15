//! The `lms.enrolled` job: welcome a learner who took a seat (logged for now;
//! mail comes with M19).

use rustcms::async_trait;
use rustcms::queue::{Job, JobCtx};
use sea_orm::EntityTrait;
use serde::{Deserialize, Serialize};

use crate::models::{course, learner};
use crate::settings::lms_settings::LmsSettings;

/// Ids only: the job reads the rows again when it runs.
#[derive(Debug, Serialize, Deserialize)]
pub struct SendWelcome {
    pub course_id: i64,
    pub learner_id: i64,
}

#[async_trait]
impl Job for SendWelcome {
    const KIND: &'static str = "lms.enrolled";

    async fn handle(self, ctx: JobCtx) -> anyhow::Result<()> {
        let db = ctx.state.db();
        let (Some(course), Some(learner)) = (
            course::Entity::find_by_id(self.course_id).one(db).await?,
            learner::Entity::find_by_id(self.learner_id).one(db).await?,
        ) else {
            // Removed since: nobody to welcome.
            return Ok(());
        };
        let school = rustcms::settings::get::<LmsSettings>(&ctx.state).await?.school_name;
        tracing::info!(job = %ctx.id, course = course.id, learner = learner.id, "{school}: welcome to {}", course.title);
        Ok(())
    }
}
