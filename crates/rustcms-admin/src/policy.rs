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
    /// Delete a record.
    pub const DELETE: &str = "delete";
}

/// Decides whether the current user may perform `ability`, optionally on `record`.
///
/// `record` holds every column of the record in wire format (trusted data,
/// not filtered by field permissions), so ownership checks can use any column.
#[async_trait]
pub trait Policy: Send + Sync {
    /// Returns `true` to allow.
    async fn allows(&self, ctx: &Ctx, ability: &str, record: Option<&Data>) -> bool;
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
}
