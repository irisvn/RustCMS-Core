//! Who may do what. Staff (`lms.all_courses`) work on every course; everyone
//! else, typically instructors, sees the courses they teach and what belongs
//! to them (the resources' scopes).

pub mod enrollment_policy;
pub mod permissions;
// rustcms:policies

use rustcms::admin::Ctx;

/// The instructor whose courses bound what `ctx` sees, or `None` for staff.
pub fn instructor(ctx: &Ctx) -> Option<i64> {
    if ctx.can(permissions::ALL_COURSES) {
        return None;
    }
    // A principal that is not a user id sees nothing.
    Some(ctx.principal.id.parse::<i64>().unwrap_or(-1))
}
