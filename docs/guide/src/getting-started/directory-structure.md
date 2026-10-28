# Directory structure

A rustcms application is one Cargo package laid out like a Laravel
application: the same folder names, inside `src/` (Rust modules live there).
`rustcms new blog` creates:

```text
blog/
├── Cargo.toml / Cargo.lock      # dependencies (≈ composer.json / composer.lock)
├── .env, .env.example           # secrets and per-machine values (.env is git-ignored)
├── .github/workflows/ci.yml     # CI: formatting, lints and the tests on SQLite
├── config/                      # app.toml, auth.toml, http.toml, log.toml
├── lang/vi.toml                 # translations of your admin labels and messages
├── resources/                   # views/admin (page templates), admin/{js,css} (extensions), logo
├── storage/                     # uploaded files (git-ignored)
├── src/
│   ├── main.rs                  # the console (≈ artisan): serve, migrate, db:seed, your commands
│   ├── lib.rs                   # declares the modules below
│   ├── bootstrap/               # build(): the application (≈ bootstrap/app.php)
│   │   └── providers.rs         #   its providers, in order (≈ bootstrap/providers.php)
│   ├── providers/               # what the application registers (≈ app/Providers)
│   │   ├── auth_service_provider.rs   # permissions
│   │   ├── admin_panel_provider.rs    # admin resources, pages, translations
│   │   └── app_service_provider.rs    # jobs, seeders, console commands
│   ├── routes/api.rs            # every API route and its access (≈ routes/api.php)
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
    ├── support/mod.rs           # helpers shared by the suites
    └── feature/                 # tests through HTTP, one binary    (make:test)
```

More folders appear when a generator first needs them, or when you add them:

| Folder | Holds | Generator |
|---|---|---|
| `src/services/` | business rules shared by the panel, the API, jobs and seeders | `make:service` |
| `src/jobs/` | background jobs ([Queues](../digging-deeper/queues.md)) | `make:job` |
| `src/settings/` | settings stored in the database ([Settings](../admin/settings.md)) | `make:settings` |
| `src/console/` | console commands | `make:command` |
| `src/http/resources/` | records as JSON (≈ API resources) | `make:api` |
| `src/http/requests/` | validated input (≈ form requests) | by hand |
| `src/admin/slots/` | content beside records ([Customizing](../admin/customizing.md)) | by hand |
| `src/database/factories/` | model factories for seeders and tests | `make:factory` |
| `resources/views/admin/` | templates of custom pages and slots ([Customizing](../admin/customizing.md)) | `make:page` |
| `resources/admin/` | extension scripts (`js/`), stylesheets (`css/`), logo and favicon | `make:widget`, `make:page --script` |

Compared with Laravel: there is no `public/` or `vendor/` (the admin panel
is built into the binary, dependencies are Cargo's), `resources/` holds what
the panel embeds (page templates, extension scripts and styles, logo), and
module names are snake_case (`http/controllers`, not `Http/Controllers`).

## Assembly

`src/bootstrap/mod.rs` turns the configuration into an `AppBuilder`; the
providers do the rest:

```rust
pub fn build(config: Config) -> AppBuilder {
    let mut app = AppBuilder::new(config);
    let api = crate::routes::api::routes();
    providers::register(&mut app, &api);
    api.register(&mut app);
    app
}
```

```rust
// src/providers/admin_panel_provider.rs
pub fn provider() -> AdminProvider {
    AdminProvider::new()
        .brand("blog")
        .translations("vi", include_str!("../../lang/vi.toml"))
        .resource::<crate::admin::resources::post_resource::PostResource>()
        // rustcms:resources
        // rustcms:pages
}
```

`src/main.rs` is the whole application (HTTP server, console, embedded admin
panel):

```rust
fn main() -> std::process::ExitCode {
    rustcms::Console::<blog::database::migrations::Migrator>::new(blog::bootstrap::build).run_blocking()
}
```

## Generators

Generators write one file and register it at a `// rustcms:…` anchor comment
(`rustcms:resources`, `rustcms:jobs`, `rustcms:seeders`, `rustcms:mods`, …).
They check every file and anchor first and write nothing if one is missing,
or if the file already exists. A folder created on demand gets its `mod.rs`
and a `pub mod` line in its parent; a flat `src/services.rs` next to it, or a
folder its parent does not declare, is refused. Names that would make a Rust
keyword (`make:command type`), a second `DatabaseSeeder` or a settings group
longer than 32 characters are refused too.

Applications created before 0.3 (`src/app.rs`, `src/migrations/`,
`src/http/api/`) are refused until moved: see the
[upgrade guide](../upgrade.md#application-layout-03).

## Where code goes

- **A rule that must hold whoever writes** (a course's seats, a stock level):
  a service function taking the transaction, called by admin hooks, API
  handlers, jobs and seeders alike. See the [LMS example](../examples/lms.md).
- **Who may see or change a record**: a policy, plus the resource's `scope()`.
- **What a query filter means** (published posts, courses someone teaches):
  a function next to the model, used by scopes and services.
- **Values admins change at run time**: a settings group (`src/settings/`)
  with its page (`src/admin/pages/`); values that need a deploy go in
  `config/`.

See also [Routing](../basics/routing.md), [Database](../basics/database.md)
and [Testing](../basics/testing.md).
