# 0019 — Customizing the admin panel

- Status: accepted
- Date: 2026-10-21

## Context

Applications need their own look, pages (reports, overviews) and content
beside records, as Filament's custom pages and widgets. The panel is a SPA
under a strict CSP (`'self'` only, no inline script or style) and must keep
deny-by-default authorization and field read rules everywhere.

## Decision

- **Theme**: validated tokens only (hex colors, bounded radius, quoted
  font-family names, raster logo/favicon checked against their bytes),
  compiled at boot into a hashed stylesheet and images under
  `assets/ext/`. Dark mode follows `data-scheme` on `<html>` (set from
  the browser's saved choice, falling back to `ColorScheme`; `system` follows
  the OS) with an OS fallback before the SPA
  runs. SVG logos, `data:` images and web fonts are deferred.
- **Pages and slots** (`PanelPage`, `PanelSlot`, `Placement::RecordAside`):
  declared in Rust with mandatory permissions (boot fails otherwise),
  rendered per request either from a **server template** or by an
  **extension script**:
  - templates: minijinja, HTML autoescape always on, no `safe`/`tojson`/
    `autoescape`, no includes (statements found by a lexer, whatever the
    spacing), fuel and recursion limits, output written into a buffer that
    stops at 512 KiB; templates are trusted application code (the limits
    bound data-driven output, not a hostile template);
    then **ammonia** with an explicit per-element allowlist (no script,
    style, handlers, forms, frames, svg, `id`/`name`), URLs validated after
    entity decoding (panel paths on segment boundaries, not API/assets, no
    traversal; `https:`/`mailto:` links; images from the panel only). The SPA
    inserts this output (its only raw HTML) and routes internal links.
  - scripts: `registerPage` / `registerSlot` / `registerCell` with the
    widget lifecycle (`mount` → `update` → `unmount`, instance checked),
    copies of data, errors contained, callbacks bound to the mount and the
    session epoch; content whose refetch fails is dropped; scripts are
    trusted code with the panel's privileges.
- **Data**: `data()` is application code; `Ctx::records` / `Ctx::record`
  read through resources (scope, policy, readable fields). Slots get the
  record as its page loads it.
- **Admission**: no page access → 403 for every slug (no probing); unknown
  slug 404 otherwise; slots: the resource's `admits(view)` before parameter
  errors, then the record as `find`; slots without their permissions omitted
  before `data`.
- **Cells**: `ColumnSchema.cell` / `props`; vocabulary 6.
- Files in `resources/` (`views/admin`, `admin/{js,css}`), `make:page`.
- **Template files** (`view!`): embedded at build time; a debug build with
  `APP_ENV=local` reads the file again at each render, through the same
  checks, environment and sanitizer (an error is a 500, not a panic). The
  condition is both the build profile (decided in the application crate by
  the macro) and the environment, so a release binary never reads source
  files, whatever its configuration.

## Consequences

- No new CSP directive; server HTML is defense in depth over application
  templates the developer controls.
- Template text participates in `lang:missing`; pages re-render per locale.
- Eject (copying the SPA into the app) and a public headless Admin API remain
  later options.
