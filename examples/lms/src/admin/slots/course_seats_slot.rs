//! Beside a course: active and withdrawn enrollments, average progress.

use rustcms::admin::{Ctx, Envelope, PageView, PanelSlot, Placement, SlotMeta};
use rustcms::{AppResult, async_trait};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde_json::{Value, json};

use crate::models::enrollment;

pub struct CourseSeatsSlot;

#[async_trait]
impl PanelSlot for CourseSeatsSlot {
    const NAME: &'static str = "course-seats";

    fn placement() -> Placement {
        Placement::RecordAside("courses")
    }

    fn meta() -> SlotMeta {
        SlotMeta::new().label("Seats").permission("courses.view")
    }

    fn view() -> PageView {
        rustcms::admin::view!("course_seats.html")
    }

    /// The course is one the user may view (the panel checked): its counts are theirs to see.
    async fn data(ctx: &Ctx, record: &Envelope) -> AppResult<Value> {
        let id: i64 = record.key.parse().unwrap_or_default();
        let rows = enrollment::Entity::find().filter(enrollment::Column::CourseId.eq(id)).all(&ctx.db).await?;
        let active: Vec<&enrollment::Model> = rows.iter().filter(|e| e.status == enrollment::ACTIVE).collect();
        let progress = if active.is_empty() {
            0
        } else {
            active.iter().map(|e| i64::from(e.progress)).sum::<i64>() / active.len() as i64
        };
        Ok(json!({
            "active": active.len(),
            "withdrawn": rows.len() - active.len(),
            "capacity": record.data.get("capacity"),
            "progress": progress,
        }))
    }
}
