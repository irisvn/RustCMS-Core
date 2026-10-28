# Console

The application's binary is its console (Laravel's `artisan`). In an
application directory, `rustcms <command>` runs it for you
(`cargo run -- <command>`); in production, run the binary itself: `./blog migrate`.

```sh
cargo run -- --help     # every command of the application, the framework's and yours
rustcms --help          # the commands of the `rustcms` tool itself (new, make:*)
rustcms route:list      # every route: method, path and where it comes from
```

The full list is in the [CLI reference](../reference/cli.md).

## Generators

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
rustcms make:widget acme/color       # resources/admin/js/acme/color.js: a custom field widget
rustcms make:test PostAdminTest      # tests/feature/post_admin_test.rs, added to tests/feature/main.rs
```

`make:page --script` names the script `<package>/<slug>` and refuses a name
the panel would refuse at boot (each part at most 32 lowercase letters,
digits or `-`). Each generator checks its target files and anchors first and writes nothing
when one is missing or the file exists ([Directory structure](../getting-started/directory-structure.md)).

`make:resource --generate` maps column types to inputs (text, textarea for
`body`/`content`/…, number, toggle, date, date-time), adds table columns and
filters, and `.soft_deletes()` for a nullable `deleted_at`. `*_id` columns
become relationship selects only with `--belongs-to`; otherwise they get a
text input and a `TODO(rustcms)` comment, with a warning. Secret-looking
columns (`password`, `token`, …) and unsupported types are skipped with a
warning; owner columns (`author_id`, `user_id`, …) are shown but set by the
server. The model must be in `src/models/<snake_name>.rs`.

## Writing commands

```rust
// Cargo.toml: anyhow = "1" (clap comes as `rustcms::clap`)
use rustcms::clap::{Arg, ArgMatches, Command};
use rustcms::prelude::*;
use rustcms::{AppCommand, AppState, async_trait};

pub struct SyncInvoices;

#[async_trait]
impl AppCommand for SyncInvoices {
    fn definition(&self) -> Command {
        Command::new("invoices:sync")
            .about("Fetch new invoices from Stripe")
            .arg(Arg::new("days").long("days").default_value("7"))
    }

    async fn run(&self, state: &AppState, args: &ArgMatches) -> anyhow::Result<()> {
        let days: u32 = args.get_one::<String>("days").expect("default").parse()?;
        // state.db(), state.service::<Stripe>(), …
        println!("synced {days} days");
        Ok(())
    }
}

// src/providers/app_service_provider.rs (`rustcms make:command invoices:sync` writes both)
app.command(SyncInvoices);
```

Commands run with the database connected and every provider booted. An `Err`
prints the error and exits with status 1.

Commands that must not run in production check the environment:

```rust
if state.config().app.env == rustcms::config::Environment::Production {
    anyhow::bail!("refusing in production");
}
```
