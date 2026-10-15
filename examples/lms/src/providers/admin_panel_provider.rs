//! The admin panel at `/admin` (like a Filament panel provider): its
//! resources, pages and labels.

use rustcms::prelude::*;

use crate::admin::resources::{
    course_resource::CourseResource, enrollment_resource::EnrollmentResource, learner_resource::LearnerResource,
    lesson_resource::LessonResource,
};

pub fn provider() -> AdminProvider {
    AdminProvider::new()
        .brand("lms")
        .media()
        .resource::<CourseResource>()
        .resource::<LessonResource>()
        .resource::<LearnerResource>()
        .resource::<EnrollmentResource>()
        // rustcms:resources
        .settings::<crate::settings::lms_settings::LmsSettings>()
        // rustcms:pages
        // Vietnamese labels (`cargo run -- lang:missing vi` lists what is missing).
        .translations("vi", include_str!("../../lang/vi.toml"))
}
