//! The authenticated identity attached to a request.

use std::collections::HashSet;

/// Who is making the request, and what they may do.
///
/// Built by a guard (M2). Permission checks go through [`Principal::can`], which
/// applies the super-admin bypass *within the guard* and then intersects with
/// the abilities of the API token, if any.
#[derive(Debug, Clone, Default)]
pub struct Principal {
    /// Encoded primary key of the user.
    pub id: String,
    /// Guard that authenticated this principal (`admin`, `api`).
    pub guard: String,
    /// Effective permissions for this guard (direct + via roles).
    pub permissions: HashSet<String>,
    /// Holds the system `super_admin` role of this guard.
    pub super_admin: bool,
    /// Abilities of the API token used, `None` for session auth.
    pub token_abilities: Option<HashSet<String>>,
}

impl Principal {
    /// Returns whether this principal holds `permission`.
    pub fn can(&self, permission: &str) -> bool {
        let granted = self.super_admin || self.permissions.contains(permission);
        granted
            && match &self.token_abilities {
                None => true,
                Some(abilities) => abilities.contains("*") || abilities.contains(permission),
            }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn principal(perms: &[&str], super_admin: bool, token: Option<&[&str]>) -> Principal {
        Principal {
            id: "1".into(),
            guard: "api".into(),
            permissions: perms.iter().map(|p| p.to_string()).collect(),
            super_admin,
            token_abilities: token.map(|t| t.iter().map(|a| a.to_string()).collect()),
        }
    }

    #[test]
    fn permission_required_without_super_admin() {
        assert!(principal(&["posts.view"], false, None).can("posts.view"));
        assert!(!principal(&["posts.view"], false, None).can("posts.delete"));
    }

    #[test]
    fn super_admin_bypasses_permissions() {
        assert!(principal(&[], true, None).can("anything"));
    }

    #[test]
    fn token_abilities_restrict_even_super_admin() {
        let p = principal(&[], true, Some(&["posts.view"]));
        assert!(p.can("posts.view"));
        assert!(!p.can("posts.delete"));
    }

    #[test]
    fn wildcard_token_keeps_user_permissions() {
        let p = principal(&["posts.view"], false, Some(&["*"]));
        assert!(p.can("posts.view"));
        assert!(!p.can("posts.delete"));
    }
}
