# Directory structure

A RustCMS application is one Cargo package laid out like a Laravel
application: the same folder names, inside `src/` (Rust modules live there).
This page shows what `rustcms new` creates, what generators add, and where your
code goes.

## A new application

`rustcms new blog` creates:

```text
blog/
├── Cargo.toml, Cargo.lock       # dependencies (≈ composer.json / composer.lock)
├── .env, .env.example           # secrets and per-machine values (.env is git-ignored)
├── .gitignore                   # target, .env, *.sqlite, /storage
├── .github/workflows/ci.yml     # CI: formatting, lints and the tests on SQLite
├── README.md                    # the commands and this layout, for your team
├── config/                      # app.toml, auth.toml, http.toml, log.toml
├── lang/vi.toml                 # translations of your admin labels and messages
├── resources/README.md          # what goes in resources/ (empty at first)
├── src/
│   ├── main.rs                  # the console (≈ artisan): serve, migrate, db:seed, your commands
│   ├── lib.rs                   # declares the modules below
│   ├── bootstrap/
│   │   ├── mod.rs               # build(): the application (≈ bootstrap/app.php)
│   │   └── providers.rs         # its providers, in order (≈ bootstrap/providers.php)
│   ├── providers/               # what the application registers (≈ app/Providers)
│   │   ├── auth_service_provider.rs   # permissions
│   │   ├── admin_panel_provider.rs    # admin resources, pages, extensions, translations
│   │   └── app_service_provider.rs    # jobs, seeders, console commands
│   ├── routes/
│   │   └── api.rs               # every API route and its access (≈ routes/api.php)
│   ├── http/                    # (≈ app/Http)
│   │   └── controllers/         #   handlers                       (make:api)
│   ├── models/                  # SeaORM entities and query scopes (make:model)
│   ├── policies/                # who may do what                  (make:policy)
│   ├── admin/                   # the admin panel (≈ app/Filament)
│   │   ├── resources/           #   one resource per model         (make:resource)
│   │   └── pages/               #   custom and settings pages      (make:page, make:settings)
│   └── database/
│       ├── migrations/          # your migrations, after the framework's (make:migration)
│       └── seeders/             # DatabaseSeeder and the others     (make:seeder)
└── tests/
    ├── support/mod.rs           # app(db): the application on a test database
    └── feature/                 # tests through HTTP, one binary    (make:test)
        ├── main.rs              #   lists the test modules
        └── health.rs            #   /health, the API ping, admin sign-in, seeding
```

Each folder of `src/` has a `mod.rs`. Two more appear on the first run, both
git-ignored: `target/` (the build) and `database.sqlite` (created by
`rustcms migrate` with the default `DATABASE_URL`).

Compared with Laravel: there is no `public/` or `vendor/` (the admin panel
is built into the binary, dependencies are Cargo's), `resources/` holds what
the panel embeds (page templates, extension scripts and styles, logo), and
module names are snake_case (`http/controllers`, not `Http/Controllers`).

## Folders added later

More folders appear when a generator first needs them, or when you add them:

| Folder | Holds | Generator |
|---|---|---|
| `src/services/` | business rules shared by the panel, the API, jobs and seeders | `make:service` |
| `src/jobs/` | background jobs ([Queues](../digging-deeper/queues.md)) | `make:job` |
| `src/settings/` | settings stored in the database ([Settings](../admin/settings.md)) | `make:settings` |
| `src/console/` | console commands ([Console](../basics/console.md)) | `make:command` |
| `src/http/resources/` | records as JSON (≈ API resources) | `make:api` |
| `src/http/requests/` | validated input (≈ form requests) | by hand |
| `src/routes/admin.rs` | JSON routes of your panel pages, on the admin session ([Admin routes](../admin/admin-routes.md)) | by hand |
| `src/admin/slots/` | content beside records ([Customizing](../admin/customizing.md)) | by hand |
| `src/database/factories/` | model factories for seeders and tests | `make:factory` |
| `resources/views/admin/` | templates of custom pages and slots ([Customizing](../admin/customizing.md)) | `make:page` |
| `resources/admin/js/`, `css/` | extension scripts and stylesheets, logo and favicon | `make:widget`, `make:page --script` |
| `resources/admin/src/`, `sdk/` | React pages and the admin SDK they build with ([React pages](../admin/react-pages.md)) | `make:page --react`, `make:sdk` |
| `storage/app/` | uploaded files on the `local` disk, once `StorageProvider` is registered ([File storage](../digging-deeper/storage.md)) | created at start |

A module you add by hand needs its `pub mod` line, as in any Rust crate
(`pub mod admin;` in `src/routes/mod.rs` for `src/routes/admin.rs`).

## Assembly

`src/main.rs` is the whole application (HTTP server, console, embedded admin
panel). It hands the console your `build` function and your migrations. The
demo's (in your application, `demo` is your crate name):

<<< @/../../../examples/demo/src/main.rs#main [src/main.rs (demo)]

`src/bootstrap/mod.rs` turns the configuration into an `AppBuilder`: the
providers in `providers.rs`, then the API routes. A new application's is:

<<< @/../../../examples/lms/src/bootstrap/mod.rs#build [src/bootstrap/mod.rs]

An application that has [admin routes](../admin/admin-routes.md) or
[schedules](../digging-deeper/scheduling.md) adds them here too, as the demo
does:

<<< @/../../../examples/demo/src/bootstrap/mod.rs#build [src/bootstrap/mod.rs (demo)]

The providers do the rest. A new application's admin panel provider holds the
anchors generators write at:

```rust
// src/providers/admin_panel_provider.rs
pub fn provider() -> AdminProvider {
    AdminProvider::new()
        .brand("blog")
        // Labels in Vietnamese (`rustcms lang:missing vi` lists what is missing).
        .translations("vi", include_str!("../../lang/vi.toml"))
        // rustcms:resources
        // rustcms:pages
        // rustcms:extensions
}
```

See [Service providers](../architecture/providers.md) for every provider and
what it registers.

## Generators

Generators write their files and register them at `// rustcms:…` anchor
comments. Keep the anchors: a generator that cannot find its anchor stops
with an error. (`make:widget` only writes the script and prints the line to
add.)

| Anchor | In | Used by |
|---|---|---|
| `rustcms:mods` | `src/lib.rs` | folders created on demand (`services`, `jobs`, …) |
| `rustcms:models`, `rustcms:policies` | `src/models/mod.rs`, `src/policies/mod.rs` | `make:model`, `make:policy` |
| `rustcms:resources`, `rustcms:pages`, `rustcms:extensions` | `src/providers/admin_panel_provider.rs` | `make:resource`; `make:page`, `make:settings`; `make:page --script`, `make:page --react` |
| `rustcms:resource-mods`, `rustcms:page-mods` | `src/admin/resources/mod.rs`, `src/admin/pages/mod.rs` | `make:resource`; `make:page`, `make:settings` |
| `rustcms:seeders`, `rustcms:jobs`, `rustcms:commands` | `src/providers/app_service_provider.rs` | `make:seeder`, `make:job`, `make:command` |
| `rustcms:api-routes` | `src/routes/api.rs` | `make:api` |
| `rustcms:http-mods`, `rustcms:controller-mods` | `src/http/mod.rs`, `src/http/controllers/mod.rs` | `make:api` |
| `rustcms:database-mods` | `src/database/mod.rs` | `make:factory` |
| `rustcms:migration-mods`, `rustcms:migrations` | `src/database/migrations/mod.rs` | `make:migration`, `make:model -m` |
| `rustcms:seeder-mods`, `rustcms:seeder-calls` | `src/database/seeders/mod.rs` | `make:seeder` |
| `rustcms:tests` | `tests/feature/main.rs` | `make:test` |

Folders created on demand get their own anchors (`rustcms:services`,
`rustcms:jobs`, `rustcms:settings`, `rustcms:factories`, …) in their `mod.rs`.

Rules every generator follows:

- It checks the files and anchors it needs before writing its own files, and
  stops if one is missing or if a file it would create already exists. A
  generator may still have prepared a module folder (its `mod.rs` and the
  `pub mod` line), or for `make:page --react` the npm setup in
  `resources/admin`, before a later check fails: look at `git status` after
  an error.
- A folder created on demand gets its `mod.rs` and a `pub mod` line in its
  parent; a flat `src/services.rs` next to it, or a folder its parent does not
  declare, is refused.
- Names are checked: a Rust keyword (`make:command type`), a command name of
  `rustcms` itself (`make:command doctor`), a second `DatabaseSeeder`, a
  settings group that is not 1–32 of `a-z`, `0-9` and `_`
  (`make:settings SchoolSettings` → group `school`), a page slug longer than
  64 characters, or a script page name that is not `vendor/name` (each part
  1–32 of `a-z`, `0-9`, `-`; the vendor comes from the package name) are
  refused.

Every generator and its options: [CLI reference](../reference/cli.md).

Applications created before 0.3 (`src/app.rs`, `src/migrations/`,
`src/http/api/`) are refused until moved: see the
[upgrade guide](../upgrade.md#application-layout-03).

## Where code goes

- **A rule that must hold whoever writes** (a course's seats, a stock level):
  a service function taking the transaction, called by admin hooks, API
  handlers, jobs and seeders alike. See the [LMS example](../examples/lms.md).
- **Who may see or change a record**: a policy, plus the resource's `scope()`.
  Both are enforced on the server for every path (lists, forms, relationships,
  actions, exports); hiding a button is not a permission
  ([Authorization](../admin/authorization.md)).
- **What a query filter means** (published posts, courses someone teaches):
  a function next to the model, used by scopes and services.
- **Values admins change at run time**: a settings group (`src/settings/`)
  with its page (`src/admin/pages/`); values that need a deploy go in
  `config/`.
- **JSON for a page of the panel**: `src/routes/admin.rs` with
  `AdminRoutes`; JSON for other clients: `src/routes/api.rs` with `ApiRoutes`.

## Related pages

- [Service providers & container](../architecture/providers.md)
- [Routing](../basics/routing.md), [Database](../basics/database.md),
  [Testing](../basics/testing.md)
- [Console](../basics/console.md)
