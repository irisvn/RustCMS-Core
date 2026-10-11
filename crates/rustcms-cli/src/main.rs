//! `rustcms`: create applications, generate code, and forward runtime commands
//! (`serve`, `migrate`, …) to the application in the current directory.

mod docs;
mod generate;
mod naming;
mod project;
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
        #[arg(long)]
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
    /// Create a migration in `src/migrations`
    #[command(name = "make:migration")]
    MakeMigration {
        /// snake_case name; `create_<table>_table` generates a table skeleton
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
        Command::MakeMigration { name } => project::Project::find().and_then(|p| generate::make_migration(&p, &name)),
        Command::DocsCli => {
            let auth = clap::Command::new("auth").subcommands(rustcms_auth::commands::definitions());
            print!("{}", docs::markdown(&Cli::command(), &rustcms_core::console::command(), &auth));
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
