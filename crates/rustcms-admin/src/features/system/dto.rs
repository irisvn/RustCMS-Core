//! What the system pages receive (users, roles, permissions).

use super::*;

/// Which system pages the user may open (`PanelResponse.system`).
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SystemLinks {
    /// Users page (`users.view_any`).
    pub users: bool,
    /// Roles page (`roles.view_any`).
    pub roles: bool,
    /// May create users (`users.create`).
    pub create_users: bool,
    /// May invite users (`users.create` and `users.invite`).
    pub invite_users: bool,
    /// May create roles (`roles.create`).
    pub create_roles: bool,
    /// The audit log (`audit.view_all`).
    pub audit: bool,
    /// The Queue page (`queue.view`).
    pub queue: bool,
}

impl SystemLinks {
    pub(crate) fn of(principal: &Principal) -> Self {
        Self {
            users: principal.can("users.view_any"),
            roles: principal.can("roles.view_any"),
            create_users: principal.can("users.create"),
            invite_users: principal.can("users.create") && principal.can("users.invite"),
            create_roles: principal.can("roles.create"),
            audit: principal.can(crate::audit::VIEW_ALL),
            queue: principal.can(crate::queue::VIEW),
        }
    }
}

/// A role as listed on a user.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct RoleRef {
    /// Role id.
    pub id: String,
    /// Role name.
    pub name: String,
}

/// A user on the system pages.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SystemUser {
    /// User id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Sign-in email.
    pub email: String,
    /// May sign in.
    pub is_active: bool,
    /// Invited, the invitation not yet accepted.
    pub invitation_pending: bool,
    /// When they were invited (RFC 3339).
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invited_at: Option<String>,
    /// Roles held in the admin guard.
    pub roles: Vec<RoleRef>,
    /// Holds the admin `super_admin` role.
    pub super_admin: bool,
    /// The signed-in user themselves (managed on `/account`).
    pub is_self: bool,
    /// What the current user may do with this user (the server re-checks).
    pub abilities: BTreeMap<&'static str, bool>,
}

/// `GET system/users`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct UserPage {
    /// Users.
    pub data: Vec<SystemUser>,
    /// Pagination.
    pub meta: PageMeta,
}

/// A role on the system pages.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SystemRole {
    /// Role id.
    pub id: String,
    /// Role name.
    pub name: String,
    /// Created by migrations (immutable, never deleted).
    pub is_system: bool,
    /// Managed by super admins only.
    pub is_privileged: bool,
    /// Permission names.
    pub permissions: Vec<String>,
    /// Users holding it.
    pub users: u64,
    /// What the current user may do with this role (the server re-checks).
    pub abilities: BTreeMap<&'static str, bool>,
}

/// A declared admin permission.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct PermissionInfo {
    /// Name (`posts.update`).
    pub name: String,
    /// Group (usually the resource slug).
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// The group as shown: a resource's plural label, else the group, in the
    /// request's language. The group itself stays the identifier.
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_label: Option<String>,
    /// The current user holds it, so may grant or remove it.
    pub grantable: bool,
    /// The resource whose ability it is (`posts`), when it is exactly one
    /// resource's `{slug}.{ability}`: a cell of the permission matrix.
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource: Option<String>,
    /// That ability (`update`, `attach_tags`, an action's).
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ability: Option<String>,
}
