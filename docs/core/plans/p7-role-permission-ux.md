# P7 plan: Roles page permission areas, search, metadata and presets

Round 2, 2026-10-20 (Codex round 1 applied). Requested by RDockPanel. **Plan only; not approved yet.**
Target: 0.7.0, a minor release: it adds public builders, DTO fields and one
admin route.

## Why
RDockPanel declares many admin permissions: apps, docker, settings, system.
Today the Roles page shows two things:
- a resource matrix;
- one fieldset per `group`, labelled with a humanized group id (`docker.manage` → "Docker").

Users cannot find a permission or learn what it does. Every role is also built
one checkbox at a time.

## Today (code)
- **Declaration:** `AuthProvider::permission(guard, name, group)` and
  `admin_permissions(routes)` store `(guard, name, group)` in
  `PermissionRegistry::declare` (`rustcms-auth/src/state.rs`). `permission:sync`
  writes them to the database.
- **Listing:** `GET {admin}/api/system/permissions` (`features/system/roles.rs`
  `list_permissions`) returns `Vec<PermissionInfo>` with:
  - `name`, `group`, `groupLabel`;
  - `grantable` = `ctx.principal.can(name)`;
  - `resource`, `ability` = matrix cell.
- **UI:** `pages/system/roles.tsx` builds the matrix (row and column toggles)
  and `groupPermissions(rest)` fieldsets.
  - Non-grantable permissions are disabled. Ones already held are kept, because the save sends
    `expected_permissions`.
  - `RbacService::set_role_permissions_if` re-checks grantability, protected and
    privileged roles, and concurrent changes.

## Design

### 1. Metadata (server; code only, no migration)
`AuthProvider` gains three builders:

```rust
AuthProvider::new()
    .permission_area("docker", PermissionArea::new("Docker").sort(20))
    .permission_meta(ADMIN_GUARD, "docker.manage",
        PermissionMeta::new().label("Manage containers")
            .description("Start, stop and remove containers").area("docker"))
    .permission_preset("viewer", PermissionPreset::new("Viewer")
        .description("Read-only access").permissions(["apps.view", "docker.view"]))
```

**Validation:**
- `name`: `[a-z0-9_-]{1,64}` for area and preset ids.
- `label`: 1–80 characters. `description`: at most 300 characters. Neither may contain control characters.
- `sort`: an `i32`.

**At boot** (`register`):
- Each metadata entry must name a permission declared in that guard,
  explicitly or through `admin_permissions`. The check runs after all
  declarations, in `boot`, because `admin_permissions` may come later in the
  chain.
- Its `area` must be declared.
- Each preset applies to the `admin` guard only, because the Roles page manages admin roles. Every name in a preset must be a declared admin permission.
- A violation panics with a message naming the entry, as other builder misuse
  does.
- **Duplicates:** among the app's own declarations, a duplicate
  `permission_meta` for the same name, area id or preset id panics.

**Framework defaults:** core ships defaults, kept apart from the app's
declarations:
- metadata for its own `users.*`, `roles.*` and `permissions.*`;
- three areas: `resources` ("Resources", sort 0), `system` ("System", sort
  1000) and `other` ("Other", sort 2000).

The app's declarations are merged over these defaults after the duplicate
check. An app may therefore declare `permission_area("system", …)` once, or
`permission_meta` for a built-in permission once, to relabel or reorder it.

**Storage:** the metadata lives in `AuthState` next to the registry, in a new
`PermissionCatalog`. Nothing is stored in the database, so `permission:sync`
is unchanged.

**i18n:** labels, descriptions, area labels and preset labels are English
sources, translated with `ctx.t()` like `group_label`. Apps add `vi` strings to
their own translation files.

### 2. DTO and route
- **`PermissionInfo`** gains four optional fields:
  - `label` and `description` (translated);
  - `area` (id) and `areaLabel` (translated).
  - The SPA uses them only when they are present. This is additive for the
    contract (`fixtures/contract/*.json`, `types.gen.ts`).
- **Area by default:** a permission without metadata gets an area anyway.
  - A matrix cell (`resource` set) → area `resources` ("Resources", sort 0).
  - Anything else → area `other` ("Other", sort 2000).
  - The server always fills `area` and `areaLabel`, so the SPA has a single
    grouping rule.
- **New route** `GET {admin}/api/system/permission-presets` returns
  `Vec<PermissionPresetInfo { id, label, description?, permissions: Vec<String> }>`.
  - Access is the same as `list_permissions`: `roles.view_any` or
    `permissions.view_any`.
  - `permissions` lists every name, including ones the caller may not grant,
    filtered to non-orphaned admin permissions. The UI then shows how much of
    the preset the caller can apply.
  - The route needs a case in `fixtures/authz/admin.toml` and goes into
    `rustcms_admin::http::routes`.
- **Areas list:** the SPA needs area order and labels for areas with no visible permission. It derives them from the permission list (`area`, `areaLabel`) plus
  an `areaSort: i32` on each `PermissionInfo`, so no extra route is needed.

### 3. Roles page (SPA)
- **Areas:**
  - One collapsible section per area, ordered by `areaSort`, then label.
  - It uses the existing kit: a `<section>` with a disclosure button, `aria-expanded`
    and `aria-controls`. No new component is needed.
  - The header shows `label · selected/total`. `selected` counts the draft
    selection inside the area over all its permissions, whatever the search.
  - Areas start expanded; with more than 3 areas, all but the first start collapsed.
  - The collapse state is component state only, not persisted.
- **Inside an area:** the layout is unchanged.
  - The matrix of the area's resource cells keeps its row and column toggles.
  - Fieldsets come next, one per `group`, for the rest.
  - Each checkbox shows `label ?? name`. When `label` is set, `name` appears as
    muted monospace text, and `description` appears below in small text.
  - The description is linked with `aria-describedby`.
- **Search:**
  - A text box above the areas matches `name`, `label`, `description` and
    `areaLabel`.
  - Matching ignores case and diacritics: NFD, marks stripped, then `đ/Đ` →
    `d` explicitly, since NFD leaves `đ` intact. So `dieu` finds `điều`.
  - Areas with no match are hidden. Areas with matches expand while a search is
    active.
  - The matrix hides rows and columns with no match.
  - An empty result shows a "no permissions match" line.
  - No debounce: the list is small, so it filters on every keystroke
    through `useDeferredValue`.
- **Bulk toggles:** row, column and group toggles act only on permissions that
  are **visible** (they match the search and sit in an expanded area) **and**
  grantable. The toggle's checked or indeterminate state reads the same set.
  There is no area-level select-all, so a collapsed area cannot be changed in
  bulk.
- **Presets:**
  - A "Start from preset" `DropdownMenu` sits in the editor toolbar. It appears
    only when presets exist and the role is editable.
  - Applying a preset:
    - Grantable permissions take exactly the preset's grantable names: added
      when listed, removed when not.
    - Non-grantable permissions keep their current state, both selected and
      unselected.
    - A toast-free inline notice says "N permissions of this preset cannot be
      granted by you and were left unchanged", when N is above 0.
    - The selection stays an ordinary draft: the user edits it and saves with
      the usual `expected_permissions`. Nothing is saved on apply.
  - Applying a preset ignores the search filter: a preset is explicit, not a
    bulk toggle over what is hidden. The notice states how many permissions
    changed (added/removed).
  - Protected and privileged roles: the menu is disabled with the same reason
    the checkboxes already show.
- **No server change for saving.** `set_role_permissions_if` already refuses
  non-grantable changes, protected roles and stale `expected_permissions`.

## Test
- **Rust** (`crates/rustcms-auth`, `crates/rustcms-admin/tests/system.rs`):
  - Metadata reaches `PermissionInfo` and is translated.
  - Default areas are `resources` and `other`; built-ins land in `system`.
  - An app overriding `system`, `resources`, `other` or a built-in
    permission's metadata once succeeds; declaring it twice panics.
  - Boot panics:
    - metadata for an undeclared permission;
    - an unknown area;
    - a preset naming an undeclared permission or a non-admin guard;
    - duplicates;
    - invalid ids or lengths.
  - The presets route:
    - access (oracle case);
    - orphaned names filtered;
    - non-grantable names still listed.
  - Contract regenerated: `UPDATE_CONTRACT=1`, `UPDATE_TYPES=1`.
- **Vitest** (`admin-ui/tests/roles.test.tsx`, new or extended):
  - Grouping, ordering and counts.
  - Collapse and expand with ARIA.
  - Search: case, diacritics (including `đ`), matrix rows and columns hidden, auto-expand,
    empty state.
  - A bulk row/column/group toggle under search changes only visible grantable
    permissions, and never non-grantable or hidden ones.
  - A preset:
    - replaces only grantable permissions and keeps non-grantable selected and
      unselected ones;
    - shows the notice;
    - leaves the draft unsaved;
    - is disabled on protected roles.
  - The save still sends `expected_permissions`.
- **E2E (Chrome):** the demo declares one area, metadata and a "Viewer" preset.
  - Search, then apply the preset, then save.
  - Reload: the permissions persist.
  - A non-super user sees non-grantable entries disabled and unchanged by the preset.
- Gates as usual: matrix, Vitest, E2E at the end, clippy, fmt, `just generated`.

## Docs
- **Guide:**
  - `security/authorization.md`: the new builders, areas, metadata, presets,
    validation rules, the i18n of labels, and the fact that a preset is a UI
    starting point, not a grant.
  - `admin/overview.md`: the Roles page section.
  - `reference/admin-api.md`: the presets route and the new `PermissionInfo` fields.
  - `release-notes.md`, `upgrade.md`: additive, no action needed. Apps with
    custom permission groups will see them under "Other" unless they declare areas.
- **Core:**
  - `CHANGELOG.md` [0.7.0];
  - `admin-protocol.md`;
  - ADR 0012 (RBAC) amended: a preset is client-side only and grants nothing;
  - `review-checklist.md`: bulk toggles act only on visible grantable
    permissions.
- **Demo:** `examples/demo` gets an area, metadata and a preset.
- After implementing: recheck all docs against the code, then a Codex docs review.

## Out of scope
- Presets stored in the database or editable at runtime.
- Presets for non-admin guards.
- Metadata from the database.
- Area-level select-all.
- Per-permission dependencies (e.g. `update` implies `view`).
