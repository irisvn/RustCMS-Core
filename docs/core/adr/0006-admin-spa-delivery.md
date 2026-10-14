# 0006 — Admin SPA delivery: committed assets, runtime meta, strict CSP

- Status: accepted
- Date: 2026-10-12

## Context

The admin panel is a React SPA (ADR 0002) that must ship inside the application
binary, work under any admin path, and never require Node to *use* the
framework. It runs on the same origin as the admin API, behind a cookie session
with CSRF protection.

## Decision

- `admin-ui/` builds into `crates/rustcms-admin/assets/`, which is committed and
  embedded with `rust-embed` (read from disk in debug builds). There is no
  `build.rs`; CI rebuilds and fails on any difference, and checks the packaged
  crate contains `assets/index.html`.
- The server injects `<base href="<admin_path>/">` and
  `<meta name="rustcms-admin" content="{basePath}">` into `index.html` at
  startup. Asset URLs are relative, so one bundle works under any prefix.
- Strict CSP on panel pages (`script-src 'self'; style-src 'self'`, no inline
  code, `frame-ancestors 'none'`). Components that inject `<style>` tags (Radix
  Dialog/Select/ScrollArea, scroll locks) are not used: native `<dialog>` and
  `<select>` and a small combobox instead. Tailwind classes for schema-driven
  layout are static maps.
- Admin API responses are `Cache-Control: private, no-store`; pages are
  `no-store`; hashed assets are immutable. Unknown `/admin/api/*` paths are JSON
  404 inside the admin stack, never the HTML shell.
- CSRF mismatches have their own code (`csrf_mismatch`): the client drops its
  token, checks the session and asks the user to retry; writes are never
  replayed automatically.
- Development: Vite serves `/admin/` and proxies `/admin/api` to the Rust
  server (instead of Rust proxying to Vite), which keeps an HTTP client out of
  the binary. The browser `Origin` is forwarded, so dev config allows
  `http://localhost:5173` in `auth.admin_origins`.
- TypeScript types of the protocol are generated from the Rust DTOs (`ts-rs`,
  feature `ts`) and checked in CI.

## Consequences

- Contributors changing `admin-ui/` must commit rebuilt assets; users of the
  crate never run Node.
- Third-party UI components must be vetted for CSP compatibility; the E2E suite
  fails on any `securitypolicyviolation`.
- The dev setup differs from production (Vite HTML without injection); the
  bundle falls back to `/admin` when the meta tag is absent.
