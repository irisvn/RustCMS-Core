//! The application's command line (`cargo run -- <command>`), like `php artisan`.

use std::marker::PhantomData;
use std::net::SocketAddr;
use std::path::Path;
use std::process::ExitCode;

use anyhow::{Context, bail};
use clap::{CommandFactory, Parser, Subcommand};
use sea_orm::ConnectionTrait;
use sea_orm_migration::MigratorTrait;

use crate::app::AppBuilder;
use crate::config::{Config, Environment};
use crate::key::AppKey;

#[derive(Parser)]
#[command(about = "rustcms application console", disable_help_subcommand = true)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Start the HTTP server
    Serve {
        /// Address to listen on (default: `app.listen`)
        #[arg(long)]
        listen: Option<SocketAddr>,
    },
    /// Run pending migrations
    Migrate,
    /// Roll back the last migrations
    #[command(name = "migrate:rollback")]
    MigrateRollback {
        /// Number of migrations to roll back
        #[arg(long, default_value_t = 1)]
        step: u32,
    },
    /// Drop all tables and re-run every migration
    #[command(name = "migrate:fresh")]
    MigrateFresh {
        /// Required in production
        #[arg(long)]
        force: bool,
    },
    /// Roll back every migration and run them again
    #[command(name = "migrate:refresh")]
    MigrateRefresh {
        /// Required in production
        #[arg(long)]
        force: bool,
    },
    /// Show applied and pending migrations
    #[command(name = "migrate:status")]
    MigrateStatus,
    /// List the HTTP routes described by the framework, providers and the application
    #[command(name = "route:list")]
    RouteList,
    /// Commands registered by providers and the application (`make:user`, …)
    #[command(external_subcommand)]
    App(Vec<String>),
    /// Generate APP_KEY and write it to .env
    #[command(name = "key:generate")]
    KeyGenerate {
        /// Print the key instead of writing .env
        #[arg(long)]
        show: bool,
        /// Replace an existing APP_KEY
        #[arg(long)]
        force: bool,
    },
}

/// Clap definition of the runtime commands (used by `rustcms docs:cli`).
pub fn command() -> clap::Command {
    Cli::command()
}

/// Entry point of an application binary.
///
/// ```rust,ignore
/// fn main() -> std::process::ExitCode {
///     rustcms::Console::<migrations::Migrator>::new(app::build).run_blocking()
/// }
/// ```
pub struct Console<M: MigratorTrait> {
    build: fn(Config) -> AppBuilder,
    _migrator: PhantomData<M>,
}

impl<M: MigratorTrait> Console<M> {
    /// `build` assembles the application from its configuration.
    pub fn new(build: fn(Config) -> AppBuilder) -> Self {
        Self { build, _migrator: PhantomData }
    }

    /// Parses `std::env::args` and runs the command on a new Tokio runtime.
    pub fn run_blocking(self) -> ExitCode {
        let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
            Ok(runtime) => runtime,
            Err(err) => return fail(&err.into()),
        };
        runtime.block_on(self.run())
    }

    /// Parses `std::env::args` and runs the command.
    pub async fn run(self) -> ExitCode {
        let cli = Cli::parse();
        match self.execute(cli.command).await {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => fail(&err),
        }
    }

    async fn execute(&self, command: Command) -> anyhow::Result<()> {
        if let Command::KeyGenerate { show, force } = command {
            return key_generate(Path::new(".env"), show, force);
        }
        let config = Config::load()?;
        crate::telemetry::init(&config.log);
        let production = config.app.env == Environment::Production;
        match command {
            Command::Serve { listen } => {
                let app = (self.build)(config).build().await?;
                app.serve(listen).await
            }
            Command::Migrate => Ok(M::up(&connect(&config).await?, None).await?),
            Command::MigrateRollback { step } => Ok(M::down(&connect(&config).await?, Some(step)).await?),
            Command::MigrateFresh { force } => {
                guard_production(production, force)?;
                let db = connect_single(&config).await?;
                // SQLite checks foreign keys on DROP TABLE: dropping every table in
                // any order needs them off (on the one connection used here).
                let sqlite = db.get_database_backend() == sea_orm::DbBackend::Sqlite;
                if sqlite {
                    db.execute_unprepared("PRAGMA foreign_keys = OFF").await?;
                }
                let result = M::fresh(&db).await;
                if sqlite {
                    db.execute_unprepared("PRAGMA foreign_keys = ON").await?;
                }
                Ok(result?)
            }
            Command::MigrateRefresh { force } => {
                guard_production(production, force)?;
                Ok(M::refresh(&connect(&config).await?).await?)
            }
            Command::RouteList => {
                let routes = (self.build)(config).described_routes();
                let width = routes.iter().map(|r| r.path.len()).max().unwrap_or(4);
                println!("{:<7} {:<width$}  SOURCE", "METHOD", "PATH");
                for route in routes {
                    println!("{:<7} {:<width$}  {}", route.method.as_str(), route.path, route.source);
                }
                Ok(())
            }
            Command::MigrateStatus => {
                let db = connect(&config).await?;
                for migration in M::get_migration_with_status(&db).await? {
                    println!("{:<8} {}", format!("{:?}", migration.status()), migration.name());
                }
                Ok(())
            }
            Command::App(args) => {
                let builder = (self.build)(config);
                let name = args.first().map(String::as_str).unwrap_or_default();
                let commands = builder.commands();
                let Some(command) = commands.iter().find(|c| c.definition().get_name() == name) else {
                    let known: Vec<String> = commands.iter().map(|c| c.definition().get_name().to_owned()).collect();
                    bail!("unknown command `{name}`; application commands: {}", known.join(", "));
                };
                let matches = match command.definition().try_get_matches_from(&args) {
                    Ok(matches) => matches,
                    Err(err) => err.exit(),
                };
                let app = builder.build().await?;
                command.run(app.state(), &matches).await
            }
            Command::KeyGenerate { .. } => unreachable!(),
        }
    }
}

/// One connection only, so per-connection settings (SQLite pragmas) apply to every statement.
async fn connect_single(config: &Config) -> anyhow::Result<sea_orm::DatabaseConnection> {
    let mut database = config.database.clone();
    database.max_connections = 1;
    crate::db::connect(&database).await.context("cannot connect to the database")
}

async fn connect(config: &Config) -> anyhow::Result<sea_orm::DatabaseConnection> {
    crate::db::connect(&config.database).await.context("cannot connect to the database")
}

fn guard_production(production: bool, force: bool) -> anyhow::Result<()> {
    if production && !force {
        bail!("refusing to drop data in production; pass --force");
    }
    Ok(())
}

fn fail(err: &anyhow::Error) -> ExitCode {
    eprintln!("error: {err:#}");
    ExitCode::FAILURE
}

/// Writes a new `APP_KEY` into `env_file`, keeping every other line.
///
/// Every `APP_KEY` assignment dotenv would accept (`APP_KEY=`, `APP_KEY = `,
/// `export APP_KEY=`, quoted values) is replaced, so no stale definition can
/// shadow the new key.
fn key_generate(env_file: &Path, show: bool, force: bool) -> anyhow::Result<()> {
    let key = AppKey::generate().encode();
    if show {
        println!("{key}");
        return Ok(());
    }
    let contents = std::fs::read_to_string(env_file).unwrap_or_default();
    let mut found = false;
    let mut lines = Vec::new();
    for line in contents.lines() {
        match app_key_assignment(line) {
            Some((export, value)) => {
                if !value.is_empty() && !force {
                    bail!("APP_KEY is already set; pass --force to replace it (existing sessions become invalid)");
                }
                if !found {
                    lines.push(format!("{}APP_KEY={key}", if export { "export " } else { "" }));
                }
                found = true;
            }
            None => lines.push(line.to_owned()),
        }
    }
    if !found {
        lines.push(format!("APP_KEY={key}"));
    }
    std::fs::write(env_file, lines.join("\n") + "\n")
        .with_context(|| format!("cannot write {}", env_file.display()))?;
    println!("APP_KEY written to {}", env_file.display());
    Ok(())
}

/// Parses `[export] APP_KEY [ ]=[ ]value`, returning whether `export` was used and the unquoted value.
fn app_key_assignment(line: &str) -> Option<(bool, &str)> {
    let line = line.trim_start();
    let (export, rest) = match line.strip_prefix("export") {
        Some(rest) if rest.starts_with(char::is_whitespace) => (true, rest.trim_start()),
        _ => (false, line),
    };
    let (name, value) = rest.split_once('=')?;
    if name.trim_end() != "APP_KEY" {
        return None;
    }
    let value = value.trim();
    let value = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')).unwrap_or(value);
    let value = value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')).unwrap_or(value);
    Some((export, value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_generate_sets_and_protects_existing_key() {
        let dir = tempfile::tempdir().unwrap();
        let env = dir.path().join(".env");
        std::fs::write(&env, "APP_NAME=Blog\nAPP_KEY=\n").unwrap();

        key_generate(&env, false, false).unwrap();
        let first = std::fs::read_to_string(&env).unwrap();
        assert!(first.starts_with("APP_NAME=Blog\nAPP_KEY=base64:"), "{first}");
        AppKey::parse(first.lines().nth(1).unwrap().trim_start_matches("APP_KEY=")).unwrap();

        assert!(key_generate(&env, false, false).is_err(), "existing key is protected");
        key_generate(&env, false, true).unwrap();
        assert_ne!(std::fs::read_to_string(&env).unwrap(), first);
    }

    /// The file as dotenv reads it.
    fn effective_key(env: &Path) -> String {
        dotenvy::from_path_iter(env).unwrap().map(Result::unwrap).find(|(k, _)| k == "APP_KEY").unwrap().1
    }

    #[test]
    fn key_generate_replaces_every_dotenv_form() {
        let old = AppKey::generate().encode();
        for (contents, export) in [
            (format!("export APP_KEY={old}\nAPP_NAME=x\n"), true),
            (format!("APP_KEY = \"{old}\"\n"), false),
            (format!("  APP_KEY='{old}'\nAPP_KEY={old}\n"), false),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let env = dir.path().join(".env");
            std::fs::write(&env, &contents).unwrap();

            assert!(key_generate(&env, false, false).is_err(), "protected: {contents}");
            key_generate(&env, false, true).unwrap();

            let written = std::fs::read_to_string(&env).unwrap();
            let new = effective_key(&env);
            assert_ne!(new, old, "{written}");
            AppKey::parse(&new).unwrap();
            assert_eq!(written.matches("APP_KEY").count(), 1, "single definition: {written}");
            assert_eq!(written.contains("export APP_KEY="), export, "{written}");
        }
    }

    #[test]
    fn key_generate_fills_empty_assignment_forms() {
        let dir = tempfile::tempdir().unwrap();
        let env = dir.path().join(".env");
        std::fs::write(&env, "export APP_KEY = \"\"\n").unwrap();
        key_generate(&env, false, false).unwrap();
        AppKey::parse(&effective_key(&env)).unwrap();
    }

    #[test]
    fn similar_names_are_not_app_key() {
        assert_eq!(app_key_assignment("APP_KEYS=x"), None);
        assert_eq!(app_key_assignment("# APP_KEY=x"), None);
        assert_eq!(app_key_assignment("exportAPP_KEY=x"), None);
        assert_eq!(app_key_assignment("export APP_KEY=x"), Some((true, "x")));
    }

    #[test]
    fn key_generate_creates_env_file() {
        let dir = tempfile::tempdir().unwrap();
        let env = dir.path().join(".env");
        key_generate(&env, false, false).unwrap();
        assert!(std::fs::read_to_string(env).unwrap().starts_with("APP_KEY=base64:"));
    }

    #[test]
    fn destructive_commands_need_force_in_production() {
        assert!(guard_production(true, false).is_err());
        assert!(guard_production(true, true).is_ok());
        assert!(guard_production(false, false).is_ok());
    }

    #[test]
    fn command_definition_is_valid() {
        command().debug_assert();
    }
}
