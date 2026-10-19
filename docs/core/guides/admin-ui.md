# Developing the admin panel (admin-ui)

For contributors to RustCMS itself. Applications need Node only to build
their own [React pages](../../guide/src/admin/react-pages.md) (`resources/admin`,
`^20.19.0 || >=22.12.0`); compiling and running their binaries does not.


Only needed to change `admin-ui/` itself (Node 22.12+, 24 or 26+: Vitest's range):

```sh
just admin-install   # npm ci
just dev             # Rust on :8000, Vite on :5173 → open http://localhost:5173/admin/
just test-ui         # Vitest
just e2e             # Playwright against the demo app (npx playwright install chromium once)
just admin-build     # rebuild crates/rustcms-admin/assets — commit the result
```

In development, Vite copies the runtime config (including the application's
extensions) from the Rust server's `/admin/` page, and proxies `/admin/api` and
`/admin/assets/ext` to it: start the server first.

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

## Scaffolding an app against a local checkout

`rustcms new blog --rustcms-path ../RustCMS` (hidden from the public CLI
reference) writes path dependencies on the checkout instead of the release Git tag.
`--in-workspace` skips the standalone `[workspace]` table for apps inside
another workspace (as `examples/demo`).
