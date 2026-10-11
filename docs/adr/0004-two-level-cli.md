# 0004 — Two-level CLI: global generator, in-app console

- Status: accepted
- Date: 2026-10-11

## Context

Laravel has `laravel new` (installer) and `php artisan` (runs inside the app).
Rust apps are compiled: commands that touch the database or the app's own
migrations and providers must run in the app binary.

## Decision

- `rustcms` (crate `rustcms-cli`) creates apps and generates code. It never links
  the application.
- Runtime commands (`serve`, `migrate*`, `key:generate`, later `make:user`, …) live in
  `rustcms_core::Console`, embedded in every app's `main.rs`.
- Any command `rustcms` does not know is forwarded to the app found in the
  current directory or a parent (`[package.metadata.rustcms]`) via `cargo run`.
- Generators edit files only at anchor comments (`// rustcms:models`, …); when an
  anchor is missing they print the line instead.

## Consequences

- `rustcms migrate` compiles the app first; production deployments call the
  binary directly (`./blog migrate`).
- `docs:cli` renders both command sets from their clap definitions; CI checks
  `docs/src/cli.md` is up to date.
