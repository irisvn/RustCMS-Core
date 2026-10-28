//! A recurring job (`posts-ping`, every hour) and a recurring task
//! (`posts-digest`, every day at 06:00 UTC) whose summary the Schedules page
//! shows. Both can be run from the panel with Run now.

use std::time::Duration;

use rustcms::queue::{Job, JobCtx};
use rustcms::schedule::Schedule;
use rustcms::{AppBuilder, async_trait};
use sea_orm::{EntityTrait, PaginatorTrait};
use serde::{Deserialize, Serialize};

use crate::models::post;

// #region job
/// Counts the posts (a stand-in for real work).
#[derive(Serialize, Deserialize)]
pub struct PingPosts;

#[async_trait]
impl Job for PingPosts {
    const KIND: &'static str = "demo.ping_posts";

    fn describe(&self) -> Option<String> {
        Some("count the posts".into())
    }

    async fn handle(self, ctx: JobCtx) -> anyhow::Result<()> {
        post::Entity::find().count(ctx.state.db()).await?;
        Ok(())
    }
}
// #endregion job

// #region schedule
/// The demo's schedules.
pub fn schedule(app: &mut AppBuilder) {
    app.schedule(Schedule::every("posts-ping", Duration::from_secs(3600)).job(|| PingPosts).without_overlapping());
    app.schedule(
        Schedule::cron("posts-digest", "0 6 * * *")
            .expect("a valid cron expression")
            .task(|ctx| async move {
                let posts = post::Entity::find().count(ctx.state.db()).await?;
                Ok(Some(format!("{posts} posts")))
            })
            .timeout(Duration::from_secs(60)),
    );
}
// #endregion schedule
