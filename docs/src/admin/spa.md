# The panel (SPA)

The admin panel is a single-page app that renders the schemas of your
resources. It is served by `AdminProvider` under `auth.admin_path` (default
`/admin`) and is embedded in your binary: using rustcms never requires Node.

| Path | Serves |
|---|---|
| `/admin`, `/admin/…` | the panel shell (`index.html`, `Cache-Control: no-store`) |
| `/admin/assets/…` | hashed JS/CSS (`immutable`); a missing file is 404 |
| `/admin/api/…` | the JSON API ([protocol](../admin-protocol.md)); unknown paths are JSON 404 |

Pages: `/admin/login`, `/admin/r/<slug>` (list), `/admin/r/<slug>/create`,
`/admin/r/<slug>/records/<key>` (view) and `…/edit`.

## What users see

- **Navigation**: resources the user may list (`view_any`), grouped by
  `ResourceMeta::group`, with `ResourceMeta::icon` (`box`, `file-text`,
  `folder`, `image`, `settings`, `shield`, `shopping-cart`, `tag`, `users`).
- **Lists**: search, sortable columns, filters and pagination are kept in the
  URL. Row buttons follow each record's `abilities`; bulk actions appear when
  rows are selected; confirmations come from `requires_confirmation`.
- **Forms**: built from the schema of the operation. Disabled inputs are
  read-only; write-only inputs (passwords) start empty and are sent only when
  filled in; edits send changed fields only. Validation messages from the
  server appear under each input.
- Someone who may update a record but not view it gets an empty edit form;
  records outside a user's scope show "Not found", forbidden pages "Access denied".

## Layout and accessibility

The panel uses a monochrome, shadcn-block-inspired design: neutral surfaces,
black primary controls in light mode, and white primary controls in dark mode.
The theme follows the operating system; there is no separate theme setting.
Error and destructive states retain a restrained red accent. Labels and icons,
not color alone, identify record status.

On desktop, navigation stays in a scrollable sidebar that can collapse to icons.
On smaller screens, **Open navigation** opens a native modal drawer. Escape or
**Close navigation** closes it and returns focus to the trigger; following a
link closes it too. A skip-to-content link and breadcrumbs help keyboard users.
Sidebar changes and background panel refreshes do not reset page forms.
The overview offers shortcuts from the user's authorized navigation, not
invented analytics or additional permissions.

Lists separate search/filters from selected-row actions. **Filters** collapses
or expands the filter area; **Clear filters** retains search, sorting and page
size. Sort headings expose their direction and select-all exposes partial
selection. Wide tables scroll inside their own region rather than widening the
whole page. Forms have a bounded reading width, responsive sections, and helper
and validation messages associated with their fields.

Record details show selectable label/value pairs from the view schema and the
actual response. Missing values do not become schema defaults; hidden and
write-only inputs are never rendered. Relationship labels use the same scoped
options endpoint, with the raw key and an explicit notice if a label cannot be
loaded. The relationship picker supports arrow keys, Enter, Escape and Tab,
without intercepting IME text composition.

Components use compiled CSS, system fonts, native `<select>` and `<dialog>`.
Do not copy third-party blocks that inject styles, add inline `style` properties,
or load fonts/scripts from another origin: the production CSP deliberately
forbids them. Responsive sizing uses static classes/CSS rather than inline
custom properties.

## Security

- CSP: `default-src 'self'; script-src 'self'; style-src 'self'; …;
  frame-ancestors 'none'`, plus `X-Frame-Options: DENY`, `nosniff` and
  `Referrer-Policy: same-origin`.
- The session cookie is `HttpOnly`, `SameSite=Lax`, scoped to the admin path.
  The CSRF token lives in memory only and is fetched again after each page load,
  sign-in and sign-out.
- Signing out (or a 401, or signing out in another tab) clears every cached
  record, schema and form. Admin API responses are `private, no-store`.
- After sign-in the user returns to the page they asked for, never outside the
  panel (`?redirect=` is validated).
- A bundle older than the server shows a banner; if the schema structure is
  newer than the bundle understands, editing is disabled.

## Developing the panel

Only needed to change `admin-ui/` itself (Node 20+):

```sh
just admin-install   # npm ci
just dev             # Rust on :8000, Vite on :5173 → open http://localhost:5173/admin/
just test-ui         # Vitest
just e2e             # Playwright against the demo app (npx playwright install chromium once)
just admin-build     # rebuild crates/rustcms-admin/assets — commit the result
```

For UI-only changes, run frontend lint, type checking, Vitest, then rebuild the
assets before browser tests:

```sh
npm --prefix admin-ui run lint
npm --prefix admin-ui run typecheck
just test-ui
just admin-build
CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 \
  PLAYWRIGHT_CHANNEL=chrome just e2e
```

`PLAYWRIGHT_CHANNEL=chrome` uses an installed Chrome; omit it to use Playwright's
bundled Chromium. The command-only Cargo overrides reduce local debug artifacts
without changing workspace profiles. E2E uses a fresh temporary SQLite database,
not the demo's local data. Responsive smoke tests cover light/dark themes,
navigation focus behavior and document-level overflow. For embedded-asset
verification, build `cargo build -p demo --release` after `just admin-build` and
run Playwright with `RUSTCMS_DEMO_BIN` set to the **absolute** release binary path;
the E2E server runs outside the checkout.

In development Vite proxies `/admin/api` to the Rust server and forwards the
browser's `Origin`, so `just dev` sets
`RUSTCMS_AUTH__ADMIN_ORIGINS='["http://localhost:8000","http://localhost:5173"]'`
(a non-empty list replaces the `APP_URL` default).

Protocol types in `admin-ui/src/api/types.gen.ts` are generated from the Rust
DTOs: `UPDATE_TYPES=1 cargo test -p rustcms-admin --features ts --test typescript`.
The contract fixtures in `fixtures/contract/` (including `kitchen-sink.*`, which
covers every component and wire kind) are rendered by the Vitest suite.
