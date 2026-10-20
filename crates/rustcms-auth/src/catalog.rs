//! How the Roles page shows permissions: areas, labels, descriptions and
//! presets. Declared in code ([`crate::AuthProvider::permission_area`] and
//! friends), never stored; nothing here grants anything.

use std::collections::BTreeMap;

use crate::ADMIN_GUARD;

/// Area of permissions that are exactly one resource's ability (the matrix).
pub const RESOURCES_AREA: &str = "resources";
/// Area of the framework's users, roles and permissions.
pub const SYSTEM_AREA: &str = "system";
/// Area of everything not declared elsewhere.
pub const OTHER_AREA: &str = "other";

/// A section of the Roles page.
///
/// ```rust,ignore
/// AuthProvider::new().permission_area("docker", PermissionArea::new("Docker").sort(20))
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionArea {
    /// English label, translated when shown.
    pub label: String,
    /// Lower first; ties by label.
    pub sort: i32,
}

impl PermissionArea {
    /// An area labelled `label` (1–80 characters), sort 0.
    ///
    /// # Panics
    /// On an empty or longer label, or control characters.
    pub fn new(label: &str) -> Self {
        text("permission area label", label, 1, 80);
        Self { label: label.to_owned(), sort: 0 }
    }

    /// Position among areas (lower first).
    pub fn sort(mut self, sort: i32) -> Self {
        self.sort = sort;
        self
    }
}

/// How one permission is shown.
///
/// ```rust,ignore
/// PermissionMeta::new().label("Manage containers").description("Start and stop containers").area("docker")
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PermissionMeta {
    /// English label (else the name is shown).
    pub label: Option<String>,
    /// English description.
    pub description: Option<String>,
    /// Area id (else `resources` for matrix cells, `other` for the rest).
    pub area: Option<String>,
}

impl PermissionMeta {
    /// No label, description or area.
    pub fn new() -> Self {
        Self::default()
    }

    /// 1–80 characters.
    ///
    /// # Panics
    /// On an empty or longer label, or control characters.
    pub fn label(mut self, label: &str) -> Self {
        text("permission label", label, 1, 80);
        self.label = Some(label.to_owned());
        self
    }

    /// 1–300 characters.
    ///
    /// # Panics
    /// On an empty or longer description, or control characters.
    pub fn description(mut self, description: &str) -> Self {
        text("permission description", description, 1, 300);
        self.description = Some(description.to_owned());
        self
    }

    /// A declared area id.
    ///
    /// # Panics
    /// On an invalid id (`[a-z0-9_-]`, 1–64).
    pub fn area(mut self, area: &str) -> Self {
        id("permission area", area);
        self.area = Some(area.to_owned());
        self
    }
}

/// A starting selection on the Roles page ("Viewer", "Developer"). Applying it
/// only changes the draft; saving grants as any selection does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionPreset {
    /// English label.
    pub label: String,
    /// English description.
    pub description: Option<String>,
    /// Admin permissions it selects.
    pub permissions: Vec<String>,
}

impl PermissionPreset {
    /// A preset labelled `label` (1–80 characters), selecting nothing.
    ///
    /// # Panics
    /// On an empty or longer label, or control characters.
    pub fn new(label: &str) -> Self {
        text("permission preset label", label, 1, 80);
        Self { label: label.to_owned(), description: None, permissions: Vec::new() }
    }

    /// 1–300 characters.
    ///
    /// # Panics
    /// As [`PermissionMeta::description`].
    pub fn description(mut self, description: &str) -> Self {
        text("permission preset description", description, 1, 300);
        self.description = Some(description.to_owned());
        self
    }

    /// The admin permissions it selects (declared ones, checked at boot).
    pub fn permissions<I, S>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.permissions.extend(names.into_iter().map(Into::into));
        self.permissions.sort();
        self.permissions.dedup();
        self
    }
}

/// Every area, permission label and preset, framework defaults included.
#[derive(Debug, Clone)]
pub struct PermissionCatalog {
    areas: BTreeMap<String, PermissionArea>,
    meta: BTreeMap<(String, String), PermissionMeta>,
    presets: Vec<(String, PermissionPreset)>,
}

impl Default for PermissionCatalog {
    fn default() -> Self {
        let mut areas = BTreeMap::new();
        areas.insert(RESOURCES_AREA.to_owned(), PermissionArea::new("Resources"));
        areas.insert(SYSTEM_AREA.to_owned(), PermissionArea::new("System").sort(1000));
        areas.insert(OTHER_AREA.to_owned(), PermissionArea::new("Other").sort(2000));
        let mut meta = BTreeMap::new();
        for (_, name) in crate::provider::BUILT_IN_PERMISSIONS {
            meta.insert((ADMIN_GUARD.to_owned(), (*name).to_owned()), PermissionMeta::new().area(SYSTEM_AREA));
        }
        Self { areas, meta, presets: Vec::new() }
    }
}

/// What the application declared, merged over the defaults at register.
#[derive(Debug, Clone, Default)]
pub(crate) struct Declarations {
    pub(crate) areas: Vec<(String, PermissionArea)>,
    pub(crate) meta: Vec<((String, String), PermissionMeta)>,
    pub(crate) presets: Vec<(String, PermissionPreset)>,
}

impl Declarations {
    pub(crate) fn area(&mut self, id_: &str, area: PermissionArea) {
        id("permission area", id_);
        assert!(!self.areas.iter().any(|(a, _)| a == id_), "permission area `{id_}` is declared twice");
        self.areas.push((id_.to_owned(), area));
    }

    pub(crate) fn meta(&mut self, guard: &str, name: &str, meta: PermissionMeta) {
        assert!(
            !self.meta.iter().any(|((g, n), _)| g == guard && n == name),
            "permission_meta for `{guard}` `{name}` is declared twice"
        );
        self.meta.push(((guard.to_owned(), name.to_owned()), meta));
    }

    pub(crate) fn preset(&mut self, id_: &str, preset: PermissionPreset) {
        id("permission preset", id_);
        assert!(!self.presets.iter().any(|(p, _)| p == id_), "permission preset `{id_}` is declared twice");
        self.presets.push((id_.to_owned(), preset));
    }

    /// The defaults with these declarations over them.
    pub(crate) fn catalog(&self) -> PermissionCatalog {
        let mut catalog = PermissionCatalog::default();
        catalog.areas.extend(self.areas.iter().cloned());
        catalog.meta.extend(self.meta.iter().cloned());
        catalog.presets.clone_from(&self.presets);
        catalog
    }
}

impl PermissionCatalog {
    /// Checks every reference against the declared permissions (`(guard,
    /// name)` keys of [`crate::PermissionRegistry::all`]).
    ///
    /// # Panics
    /// On metadata for an undeclared permission, an unknown area, or a preset
    /// naming a permission the admin guard does not declare.
    pub fn check<'a>(&self, declared: impl IntoIterator<Item = &'a (String, String)>) {
        let declared: std::collections::BTreeSet<&(String, String)> = declared.into_iter().collect();
        for ((guard, name), meta) in &self.meta {
            let key = (guard.clone(), name.clone());
            assert!(declared.contains(&key), "permission_meta: `{guard}` `{name}` is not a declared permission");
            if let Some(area) = &meta.area {
                assert!(self.areas.contains_key(area), "permission_meta: `{name}`: unknown area `{area}`");
            }
        }
        for (preset, p) in &self.presets {
            for name in &p.permissions {
                let key = (ADMIN_GUARD.to_owned(), name.clone());
                assert!(
                    declared.contains(&key),
                    "permission preset `{preset}`: `{name}` is not a declared admin permission"
                );
            }
        }
    }

    /// The area `id`, if declared.
    pub fn area(&self, id: &str) -> Option<&PermissionArea> {
        self.areas.get(id)
    }

    /// How `name` of `guard` is shown, if declared.
    pub fn meta(&self, guard: &str, name: &str) -> Option<&PermissionMeta> {
        self.meta.get(&(guard.to_owned(), name.to_owned()))
    }

    /// Presets in declaration order.
    pub fn presets(&self) -> &[(String, PermissionPreset)] {
        &self.presets
    }

    /// Every English string shown (for `lang:missing`).
    pub fn display_strings(&self) -> Vec<String> {
        let mut out: Vec<String> = self.areas.values().map(|a| a.label.clone()).collect();
        for meta in self.meta.values() {
            out.extend(meta.label.iter().cloned());
            out.extend(meta.description.iter().cloned());
        }
        for (_, preset) in &self.presets {
            out.push(preset.label.clone());
            out.extend(preset.description.iter().cloned());
        }
        out
    }
}

fn id(what: &str, value: &str) {
    assert!(
        (1..=64).contains(&value.len())
            && value.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-'),
        "{what} id `{value}`: 1–64 of a-z, 0-9, `_` and `-`"
    );
}

fn text(what: &str, value: &str, min: usize, max: usize) {
    let count = value.chars().count();
    assert!(
        (min..=max).contains(&count) && !value.trim().is_empty(),
        "{what} must be {min}–{max} characters, not {count}"
    );
    assert!(!value.chars().any(char::is_control), "{what} must not contain control characters");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn declared(names: &[(&str, &str)]) -> Vec<(String, String)> {
        names.iter().map(|(g, n)| ((*g).to_owned(), (*n).to_owned())).collect()
    }

    #[test]
    fn app_declarations_override_defaults_once() {
        let mut d = Declarations::default();
        d.area(SYSTEM_AREA, PermissionArea::new("Platform").sort(5));
        d.meta(ADMIN_GUARD, "users.view_any", PermissionMeta::new().label("See users"));
        let catalog = d.catalog();
        assert_eq!(catalog.area(SYSTEM_AREA).unwrap().label, "Platform");
        assert_eq!(catalog.area(OTHER_AREA).unwrap().sort, 2000);
        assert_eq!(catalog.meta(ADMIN_GUARD, "users.view_any").unwrap().label.as_deref(), Some("See users"));
        assert_eq!(catalog.meta(ADMIN_GUARD, "roles.view").unwrap().area.as_deref(), Some(SYSTEM_AREA));
        assert!(std::panic::catch_unwind(|| d.clone().area(SYSTEM_AREA, PermissionArea::new("Again"))).is_err());
        assert!(
            std::panic::catch_unwind(|| d.clone().meta(ADMIN_GUARD, "users.view_any", PermissionMeta::new())).is_err()
        );
        let mut p = Declarations::default();
        p.preset("viewer", PermissionPreset::new("Viewer"));
        assert!(std::panic::catch_unwind(move || p.preset("viewer", PermissionPreset::new("Viewer"))).is_err());
    }

    #[test]
    fn references_are_checked() {
        let mut ok = declared(&[("admin", "docker.view"), ("api", "posts.view")]);
        // The framework's own carry default metadata.
        ok.extend(crate::BUILT_IN_PERMISSIONS.iter().map(|(_, n)| (ADMIN_GUARD.to_owned(), (*n).to_owned())));
        let check = |d: Declarations| {
            let ok = ok.clone();
            std::panic::catch_unwind(move || d.catalog().check(&ok)).is_err()
        };
        let mut d = Declarations::default();
        d.meta("admin", "docker.view", PermissionMeta::new().area("docker"));
        assert!(check(d.clone()), "unknown area");
        d.area("docker", PermissionArea::new("Docker"));
        assert!(!check(d.clone()));
        let mut undeclared = Declarations::default();
        undeclared.meta("admin", "docker.nope", PermissionMeta::new());
        assert!(check(undeclared));
        let mut api = Declarations::default();
        api.preset("x", PermissionPreset::new("X").permissions(["posts.view"]));
        assert!(check(api), "presets name admin permissions only");
    }

    #[test]
    fn texts_and_ids_are_validated() {
        assert!(std::panic::catch_unwind(|| PermissionArea::new("")).is_err());
        assert!(std::panic::catch_unwind(|| PermissionArea::new(&"x".repeat(81))).is_err());
        assert!(std::panic::catch_unwind(|| PermissionMeta::new().label("a\nb")).is_err());
        assert!(std::panic::catch_unwind(|| PermissionMeta::new().description(&"x".repeat(301))).is_err());
        assert!(std::panic::catch_unwind(|| PermissionMeta::new().area("Docker")).is_err());
        assert!(
            std::panic::catch_unwind(|| Declarations::default().preset("a b", PermissionPreset::new("X"))).is_err()
        );
        assert_eq!(PermissionArea::new(&"ệ".repeat(80)).label.chars().count(), 80);
    }
}
