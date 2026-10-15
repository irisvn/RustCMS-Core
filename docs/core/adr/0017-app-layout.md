# 0017 — Application layout

- Status: accepted
- Date: 2026-10-20

## Context

Generated applications put everything that was not a model, resource,
policy or migration next to `main.rs` (`app.rs`, `settings.rs`, `seed.rs`,
`jobs.rs`). The LMS example showed the cost: a 200-line seed command that
bypassed the business rules, a settings struct mixing data and admin page,
rules living inside admin hooks where seeders and APIs could not reach them.
Users come from Laravel and Filament and expect their layout.

## Decision

Laravel's folder names, inside `src/` (option "B"; option "A", a crate
rooted at `app/` with `#[path]` modules for `database/`, `routes/` and
`bootstrap/`, was rejected as unidiomatic for Rust tooling):

- `bootstrap/` (`build`, `providers.rs`), `providers/` (auth, admin panel,
  app service providers), `routes/api.rs`, `http/{controllers,requests,resources}`,
  `models/` (entities and query scopes), `services/` (rules shared by every
  writer; name chosen over "actions"), `policies/` (+ `permissions.rs`),
  `jobs/`, `settings/` (data only), `console/`, `admin/{resources,pages}`
  (≈ `app/Filament`), `database/{migrations,seeders,factories}`;
  `config/`, `lang/`, `storage/`, `tests/{support,feature}` at the root.
- Folders other than the minimal set appear when a `make:*` generator first
  needs them (it creates `mod.rs` and the `pub mod` line in the parent).
- Generators check every destination and anchor before writing; applications
  in the old layout are refused with a pointer to the upgrade guide (pre-release:
  no dual support).
- Framework support: `rustcms::database::{Seeder, Factory, call}`,
  `AppBuilder::seeder`, `db:seed [--class] [--force]`,
  `migrate:fresh --seed`; `Factory` moved from `rustcms-testing` (re-exported
  there); `rustcms::clap` re-exported for commands.
- Settings: data (`impl Settings`) in `settings/`, page (`impl SettingsPage`
  for the same struct) in `admin/pages/`; no separate page type.
- Feature tests: one binary, `tests/feature/main.rs` listing modules,
  `tests/support/mod.rs` included with `#[path]`.

## Consequences

- Demo and LMS examples follow the layout; migration file names are kept
  (they are stored in `seaql_migrations`, checked by a test).
- A seeder that creates public-password accounts refuses production itself
  (`--force` is not enough).
- Seeders have no shared transaction: they must complete on a rerun and
  write through services.
