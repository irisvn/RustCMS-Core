//! Enrollments: CRUD permissions, plus progress updates for instructors.

use rustcms::admin::*;

use super::permissions::UPDATE_PROGRESS;

// #region policy
/// `enrollments.<ability>`; `enrollments.update_progress` also admits `update`
/// (field rules then limit the change to `progress`). Decisions never depend
/// on the record: which records a user sees is the resource's scope.
pub struct EnrollmentPolicy;

impl EnrollmentPolicy {
    fn grants(ctx: &Ctx, ability: &str) -> bool {
        ctx.can(&format!("enrollments.{ability}")) || (ability == "update" && ctx.can(UPDATE_PROGRESS))
    }
}

#[async_trait::async_trait]
impl Policy for EnrollmentPolicy {
    async fn allows(&self, ctx: &Ctx, ability: &str, _record: Option<&Data>) -> bool {
        Self::grants(ctx, ability)
    }

    /// Refused before any lookup: without the permission, 403 whether the
    /// enrollment exists or not.
    async fn admits(&self, ctx: &Ctx, ability: &str) -> bool {
        Self::grants(ctx, ability)
    }

    /// Lists need no per-record check (no scan of the matching rows).
    fn view_depends_on_record(&self) -> bool {
        false
    }
}
// #endregion policy
