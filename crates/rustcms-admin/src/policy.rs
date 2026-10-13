//! Record-level authorization. Deny by default.

use async_trait::async_trait;

use crate::Data;
use crate::context::Ctx;

/// Standard abilities checked by the CRUD endpoints.
pub mod ability {
    /// List records.
    pub const VIEW_ANY: &str = "view_any";
    /// Read one record.
    pub const VIEW: &str = "view";
    /// Create a record.
    pub const CREATE: &str = "create";
    /// Update a record.
    pub const UPDATE: &str = "update";
    /// Delete a record (move it to the trash with soft deletes).
    pub const DELETE: &str = "delete";
    /// Restore a trashed record (soft deletes).
    pub const RESTORE: &str = "restore";
    /// Delete a trashed record for good (soft deletes).
    pub const FORCE_DELETE: &str = "force_delete";
}

/// Decides whether the current user may perform `ability`, optionally on `record`.
///
/// `record` holds every column of the record in wire format (trusted data,
/// not filtered by field permissions), so ownership checks can use any column.
#[async_trait]
pub trait Policy: Send + Sync {
    /// Returns `true` to allow.
    async fn allows(&self, ctx: &Ctx, ability: &str, record: Option<&Data>) -> bool;

    /// Whether `view` can differ between records. When `true` (the safe
    /// default), lists check every record and never show (or count) denied ones,
    /// which costs a scan of the matching rows. Return `false` when record
    /// visibility is fully expressed by `Resource::scope()`.
    fn view_depends_on_record(&self) -> bool {
        true
    }

    /// Whether the user may perform `ability` on *some* record, checked before
    /// a record is looked up by key. Refusing here answers 403 whether or not
    /// the key exists, so a user without the ability cannot probe which keys
    /// exist. The default admits everyone and leaves the decision to
    /// [`Policy::allows`] on the record (then a missing key answers 404 and a
    /// denied one 403): override it when the ability needs a permission
    /// regardless of the record, or hide records with `Resource::scope()`.
    async fn admits(&self, _ctx: &Ctx, _ability: &str) -> bool {
        true
    }
}

/// Default policy: `{slug}.{ability}` must be granted to the user.
pub struct PermissionPolicy {
    slug: &'static str,
}

impl PermissionPolicy {
    /// Policy for the resource registered under `slug`.
    pub fn new(slug: &'static str) -> Self {
        Self { slug }
    }
}

#[async_trait]
impl Policy for PermissionPolicy {
    async fn allows(&self, ctx: &Ctx, ability: &str, _record: Option<&Data>) -> bool {
        ctx.can(&format!("{}.{}", self.slug, ability))
    }

    async fn admits(&self, ctx: &Ctx, ability: &str) -> bool {
        self.allows(ctx, ability, None).await
    }

    fn view_depends_on_record(&self) -> bool {
        false
    }
}
