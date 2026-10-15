//! The admin panel at `/admin` (like a Filament panel provider): its
//! resources, pages and labels.

use rustcms::admin::Theme;
use rustcms::prelude::*;

use crate::admin::resources::{
    course_resource::CourseResource, enrollment_resource::EnrollmentResource, learner_resource::LearnerResource,
    lesson_resource::LessonResource,
};

pub fn provider() -> AdminProvider {
    AdminProvider::new()
        .brand("lms")
        .theme(
            Theme::new()
                .primary("#2563eb")
                .primary_foreground("#ffffff")
                .dark(|dark| dark.primary("#60a5fa").primary_foreground("#0b1220"))
                .radius("0.625rem")
                .logo(include_bytes!("../../resources/admin/logo.png"), "image/png")
                .favicon(include_bytes!("../../resources/admin/favicon.png"), "image/png"),
        )
        .media()
        .resource::<CourseResource>()
        .resource::<LessonResource>()
        .resource::<LearnerResource>()
        .resource::<EnrollmentResource>()
        // rustcms:resources
        .settings::<crate::settings::lms_settings::LmsSettings>()
        .page::<crate::admin::pages::course_overview_page::CourseOverviewPage>()
        // rustcms:pages
        .slot::<crate::admin::slots::course_seats_slot::CourseSeatsSlot>()
        // The `lms/progress` cell of the enrollments table.
        .extension_script("lms-progress", include_str!("../../resources/admin/js/lms/progress.js"))
        .extension_style("lms-progress", include_str!("../../resources/admin/css/lms/progress.css"))
        // rustcms:extensions
        // Vietnamese labels (`cargo run -- lang:missing vi` lists what is missing).
        .translations("vi", include_str!("../../lang/vi.toml"))
}
