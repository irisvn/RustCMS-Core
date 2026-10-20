//! `schedule:list` and `schedule:run`.

use async_trait::async_trait;
use clap::{Arg, ArgMatches, Command};

use sea_orm::ActiveModelTrait;

use super::{Action, Schedules, claim};
use crate::{AppCommand, AppState};

/// `schedule:list`: every schedule, its next slot and its last claim.
pub struct ScheduleList;

#[async_trait]
impl AppCommand for ScheduleList {
    fn definition(&self) -> Command {
        Command::new("schedule:list").about("List the schedules, their next slot and their last claimed slot")
    }

    async fn run(&self, state: &AppState, _: &ArgMatches) -> anyhow::Result<()> {
        let schedules = state.service::<Schedules>().map(|s| s.0.clone()).unwrap_or_default();
        if schedules.is_empty() {
            println!("No schedules.");
            return Ok(());
        }
        let states = claim::states(state.db()).await.unwrap_or_default();
        let now = chrono::Utc::now();
        for schedule in &schedules {
            let action = match &schedule.action {
                Some(Action::Job { kind, .. }) => format!("job `{kind}`"),
                Some(Action::Run(_)) => "run".to_owned(),
                None => "-".to_owned(),
            };
            let overlap = if schedule.without_overlapping { ", without overlapping" } else { "" };
            let next = schedule.rule.next_after(now).map_or_else(|| "never".to_owned(), |t| t.to_rfc3339());
            let last = states.iter().find(|s| s.name == schedule.name);
            let last = last.and_then(|s| s.last_slot_at).map_or_else(|| "never".to_owned(), |t| t.to_rfc3339());
            let job = states
                .iter()
                .find(|s| s.name == schedule.name)
                .and_then(|s| s.active_job_id)
                .map(|id| format!(" (job {id})"))
                .unwrap_or_default();
            println!(
                "{}  {}  {action}{overlap}  next {next}  last {last}{job}",
                schedule.name,
                schedule.rule.describe_rule()
            );
        }
        Ok(())
    }
}

/// `schedule:run <name>`: runs a schedule now, as a manual run.
pub struct ScheduleRun;

#[async_trait]
impl AppCommand for ScheduleRun {
    fn definition(&self) -> Command {
        Command::new("schedule:run")
            .about(
                "Run a schedule now (a manual run: no slot claim, no grace, no without_overlapping; a job is \
                 enqueued for the workers, a run runs here)",
            )
            .arg(Arg::new("name").required(true).help("The schedule's name"))
    }

    async fn run(&self, state: &AppState, args: &ArgMatches) -> anyhow::Result<()> {
        let name = args.get_one::<String>("name").expect("required");
        let schedules = state.service::<Schedules>().map(|s| (*s).clone()).unwrap_or_default();
        let schedule = schedules.get(name).ok_or_else(|| anyhow::anyhow!("no schedule named `{name}`"))?;
        match &schedule.action {
            Some(Action::Job { dispatch, kind, .. }) => {
                let txn = crate::db::begin_write(state.db()).await?;
                let id = dispatch(&txn, None).await?;
                super::runs::new_run(name, None, super::Trigger::Manual, "dispatched", Some(id), None)
                    .insert(&txn)
                    .await?;
                txn.commit().await?;
                println!("enqueued job `{kind}` {id}");
            }
            Some(Action::Run(exec)) => {
                let run = super::runs::new_run(name, None, super::Trigger::Manual, "running", None, None)
                    .insert(state.db())
                    .await?;
                let ctx = super::TaskCtx {
                    state: state.clone(),
                    run_id: run.id,
                    trigger: super::Trigger::Manual,
                    initiated_by: None,
                };
                let result = tokio::select! {
                    result = super::runs::execute(exec, ctx, schedule.timeout) => result,
                    () = crate::app::shutdown_signal() => {
                        super::runs::finish(state.db(), run.id, "unknown", None, Some("interrupted")).await?;
                        anyhow::bail!("cancelled")
                    }
                };
                match result {
                    Ok(summary) => {
                        super::runs::finish(state.db(), run.id, "succeeded", summary.as_deref(), None).await?;
                        match summary {
                            Some(summary) => println!("ran `{name}`: {summary}"),
                            None => println!("ran `{name}`"),
                        }
                    }
                    Err(message) => {
                        super::runs::finish(state.db(), run.id, "failed", None, Some(&message)).await?;
                        anyhow::bail!("`{name}` failed: {message}");
                    }
                }
            }
            None => anyhow::bail!("schedule `{name}` has no action"),
        }
        Ok(())
    }
}

/// Clap definitions of the schedule commands (for `rustcms docs:cli`).
pub fn definitions() -> Vec<Command> {
    vec![ScheduleList.definition(), ScheduleRun.definition()]
}
