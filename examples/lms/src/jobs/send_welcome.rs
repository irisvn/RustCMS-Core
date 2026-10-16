//! The `lms.enrolled` job: mails a learner who took a seat (`welcome`
//! template, in the application's language). Sent at most once per seat as
//! far as the job knows: a receipt is written after the mail is sent and
//! checked before (a run cut between the two sends it again).

use rustcms::async_trait;
use rustcms::mail::{self, Message};
use rustcms::queue::{Failure, Job, JobCtx};
use sea_orm::EntityTrait;
use serde::{Deserialize, Serialize};
use serde_json::json;

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

    fn max_attempts() -> u32 {
        8
    }

    async fn handle(self, ctx: JobCtx) -> anyhow::Result<()> {
        let db = ctx.state.db();
        let (Some(course), Some(learner)) = (
            course::Entity::find_by_id(self.course_id).one(db).await?,
            learner::Entity::find_by_id(self.learner_id).one(db).await?,
        ) else {
            // Removed since: nobody to welcome.
            return Ok(());
        };
        let receipt = format!("welcome:{}:{}", course.id, learner.id);
        if rustcms::models::job_receipt::Entity::find_by_id(receipt.clone()).one(db).await?.is_some() {
            return Ok(());
        }
        let school = rustcms::settings::get::<LmsSettings>(&ctx.state).await?.school_name;
        let data = json!({ "name": learner.name, "course": course.title, "school": school });
        let locale = ctx.state.config().app.locale.clone();
        let sent = async {
            let rendered = mail::render(&ctx.state, "welcome", &locale, &data)?;
            mail::send(&ctx.state, &Message::rendered([learner.email.clone()], rendered)).await
        }
        .await;
        match sent {
            Ok(()) => {}
            Err(err) if err.is_permanent() => return Err(Failure::from(err).permanent().into()),
            Err(err) => return Err(Failure::from(err).into()),
        }
        // A short transaction after the mail, never held across the send.
        let txn = rustcms::db::begin_write(db).await?;
        rustcms::queue::once(&txn, &receipt).await?;
        txn.commit().await?;
        Ok(())
    }
}
