//! `rustcms`: create applications, generate code, and forward runtime commands
//! (`serve`, `migrate`, …) to the application in the current directory.

mod api;
mod docs;
mod doctor;
mod generate;
mod guide;
mod layout;
mod make;
mod naming;
mod project;
mod react;
mod release;
mod resource;
mod scaffold;
mod self_update;
mod upgrade;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{CommandFactory, Parser, Subcommand};

#[derive(Parser)]
#[command(name = "rustcms", version = concat!(env!("CARGO_PKG_VERSION"), " (", env!("RUSTCMS_TARGET"), ")"), about = "rustcms command line", disable_help_subcommand = true)]
pub(crate) struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create a new application
    New {
        /// Package name (lowercase letters, digits, `-` and `_`)
        name: String,
        /// Parent directory
        #[arg(long, default_value = ".")]
        path: PathBuf,
        /// Use a local rustcms checkout instead of the Git tag (written verbatim, relative to the new app)
        #[arg(long, hide = true)]
        rustcms_path: Option<String>,
        /// Do not declare a standalone `[workspace]` (for apps inside another workspace)
        #[arg(long)]
        in_workspace: bool,
        /// Do not resolve the dependencies (`Cargo.lock` is written on the first build)
        #[arg(long)]
        no_fetch: bool,
    },
    /// Create a SeaORM entity in `src/models`
    #[command(name = "make:model")]
    MakeModel {
        /// Model name in PascalCase, e.g. `BlogPost`
        name: String,
        /// Also create a `create_<table>_table` migration
        #[arg(short = 'm', long)]
        migration: bool,
    },
    /// Create an admin resource in `src/admin/resources` for a model
    #[command(name = "make:resource")]
    MakeResource {
        /// Model name in PascalCase (the model must exist in `src/models`)
        model: String,
        /// Generate fields, form, table and filters from the model's columns
        #[arg(long)]
        generate: bool,
        /// Relationship select for a column: `category_id=categories` (repeatable)
        #[arg(long = "belongs-to", value_name = "FIELD=SLUG")]
        belongs_to: Vec<String>,
    },
    /// Create a policy in `src/policies`
    #[command(name = "make:policy")]
    MakePolicy {
        /// Policy name in PascalCase, e.g. `PostPolicy`
        name: String,
    },
    /// Create a migration in `src/database/migrations`
    #[command(name = "make:migration")]
    MakeMigration {
        /// snake_case name; `create_<table>_table` generates a table skeleton
        name: String,
    },
    /// Create a feature test in `tests/feature`
    #[command(name = "make:test")]
    MakeTest {
        /// Test name in PascalCase, e.g. `PostAdminTest`
        name: String,
    },
    /// Create a JSON API for a model: controller, JSON resource and routes (list and show)
    #[command(name = "make:api")]
    MakeApi {
        /// Model name in PascalCase (the model must exist in `src/models`)
        model: String,
    },
    /// Create a service (business rules) in `src/services`
    #[command(name = "make:service")]
    MakeService {
        /// Name ending in `Service`, e.g. `EnrollmentService`
        name: String,
    },
    /// Create a background job in `src/jobs` and register it
    #[command(name = "make:job")]
    MakeJob {
        /// Job name in PascalCase, e.g. `SendWelcome`
        name: String,
    },
    /// Create a seeder in `src/database/seeders` and register it
    #[command(name = "make:seeder")]
    MakeSeeder {
        /// Name ending in `Seeder`, e.g. `CourseSeeder`
        name: String,
    },
    /// Create a model factory in `src/database/factories`
    #[command(name = "make:factory")]
    MakeFactory {
        /// Model name in PascalCase (the model must exist in `src/models`)
        model: String,
    },
    /// Create a settings group (`src/settings`) and its admin page (`src/admin/pages`)
    #[command(name = "make:settings")]
    MakeSettings {
        /// Name ending in `Settings`, e.g. `SchoolSettings`
        name: String,
    },
    /// Create a console command in `src/console` and register it
    #[command(name = "make:command")]
    MakeConsole {
        /// Command name, e.g. `lms:report`
        name: String,
    },
    /// Create a custom admin page: `src/admin/pages` + a template in `resources/views/admin` (or a script)
    #[command(name = "make:page")]
    MakePage {
        /// Name ending in `Page`, e.g. `CourseOverviewPage`
        name: String,
        /// Draw it with an extension script (`resources/admin/js`) instead of a template
        #[arg(long, conflicts_with = "react")]
        script: bool,
        /// Draw it with a React component (`resources/admin/src`, host API v2, built with npm)
        #[arg(long)]
        react: bool,
    },
    /// Install or update the admin SDK in `resources/admin/sdk` (edited files are kept)
    #[command(name = "make:sdk")]
    MakeSdk,
    /// Create an admin widget script in `resources/admin/js`
    #[command(name = "make:widget")]
    MakeWidget {
        /// Widget name, `vendor/name`, e.g. `acme/color`
        name: String,
    },
    /// Check that this machine can build an application
    Doctor,
    /// Move the application to this command's RustCMS version (Cargo.toml and Cargo.lock)
    Upgrade {
        /// Show the changes without writing them
        #[arg(long)]
        dry_run: bool,
        /// Allow going back to an older version
        #[arg(long)]
        force: bool,
    },
    /// Replace this command with the latest release (installed by install.sh)
    #[command(name = "self-update")]
    SelfUpdate {
        /// A given release instead of the latest, e.g. `v0.4.0` (may be older)
        #[arg(long)]
        version: Option<String>,
    },
    /// Open the guide of this version in a browser
    Docs {
        /// Print its URL instead of opening it
        #[arg(long)]
        print_path: bool,
    },
    /// Print the command reference as Markdown
    #[command(name = "docs:cli")]
    DocsCli,
    /// Any other command runs in the application (`serve`, `migrate`, `key:generate`, …)
    #[command(external_subcommand)]
    App(Vec<String>),
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::New { name, path, rustcms_path, in_workspace, no_fetch } => {
            let options =
                scaffold::NewOptions { rustcms_path: rustcms_path.as_deref(), in_workspace, fetch: !no_fetch };
            scaffold::new_app(&name, &path, &options)
        }
        Command::MakeModel { name, migration } => app().and_then(|p| generate::make_model(&p, &name, migration)),
        Command::MakeResource { model, generate, belongs_to } => {
            app().and_then(|p| resource::make_resource(&p, &model, generate, &belongs_to))
        }
        Command::MakePolicy { name } => app().and_then(|p| resource::make_policy(&p, &name)),
        Command::MakeTest { name } => app().and_then(|p| generate::make_test(&p, &name)),
        Command::MakeMigration { name } => app().and_then(|p| generate::make_migration(&p, &name)),
        Command::MakeApi { model } => app().and_then(|p| api::make_api(&p, &model)),
        Command::MakeService { name } => app().and_then(|p| make::make_service(&p, &name)),
        Command::MakeJob { name } => app().and_then(|p| make::make_job(&p, &name)),
        Command::MakeSeeder { name } => app().and_then(|p| make::make_seeder(&p, &name)),
        Command::MakeFactory { model } => app().and_then(|p| make::make_factory(&p, &model)),
        Command::MakeSettings { name } => app().and_then(|p| make::make_settings(&p, &name)),
        Command::MakeConsole { name } => app().and_then(|p| make::make_command(&p, &name)),
        Command::MakePage { name, script, react } => app().and_then(|p| {
            let kind = if react {
                make::PageKind::React
            } else if script {
                make::PageKind::Script
            } else {
                make::PageKind::Template
            };
            make::make_page(&p, &name, kind)
        }),
        Command::MakeSdk => app().and_then(|p| {
            let kept = react::install_sdk(&p.root.join("resources/admin/sdk"))?;
            for file in &kept {
                println!("kept resources/admin/sdk/{file} (edited): compare it with the new one yourself");
            }
            println!("SDK {} in resources/admin/sdk; run `npm run build` in resources/admin.", react::sdk_version());
            for hint in react::older_registrations_hint(&p.root) {
                println!("{hint}");
            }
            Ok(std::process::ExitCode::SUCCESS)
        }),
        Command::MakeWidget { name } => app().and_then(|p| generate::make_widget(&p, &name)),
        Command::Doctor => doctor::run(),
        Command::Upgrade { dry_run, force } => {
            project::Project::find().and_then(|p| upgrade::run(&p, &upgrade::Options { dry_run, force }))
        }
        Command::SelfUpdate { version } => self_update::run(version.as_deref()),
        Command::Docs { print_path } => guide::run(print_path),
        Command::DocsCli => {
            let auth = clap::Command::new("auth").subcommands(rustcms_auth::commands::definitions());
            let admin = clap::Command::new("admin").subcommands(rustcms_admin::panel::provider::command_definitions());
            // Every application has the queue commands too.
            let runtime = rustcms_core::console::command()
                .subcommands(rustcms_core::queue::definitions())
                .subcommands(rustcms_core::crypto::definitions())
                .subcommands(rustcms_core::schedule::definitions());
            print!("{}", docs::markdown(&Cli::command(), &runtime, &auth, &admin));
            Ok(ExitCode::SUCCESS)
        }
        Command::App(args) => project::Project::find().and_then(|p| p.forward(&args)),
    };
    match result {
        Ok(code) => code,
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::FAILURE
        }
    }
}

/// The application around the current directory, in the current layout.
fn app() -> anyhow::Result<project::Project> {
    let project = project::Project::find()?;
    project.require_layout()?;
    Ok(project)
}
