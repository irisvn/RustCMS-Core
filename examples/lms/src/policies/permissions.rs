//! Permissions beyond each resource's CRUD abilities (declared in
//! `providers/auth_service_provider.rs`).

// #region permissions
/// Every course, not only the ones taught.
pub const ALL_COURSES: &str = "lms.all_courses";
/// Choosing a course's instructor.
pub const ASSIGN_INSTRUCTOR: &str = "courses.assign_instructor";
/// Recording a learner's progress (the only change to an enrollment it allows).
pub const UPDATE_PROGRESS: &str = "enrollments.update_progress";
// #endregion permissions
