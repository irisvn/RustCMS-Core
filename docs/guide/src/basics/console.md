# Console

Your application's binary is also its console, like Laravel's `artisan`: the
same executable serves HTTP, runs migrations and seeders, works the queue and
runs your own commands. Use the `rustcms` tool while developing and the binary
itself in production.

## Quick start

```sh
rustcms --help          # the rustcms tool's own commands (new, make:*, doctor, …)
cargo run -- --help     # the application binary's built-in commands (serve, migrate, …)
rustcms migrate         # in an application directory: runs `cargo run -- migrate`
rustcms route:list      # every route: method, path and where it comes from
./blog migrate          # in production: the built binary, no cargo
```

Inside an application (any directory below the one whose `Cargo.toml` has
`[package.metadata.rustcms]`), `rustcms <command>` forwards every command it
does not know itself to the application:
`cargo run --quiet --package <app> --bin <app> -- <command>`, run from the
application's root. Outside one, it fails with "not inside a rustcms
application".

Every command and flag is listed in the [CLI reference](../reference/cli.md),
which is generated from the code.

## The entry point

`src/main.rs` hands the command line to `rustcms::Console`, with the
application's migrator and its `build` function:

<<< @/../../../examples/demo/src/main.rs#main [src/main.rs]

| Method | Does |
|---|---|
| `Console::<M>::new(build)` | `M`: the migrator (`database::migrations::Migrator`); `build`: `fn(Config) -> AppBuilder` |
| `.run_blocking()` | parses `std::env::args`, runs the command on a new multi-threaded Tokio runtime, returns the `ExitCode` |
| `.run().await` | the same, on a runtime you already have |

`rustcms::console::command()` returns the clap definition of the built-in
commands (the CLI reference is generated from it).

## Commands

| Group | Commands | Added by |
|---|---|---|
| Server | `serve [--listen <addr>]` | the console |
| Database | `migrate`, `migrate:status`, `migrate:rollback`, `migrate:refresh`, `migrate:fresh`, `db:seed` ([Database](database.md#commands)) | the console |
| Routes | `route:list` | the console |
| Key | `key:generate [--show] [--force]` | the console |
| Queue | `queue:work`, `queue:failed`, `queue:retry`, `queue:forget` ([Queues](../digging-deeper/queues.md)) | every app builder |
| Encryption | `key:reencrypt` ([Rotating APP_KEY](../operations/app-key.md)) | every app builder |
| Schedules | `schedule:list`, `schedule:run` ([Scheduling](../digging-deeper/scheduling.md)) | every app builder |
| Accounts | `make:user`, `role:assign`, `token:create`, `permission:sync`, `session:prune`, `audit:prune` | `AuthProvider` |
| Admin panel | `idempotency:prune`, `media:prune`, `lang:missing`, `admin:branding` | `AdminProvider` |
| Yours | any `AppCommand` | `app.command(…)` |

How each kind of command starts:

| Command | Loads `Config` | Connects to the database | Boots providers |
|---|---|---|---|
| `key:generate` | no | no | no |
| `route:list` | yes | no | no (providers register only) |
| `migrate`, `migrate:rollback`, `migrate:refresh`, `migrate:fresh` | yes | yes, one connection | no |
| `migrate:status` | yes | yes, the pool | no |
| `serve`, `db:seed`, `migrate:fresh --seed`, `queue:*`, `schedule:*`, `key:reencrypt`, provider and application commands | yes | yes, the pool | yes |

- `key:generate` writes `APP_KEY` into `.env` in the current directory,
  keeping every other line. It replaces every form dotenv accepts
  (`APP_KEY=`, `APP_KEY = …`, `export APP_KEY=`, quoted values), so no stale
  line can shadow the new key. It refuses to replace a key that is set
  without `--force` (rotating the key signs every admin out:
  [Rotating APP_KEY](../operations/app-key.md)). `--show` prints a key
  without writing anything.
- `route:list` prints `METHOD`, `PATH` and `SOURCE` columns, sorted by path,
  from what the framework, the providers and the application described
  (`AppBuilder::describe_routes`). Plain Axum routes you add without
  describing them are missing ([Routing](routing.md#plain-axum-routes)).
- Booting runs every provider's `boot`. `AuthProvider::boot` starts the hourly
  clean-up of sessions, JWT sign-ins, reset links and old audit entries in
  every process that boots, a console command included; its first run is one
  hour after start, so a short command exits before it
  ([Authentication](../security/authentication.md#admin-sessions)).

## Generators

The `rustcms` tool writes code into an application. Each generator checks its
target files and the anchors it edits first, and writes nothing when one is
missing or a file already exists
([Directory structure](../getting-started/directory-structure.md)).
`make:sdk` is the exception: it replaces the SDK files you have not changed
and keeps (and lists) the ones you edited.

```sh
rustcms make:model Post -m           # src/models/post.rs + a create_posts_table migration
rustcms make:migration add_slug_to_posts
rustcms make:resource Post --generate --belongs-to category_id=categories
                                     # src/admin/resources/post_resource.rs: fields, form, table and filters
rustcms make:policy PostPolicy       # src/policies/post_policy.rs
rustcms make:api Post                # src/http/controllers/post_controller.rs + JSON resource + routes
rustcms make:service EnrollmentService  # src/services/enrollment_service.rs: shared business rules
rustcms make:job SendWelcome         # src/jobs/send_welcome.rs, registered in app_service_provider.rs
rustcms make:seeder CourseSeeder     # src/database/seeders/course_seeder.rs, registered and called
rustcms make:factory Course          # src/database/factories/course_factory.rs
rustcms make:settings SchoolSettings # src/settings/school_settings.rs + src/admin/pages/school_settings_page.rs
rustcms make:command lms:report      # src/console/lms_report.rs, registered in app_service_provider.rs
rustcms make:page ReportPage         # src/admin/pages/report_page.rs + resources/views/admin/report.html
rustcms make:page ChartPage --script # … drawn by resources/admin/js/<app>/chart.js instead
rustcms make:page StatsPage --react  # … drawn by a React component in resources/admin/src (built with npm)
rustcms make:sdk                     # installs or updates the admin SDK in resources/admin/sdk
rustcms make:widget acme/color       # resources/admin/js/acme/color.js: a custom field widget
rustcms make:test PostAdminTest      # tests/feature/post_admin_test.rs, added to tests/feature/main.rs
```

`make:page --script` names the script `<package>/<slug>` and refuses a name
the panel would refuse at boot (each part at most 32 lowercase letters,
digits or `-`). React pages are described in
[React pages](../admin/react-pages.md).

`make:resource --generate` maps column types to inputs (text, textarea for
`body`/`content`/…, number, toggle, date, date-time), adds table columns and
filters, and `.soft_deletes()` for a nullable `deleted_at`. `*_id` columns
become relationship selects only with `--belongs-to`; otherwise they get a
text input and a `TODO(rustcms)` comment, with a warning. Secret-looking
columns (`password`, `token`, …) and unsupported types are skipped with a
warning. Owner columns (`author_id`, `user_id`, `owner_id`, `created_by`,
`updated_by`) become `.guarded().readable(…)`: shown, never written by
clients. Nothing fills them for you: set them in the resource's
`mutate_before_create`. A column you map with `--belongs-to` is a
relationship select instead, without that protection. The model must be in
`src/models/<snake_name>.rs`.

The tool's other commands: `new <name> [--path] [--in-workspace] [--no-fetch]`
creates an application, `doctor` checks that the machine can build one,
`upgrade [--dry-run] [--force]` moves an application to the tool's RustCMS
version, `self-update [--version]` replaces the tool, `docs [--print-path]`
opens this guide, and `docs:cli` prints the CLI reference.

## Writing commands

A command implements `rustcms::AppCommand`: a clap definition, whose name is
the command's name, and an async `run` that receives the booted application's
state and the parsed arguments.

```rust
#[async_trait]
pub trait AppCommand: Send + Sync + 'static {
    fn definition(&self) -> clap::Command;
    async fn run(&self, state: &AppState, args: &clap::ArgMatches) -> anyhow::Result<()>;
}
```

`rustcms make:command users:count` writes `src/console/users_count.rs` with an
empty command named `UsersCount` and registers it in
`src/providers/app_service_provider.rs`. Filled in (a sketch: no example
application has a command of its own):

```rust
// src/console/users_count.rs
use rustcms::clap::{Arg, ArgAction, ArgMatches, Command};
use rustcms::models::user;
use rustcms::{AppCommand, AppState, async_trait};
use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter};

pub struct UsersCount;

#[async_trait]
impl AppCommand for UsersCount {
    fn definition(&self) -> Command {
        Command::new("users:count")
            .about("Count user accounts")
            .arg(Arg::new("active").long("active").action(ArgAction::SetTrue).help("Only enabled accounts"))
    }

    async fn run(&self, state: &AppState, args: &ArgMatches) -> anyhow::Result<()> {
        let mut query = user::Entity::find();
        if args.get_flag("active") {
            query = query.filter(user::Column::IsActive.eq(true));
        }
        println!("{}", query.count(state.db()).await?);
        Ok(())
    }
}
```

```rust
// src/providers/app_service_provider.rs, in register (written by make:command)
app.command(crate::console::users_count::UsersCount);
```

```sh
rustcms users:count --active
```

Clap comes re-exported as `rustcms::clap`, so the application needs no clap
dependency of its own; `anyhow` it does need (`anyhow = "1"`, already in a new
application). `AppBuilder::commands()` lists the commands registered so far.

- **Exit status:** `Ok(())` exits with 0. An `Err` prints `error: …` (with its
  causes) and exits with 1. A wrong argument is clap's error, exit status 2;
  `--help` prints the command's help.
- **Listing commands:** `cargo run -- --help` lists only the built-in
  commands; provider and application commands are not in it. `<command> --help`
  shows one command's help, and an unknown command's error names every
  application and provider command.
- **Writes:** go through your services and `RbacService`, as a request
  would; `Actor::System` is the actor for trusted operators
  ([Roles & permissions](../security/authorization.md#changing-access-rbacservice)).

Commands that must not run in production check the environment, as the demo's
seeders do:

<<< @/../../../examples/demo/src/database/seeders/user_seeder.rs#refuse-in-production [src/database/seeders/user_seeder.rs]

## Pitfalls

- **Name clashes.** A command named like a built-in one (`serve`, `migrate`,
  `queue:work`, `make:user`, …) is never reached: the built-in command runs.
  `make:command` refuses only the names of the `rustcms` tool's own commands.
- **Side effects of booting.** Your command boots every provider: a provider
  whose `boot` starts work starts it in your command too.
- **Production.** `db:seed`, `migrate:refresh` and `migrate:fresh` refuse in
  production without `--force`; your own destructive commands must check the
  environment themselves.

## Related pages

- [CLI reference](../reference/cli.md)
- [Database & migrations](database.md)
- [Service providers](../architecture/providers.md)
- [Deployment](../getting-started/deployment.md)
