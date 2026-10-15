//! `queue:work`, `queue:failed`, `queue:retry`, `queue:forget`.

use async_trait::async_trait;
use clap::{Arg, ArgAction, ArgMatches, Command};
use sea_orm::sea_query::Expr;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};

use super::{QueueConfig, Workers, now, run_until_idle, valid_queue};
use crate::models::job;
use crate::{AppCommand, AppState};

fn queues(args: &ArgMatches) -> anyhow::Result<Option<Vec<String>>> {
    let Some(values) = args.get_many::<String>("queue") else { return Ok(None) };
    let queues: Vec<String> = values.cloned().collect();
    for queue in &queues {
        anyhow::ensure!(valid_queue(queue), "invalid queue name `{queue}`");
    }
    Ok(Some(queues))
}

/// `queue:work`: runs workers until stopped (Ctrl-C / SIGTERM), or once.
pub struct QueueWork;

#[async_trait]
impl AppCommand for QueueWork {
    fn definition(&self) -> Command {
        Command::new("queue:work")
            .about("Run queued jobs until stopped (Ctrl-C / SIGTERM lets running jobs finish)")
            .arg(
                Arg::new("queue")
                    .long("queue")
                    .action(ArgAction::Append)
                    .help("Queue to take jobs from (repeatable; default: queue.queues)"),
            )
            .arg(
                Arg::new("once")
                    .long("once")
                    .action(ArgAction::SetTrue)
                    .help("Run the jobs ready now (at most 1,000), then exit"),
            )
            .arg(
                Arg::new("workers")
                    .long("workers")
                    .value_parser(clap::value_parser!(u16).range(1..=64))
                    .help("Workers (default: queue.workers)"),
            )
    }

    async fn run(&self, state: &AppState, args: &ArgMatches) -> anyhow::Result<()> {
        let config = QueueConfig::of(state.config())?;
        let queues = queues(args)?;
        if args.get_flag("once") {
            let ran = run_until_idle(state, queues, 1_000).await?;
            println!("{ran} jobs run");
            return Ok(());
        }
        let count = args.get_one::<u16>("workers").map_or(config.workers.max(1), |n| usize::from(*n));
        let workers = Workers::start(state, &config, queues, count);
        tracing::info!(workers = count, "queue workers started");
        crate::app::shutdown_signal().await;
        workers.shutdown().await;
        Ok(())
    }
}

/// `queue:failed`: lists jobs that will not run again.
pub struct QueueFailed;

#[async_trait]
impl AppCommand for QueueFailed {
    fn definition(&self) -> Command {
        Command::new("queue:failed").about("List failed jobs")
    }

    async fn run(&self, state: &AppState, _: &ArgMatches) -> anyhow::Result<()> {
        let failed = job::Entity::find()
            .filter(job::Column::FailedAt.is_not_null())
            .order_by_desc(job::Column::FailedAt)
            .all(state.db())
            .await?;
        if failed.is_empty() {
            println!("No failed jobs.");
        }
        for job in failed {
            let when = job.failed_at.map(|t| t.to_rfc3339()).unwrap_or_default();
            println!(
                "{}  {}  {} attempts  {}  {}",
                job.id,
                job.kind,
                job.attempts,
                when,
                job.last_error.unwrap_or_default()
            );
        }
        Ok(())
    }
}

fn target(args: &ArgMatches) -> anyhow::Result<Option<uuid::Uuid>> {
    if args.get_flag("all") {
        return Ok(None);
    }
    let id = args.get_one::<String>("id").ok_or_else(|| anyhow::anyhow!("give a job id or --all"))?;
    Ok(Some(id.parse().map_err(|_| anyhow::anyhow!("`{id}` is not a job id"))?))
}

fn by_id(command: Command) -> Command {
    command
        .arg(Arg::new("id").help("Job id"))
        .arg(Arg::new("all").long("all").action(ArgAction::SetTrue).conflicts_with("id").help("Every failed job"))
}

/// `queue:retry`: runs failed jobs again.
pub struct QueueRetry;

#[async_trait]
impl AppCommand for QueueRetry {
    fn definition(&self) -> Command {
        by_id(Command::new("queue:retry").about("Run failed jobs again (attempts start over)"))
    }

    async fn run(&self, state: &AppState, args: &ArgMatches) -> anyhow::Result<()> {
        let mut update = job::Entity::update_many()
            .col_expr(job::Column::FailedAt, Expr::value(Option::<chrono::DateTime<chrono::Utc>>::None))
            .col_expr(job::Column::Attempts, Expr::value(0))
            .col_expr(job::Column::AvailableAt, Expr::value(now()))
            .col_expr(job::Column::ReservedBy, Expr::value(Option::<String>::None))
            .col_expr(job::Column::ReservedUntil, Expr::value(Option::<chrono::DateTime<chrono::Utc>>::None))
            .filter(job::Column::FailedAt.is_not_null());
        if let Some(id) = target(args)? {
            update = update.filter(job::Column::Id.eq(id));
        }
        let retried = update.exec(state.db()).await?.rows_affected;
        println!("{retried} jobs queued again");
        Ok(())
    }
}

/// `queue:forget`: deletes failed jobs.
pub struct QueueForget;

#[async_trait]
impl AppCommand for QueueForget {
    fn definition(&self) -> Command {
        by_id(Command::new("queue:forget").about("Delete failed jobs"))
    }

    async fn run(&self, state: &AppState, args: &ArgMatches) -> anyhow::Result<()> {
        let mut delete = job::Entity::delete_many().filter(job::Column::FailedAt.is_not_null());
        if let Some(id) = target(args)? {
            delete = delete.filter(job::Column::Id.eq(id));
        }
        let deleted = delete.exec(state.db()).await?.rows_affected;
        println!("{deleted} failed jobs deleted");
        Ok(())
    }
}

/// Clap definitions of the queue commands (for `rustcms docs:cli`).
pub fn definitions() -> Vec<Command> {
    vec![QueueWork.definition(), QueueFailed.definition(), QueueRetry.definition(), QueueForget.definition()]
}
