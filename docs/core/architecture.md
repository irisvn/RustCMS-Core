# Architecture

How rustcms is organised, for contributors and for application developers who
want to know where a behaviour comes from. Decisions and their reasons are in
the [decision records](adr/).

## Crates

```text
rustcms             the crate applications depend on: re-exports, prelude
 ├─ rustcms-core    application, config, HTTP stack, database, console, storage, errors
 ├─ rustcms-auth    guards, sessions, tokens, roles and permissions      (uses core)
 ├─ rustcms-admin   the admin engine (resources → JSON schema) + the SPA  (uses core, auth)
 └─ rustcms-migration  the framework's migrations and schema helpers
rustcms-testing     TestApp, TestDb, db_test!, factories
rustcms-cli         `rustcms new`, `make:*`
admin-ui/           the React SPA, built into rustcms-admin/assets
examples/demo       a blog, laid out as `rustcms new` generates (ADR 0017)
examples/lms        a small LMS back office (seats under a lock, instructor scopes, a job)
```

Dependencies go one way: `rustcms-auth` uses `rustcms-core`, `rustcms-admin`
uses both, and `rustcms` re-exports all three. Lower-level crates never depend
on the admin engine, so it can change without touching them.

| Laravel / Filament | rustcms |
|---|---|
| `Illuminate\Foundation`, service providers, container | `rustcms_core::app` (`AppBuilder`, `ServiceProvider`, `Inject`) |
| `Illuminate\Http`, middleware | `rustcms_core::http` |
| `Illuminate\Database`, migrations, seeders, factories | `rustcms_core::db`, `rustcms_core::database`, `rustcms-migration` |
| `Illuminate\Console`, artisan | `rustcms_core::console`, `AppCommand`, `rustcms-cli` |
| `Illuminate\Filesystem` | `rustcms_core::storage` |
| `Illuminate\Auth`, gates, policies | `rustcms-auth` |
| Filament panel | `rustcms_admin::panel` (`AdminProvider`) |
| Filament resources | `rustcms_admin::resources` |
| `filament/forms`, `filament/tables` | `rustcms_admin::forms`, `rustcms_admin::tables` |
| Livewire rendering | `rustcms_admin::contract` (JSON) + `admin-ui` |

## Modules by role

### rustcms-admin

| Module | Role |
|---|---|
| `resources/` | what applications declare: `Resource`, `Field`, `Action`, `Policy`, list `query` types |
| `forms/`, `tables/` | builders (`TextInput::make`, `Table::new`, …); `ui` re-exports both |
| `panel/` | `AdminProvider` and the SPA shell (assets, extensions) |
| `engine/` | the generic engine: `Registry`/`DynResource`, `Ctx`, wire format, idempotency, `validate` (value checks shared by resources and settings) |
| `engine/adapter/` | `ResourceAdapter<R>`: `input` (acceptance), `references` (checks after hooks), `read`, `write` (transactions, row and parent locks, ADR 0018), `schema`, `operations` (the `DynResource` impl), `audit` (snapshots, redaction, entries) |
| `http/` | routes and handlers: `resources`, `relations`, `actions`, `extract` (`AdminCtx`) |
| `contract/` | the DTOs the SPA receives (`ResourceSchema`, …); also reachable as `schema` |
| `features/` | built-in features: `audit` (log viewer, record history), `queue` (System → Queue page, M15d), `media/`, `rich_text/`, `settings/` (settings pages; ADR 0015), `system/` (users and roles) |

### rustcms-auth

| Module | Role |
|---|---|
| `guards/` | `guard` (`Auth`, `MaybeAuth`), `session`, `tokens`, `jwt` (access tokens), `jwt_session` (refresh families), `credentials` (revocation) |
| `rbac/` | `service` (`RbacService`, every change; `begin_as` for writes outside RBAC under its lock), `rules` (no escalation), `resolver` (grants cache keyed by the database generation), `queries` |
| `http/` | the auth routes |
| `console/` | `user:create`, `role:assign`, … |
| `support/` | password hashing, secret comparison, rate limits, `audit` (sign-in events) |
| `entities/` | the auth tables |

### rustcms-core

Small modules stay at the top (`app`, `config`, `console`, `error`, `http`,
`key`, `principal`, `telemetry`, `models`, `i18n` (locale, catalogs, issue codes), `audit` (entries, diff, bounds, prune; ADR 0014), `settings` (groups, `Secret`, encrypted storage; ADR 0015), `queue/` (jobs, workers, commands, `manage`: statuses and single-job actions; ADR 0016), `jobs` (periodic jobs run by one
instance), `redis` with feature `redis`). Two are directories:

- `db/`: `connect`, `check` (MySQL setup), `transactions`, `locks` (named locks).
- `storage/`: the `Storage` trait, `local`, `s3`.

Grouping modules does not move public items: the paths that existed before
(`rustcms_admin::media`, `rustcms_admin::schema`, `rustcms_auth::session`,
`rustcms_core::db::begin_write`, …) are re-exports, checked by `tests/paths.rs`
in each crate.

## Patterns

| Pattern | Where |
|---|---|
| Service provider + container | `AppBuilder::provider`, `bind`, `Inject` |
| Declarative resource + builders | `Resource`, `Field::column`, `TextInput::make`, `Form`, `Table` |
| Type erasure + adapter | `ResourceAdapter<R>` implements the object-safe `DynResource`, so a `Registry` holds resources of any entity |
| Registry checked at boot | `Registry::finish` panics on misconfiguration, never per request |
| Schema-driven UI | the server sends `ResourceSchema`; the SPA only renders it |
| Policy and strategy | `Policy`, `Storage` (local, S3), `KeyStrategy` |
| Guards as extractors | `Auth`, `MaybeAuth`, `AdminCtx` |
| Active record | SeaORM entities (no repository layer) |
| Lifecycle hooks | `mutate_before_create`, `after_force_delete`, … in the write transaction |
| Intent / receipt | idempotency receipts, media upload intents and deletion queue |
| Audit in the write's transaction | `audit::record(txn, …)` from the engine, `RbacService`, credentials |

## Applications

`rustcms new` lays an application out like Laravel, inside `src/` (ADR 0017;
guide: Directory structure):

| Laravel | rustcms application |
|---|---|
| `bootstrap/app.php`, `bootstrap/providers.php` | `src/bootstrap/mod.rs` (`build`), `src/bootstrap/providers.rs` |
| `app/Providers` | `src/providers/` (auth, admin panel, app service providers) |
| `app/Filament/Resources/PostResource.php`, `app/Filament/Pages` | `src/admin/resources/post_resource.rs` (`make:resource`), `src/admin/pages/` |
| `app/Models` (+ local scopes) | `src/models` (`make:model`) |
| `app/Services` | `src/services` (`make:service`) |
| `app/Policies` | `src/policies` (`make:policy`) |
| `app/Jobs` | `src/jobs` (`make:job`) |
| `app/Console/Commands` | `src/console` (`make:command`) |
| spatie `app/Settings` | `src/settings` (`make:settings`, with its page) |
| `routes/api.php` | `src/routes/api.rs` |
| `app/Http/{Controllers,Requests,Resources}` | `src/http/{controllers,requests,resources}` (`make:api`) |
| `database/{migrations,seeders,factories}` | `src/database/…` (`make:migration`, `make:seeder`, `make:factory`) |
| `config/*.php` | `config/*.toml` |
| `tests/Feature` | `tests/feature/` (one binary, `make:test`), `tests/support/` |
| Filament assets | `assets/admin/<vendor>/` (`make:widget`) |

The CLI's `layout.rs` holds these paths and the module plumbing
(`ensure_module`: `mod.rs` + `pub mod` in the parent); every generator
preflights files and anchors before writing.
