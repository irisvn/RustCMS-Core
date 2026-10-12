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

In development Vite proxies `/admin/api` to the Rust server and forwards the
browser's `Origin`, so `just dev` sets
`RUSTCMS_AUTH__ADMIN_ORIGINS='["http://localhost:8000","http://localhost:5173"]'`
(a non-empty list replaces the `APP_URL` default).

Protocol types in `admin-ui/src/api/types.gen.ts` are generated from the Rust
DTOs: `UPDATE_TYPES=1 cargo test -p rustcms-admin --features ts --test typescript`.
The contract fixtures in `fixtures/contract/` (including `kitchen-sink.*`, which
covers every component and wire kind) are rendered by the Vitest suite.
