//! `audit_logs` table (created by `rustcms-migration`). Written through
//! [`crate::audit`], never updated.

use sea_orm::entity::prelude::*;

/// One audited event.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "audit_logs")]
pub struct Model {
    /// UUID v7 (ordered by time).
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    /// When it happened.
    pub at: DateTimeUtc,
    /// `created`, `updated`, `user.roles_changed`, `auth.signed_in`, …
    pub event: String,
    /// A resource slug, or `user`, `role`, `permission`, `session`, `token`, `media`.
    pub subject_type: String,
    /// Key of the subject, if any.
    pub subject_key: Option<String>,
    /// `user`, `system` or `guest`.
    pub actor_kind: String,
    /// The acting user (no foreign key: entries outlive users).
    pub actor_user_id: Option<i64>,
    /// Guard of the acting user.
    pub actor_guard: Option<String>,
    /// `session`, `token:<id>`, `jwt:<family>` or `console:<command>`.
    pub credential: Option<String>,
    /// Client address.
    pub ip: Option<String>,
    /// Client user agent (cut).
    pub user_agent: Option<String>,
    /// `x-request-id` of the request.
    pub request_id: Option<String>,
    /// `{field: [old, new]}` of what changed (redacted, bounded).
    pub changes: Option<Json>,
    /// Facts specific to the event.
    pub meta: Option<Json>,
}

/// No relations.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
