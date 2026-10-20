# ADR 0034: How the Roles page shows permissions

- Status: accepted (0.7.0, P7)
- Date: 2026-10-20

## Context

Apps declare many admin permissions (RDockPanel: apps, docker, settings,
system). The Roles page showed a resource matrix and one box per group with
raw names, and every role was built one checkbox at a time.

## Decision

- **Declared in code, never stored.** `AuthProvider::permission_area`,
  `permission_meta` (label, description, area) and `permission_preset`
  build a `PermissionCatalog` in `AuthState`. Nothing reaches the database;
  `permission:sync` is unchanged. Labels are English sources translated with
  `ctx.t()`.
- **Defaults apart from the app's declarations.** Core ships the areas
  `resources`, `system`, `other` and metadata for its own permissions; the
  app's declarations are checked for duplicates among themselves, then merged
  over the defaults (so the app may relabel a default once).
- **Checked when the panel registers** (`AdminProvider::register`, after the
  panel declared its own permissions), against the permissions declared by
  then: providers declaring referenced permissions register before it.
  Metadata names a declared permission, areas exist, presets name declared
  admin permissions. The builders check ids, texts and duplicates at once.
  Misuse panics, like other builders.
- **Presets grant nothing.** `GET {admin}/api/system/permission-presets` lists
  them (same access as the permission list). Applying one only edits the
  unsaved selection: grantable permissions become exactly the preset's,
  non-grantable ones keep their state. Saving goes through
  `RbacService::set_role_permissions_if` as before (grantability, protected
  roles, `expected_permissions`).
- **Bulk toggles act on what is shown.** Row, column and group toggles change
  only permissions that match the search, sit in an open area and are
  grantable. There is no area-level select-all.

## Consequences

- Metadata cannot change at runtime and has no per-tenant form.
- Permissions without metadata show by name, under "Resources" (matrix cells)
  or "Other"; orphaned permissions are not listed.
