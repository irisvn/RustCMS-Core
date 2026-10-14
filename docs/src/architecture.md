# Architecture

How rustcms is organised, for contributors and for application developers who
want to know where a behaviour comes from. Decisions and their reasons are in
the [decision records](adr.md).

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
examples/demo       an application, laid out as `rustcms new` generates
```

Dependencies go one way: `rustcms-auth` uses `rustcms-core`, `rustcms-admin`
uses both, and `rustcms` re-exports all three. Lower-level crates never depend
on the admin engine, so it can change without touching them.

| Laravel / Filament | rustcms |
|---|---|
| `Illuminate\Foundation`, service providers, container | `rustcms_core::app` (`AppBuilder`, `ServiceProvider`, `Inject`) |
| `Illuminate\Http`, middleware | `rustcms_core::http` |
| `Illuminate\Database`, migrations | `rustcms_core::db`, `rustcms-migration` |
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
| `engine/` | the generic engine: `Registry`/`DynResource`, `Ctx`, wire format, idempotency |
| `engine/adapter/` | `ResourceAdapter<R>`: `input` (acceptance), `references` (checks after hooks), `read`, `write` (transactions, locks), `schema`, `operations` (the `DynResource` impl) |
| `http/` | routes and handlers: `resources`, `relations`, `actions`, `extract` (`AdminCtx`) |
| `contract/` | the DTOs the SPA receives (`ResourceSchema`, …); also reachable as `schema` |
| `features/` | built-in features: `media/`, `rich_text/`, `system/` (users and roles) |

### rustcms-auth

| Module | Role |
|---|---|
| `guards/` | `guard` (`Auth`, `MaybeAuth`), `session`, `tokens`, `credentials` (revocation) |
| `rbac/` | `service` (`RbacService`, every change), `rules` (no escalation), `resolver` (grants cache), `queries` |
| `http/` | the auth routes |
| `console/` | `user:create`, `role:assign`, … |
| `support/` | password hashing, secret comparison, rate limits |
| `entities/` | the auth tables |

### rustcms-core

Small modules stay at the top (`app`, `config`, `console`, `error`, `http`,
`key`, `principal`, `telemetry`, `models`). Two are directories:

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

## Applications

`rustcms new` lays an application out like Laravel's `app/`:

| Laravel | rustcms application |
|---|---|
| `app/Filament/Resources/PostResource.php` | `src/admin/post.rs` (`make:resource`) |
| `app/Models` | `src/models` (`make:model`) |
| `app/Policies` | `src/policies` (`make:policy`) |
| `database/migrations` | `src/migrations` (`make:migration`) |
| `config/*.php` | `config/*.toml` |
| `tests/` | `tests/` (`make:test`) |
| Filament assets | `assets/admin/<vendor>/` (`make:widget`) |
