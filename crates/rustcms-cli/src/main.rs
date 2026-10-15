//! `rustcms`: create applications, generate code, and forward runtime commands
//! (`serve`, `migrate`, …) to the application in the current directory.

mod api;
mod docs;
mod generate;
mod naming;
mod project;
mod resource;
mod scaffold;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{CommandFactory, Parser, Subcommand};

#[derive(Parser)]
#[command(name = "rustcms", version, about = "rustcms command line", disable_help_subcommand = true)]
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
        /// Use a local rustcms checkout instead of crates.io (written verbatim, relative to the new app)
        #[arg(long, hide = true)]
        rustcms_path: Option<String>,
        /// Do not declare a standalone `[workspace]` (for apps inside another workspace)
        #[arg(long)]
        in_workspace: bool,
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
    /// Create an admin resource in `src/admin` for a model
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
    /// Create a migration in `src/migrations`
    #[command(name = "make:migration")]
    MakeMigration {
        /// snake_case name; `create_<table>_table` generates a table skeleton
        name: String,
    },
    /// Create an integration test in `tests/`
    #[command(name = "make:test")]
    MakeTest {
        /// Test name in PascalCase, e.g. `PostAdminTest`
        name: String,
    },
    /// Create a JSON API module for a model in `src/http/api` (list and show)
    #[command(name = "make:api")]
    MakeApi {
        /// Model name in PascalCase (the model must exist in `src/models`)
        model: String,
    },
    /// Create an admin widget script in `assets/admin`
    #[command(name = "make:widget")]
    MakeWidget {
        /// Widget name, `vendor/name`, e.g. `acme/color`
        name: String,
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
        Command::New { name, path, rustcms_path, in_workspace } => {
            scaffold::new_app(&name, &path, rustcms_path.as_deref(), in_workspace)
        }
        Command::MakeModel { name, migration } => {
            project::Project::find().and_then(|p| generate::make_model(&p, &name, migration))
        }
        Command::MakeResource { model, generate, belongs_to } => {
            project::Project::find().and_then(|p| resource::make_resource(&p, &model, generate, &belongs_to))
        }
        Command::MakePolicy { name } => project::Project::find().and_then(|p| resource::make_policy(&p, &name)),
        Command::MakeTest { name } => project::Project::find().and_then(|p| generate::make_test(&p, &name)),
        Command::MakeMigration { name } => project::Project::find().and_then(|p| generate::make_migration(&p, &name)),
        Command::MakeApi { model } => project::Project::find().and_then(|p| api::make_api(&p, &model)),
        Command::MakeWidget { name } => project::Project::find().and_then(|p| generate::make_widget(&p, &name)),
        Command::DocsCli => {
            let auth = clap::Command::new("auth").subcommands(rustcms_auth::commands::definitions());
            let admin = clap::Command::new("admin").subcommands(rustcms_admin::panel::provider::command_definitions());
            // Every application has the queue commands too.
            let runtime = rustcms_core::console::command().subcommands(rustcms_core::queue::definitions());
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
