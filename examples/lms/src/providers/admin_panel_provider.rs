//! The admin panel at `/admin` (like a Filament panel provider): its
//! resources, pages and labels.

use rustcms::admin::Theme;
use rustcms::admin::dashboard::{Bucket, Dashboard, Widget};
use rustcms::prelude::*;

use crate::admin::resources::{
    category_resource::CategoryResource, course_resource::CourseResource, enrollment_resource::EnrollmentResource,
    learner_resource::LearnerResource, lesson_resource::LessonResource, news_resource::NewsResource,
    tag_resource::TagResource,
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
        .resource::<NewsResource>()
        .resource::<CategoryResource>()
        .resource::<TagResource>()
        // rustcms:resources
        .settings::<crate::settings::lms_settings::LmsSettings>()
        .mail_settings()
        .dashboard(
            Dashboard::new()
                .widget(Widget::stat("learners", "learners").label("Learners"))
                .widget(
                    Widget::stat("new_enrollments", "enrollments")
                        .label("New enrollments")
                        .compare_last_days(30, "enrolled_at"),
                )
                .widget(
                    Widget::chart("enrollments_per_week", "enrollments", "enrolled_at")
                        .label("Enrollments per week")
                        .per(Bucket::Week)
                        .last(12)
                        .span(2),
                )
                .widget(
                    Widget::recent("new_courses", "courses")
                        .label("New courses")
                        .columns(&["capacity"])
                        .sort("-created_at"),
                ),
        )
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
