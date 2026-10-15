//! Course overview (`/admin/p/course-overview`): seats of the courses the
//! user may see, from a server template.

use rustcms::admin::{Ctx, ListQuery, PageView, PanelPage, PanelPageMeta};
use rustcms::{AppResult, async_trait};
use serde_json::{Value, json};

use crate::admin::int;
use crate::services::enrollment_service;

pub struct CourseOverviewPage;

#[async_trait]
impl PanelPage for CourseOverviewPage {
    const SLUG: &'static str = "course-overview";

    fn meta() -> PanelPageMeta {
        PanelPageMeta::new("Course overview").icon("layers").group("Learning").permission("courses.view_any")
    }

    fn view() -> PageView {
        rustcms::admin::view!("course_overview.html")
    }

    /// The courses as the user may list them (instructors: theirs), with the
    /// seats taken: counted for those courses only.
    async fn data(ctx: &Ctx) -> AppResult<Value> {
        let query = ListQuery { per_page: Some(100), ..Default::default() };
        let courses = ctx.records("courses", &query).await?;
        let mut out = Vec::new();
        for course in courses.data {
            let id = int(&course.data, "id").or_else(|| course.key.parse().ok()).unwrap_or_default();
            out.push(json!({
                "key": course.key,
                "title": course.title,
                "capacity": course.data.get("capacity"),
                "published": course.data.get("published"),
                "active": enrollment_service::active_count(&ctx.db, id).await?,
            }));
        }
        Ok(json!({ "courses": out }))
    }
}
