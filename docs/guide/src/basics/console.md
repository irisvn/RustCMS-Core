# Console

The application's binary is its console (Laravel's `artisan`). In an
application directory, `rustcms <command>` runs it for you (`cargo run --
<command>`); in production, run the binary itself: `./blog migrate`.

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
                                     # src/admin/post.rs: fields, form, table and filters from the model
rustcms make:policy PostPolicy       # src/policies/post_policy.rs
rustcms make:api Post                # src/http/api/posts/: list and show
rustcms make:widget acme/color       # assets/admin/acme/color.js: a custom field widget
rustcms make:test PostAdminTest      # tests/post_admin_test.rs
```

`make:resource --generate` maps column types to inputs (text, textarea for
`body`/`content`/…, number, toggle, date, date-time), adds table columns and
filters, and `.soft_deletes()` for a nullable `deleted_at`. `*_id` columns
become relationship selects only with `--belongs-to`; otherwise they get a
text input and a `TODO(rustcms)` comment, with a warning. Secret-looking
columns (`password`, `token`, …) and unsupported types are skipped with a
warning; owner columns (`author_id`, `user_id`, …) are shown but set by the
server. The model must be in `src/models/<snake_name>.rs`.

## Writing commands

```rust,ignore
// Cargo.toml: clap = "4", anyhow = "1"
use clap::{Arg, ArgMatches, Command};
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

// src/app.rs (or a provider's `register`)
app.command(SyncInvoices);
```

Commands run with the database connected and every provider booted. An `Err`
prints the error and exits with status 1.

Commands that must not run in production check the environment:

```rust,ignore
if state.config().app.env == rustcms::config::Environment::Production {
    anyhow::bail!("refusing in production");
}
```
