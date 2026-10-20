//! Running the schedules in `serve` (ADR 0031): one task per schedule, one
//! claim at a time per schedule, actions supervised, everything owned.

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

use super::claim::{self, Claimed};
use super::{Action, Clock, Schedule, ScheduleConfig, Schedules, SystemClock, TaskCtx, Trigger};
use crate::AppState;
use crate::models::schedule_run;

/// Longest sleep before the wall clock is read again (clock jumps, long
/// cron waits).
const MAX_SLEEP: Duration = Duration::from_secs(60);
/// How often old claims are pruned.
const PRUNE_EVERY: Duration = Duration::from_secs(3600);
/// How long claims are kept.
const KEEP_CLAIMS: chrono::Duration = chrono::Duration::days(7);

/// The running scheduler, owned by `serve`. Dropping it cancels the
/// schedules and aborts every task it started, actions included.
pub(crate) struct Scheduler {
    cancel: CancellationToken,
    tasks: JoinSet<()>,
}

impl Drop for Scheduler {
    fn drop(&mut self) {
        self.cancel.cancel();
        self.tasks.abort_all();
    }
}

impl Scheduler {
    /// Stops claiming; running actions get `grace`, then are aborted.
    pub(crate) async fn shutdown(mut self, grace: Duration) {
        self.cancel.cancel();
        let joined = tokio::time::timeout(grace + Duration::from_secs(1), async {
            while self.tasks.join_next().await.is_some() {}
        })
        .await;
        if joined.is_err() {
            tracing::warn!("scheduled actions still running after the grace: aborted");
        }
    }
}

/// Starts the schedules of `state` (with `[schedule] enabled`), or nothing.
pub(crate) fn start(state: &AppState, grace: Duration) -> anyhow::Result<Option<Scheduler>> {
    let config = ScheduleConfig::of(state.config())?;
    let schedules = state.service::<Schedules>().map(|s| s.0.clone()).unwrap_or_default();
    if !config.enabled || schedules.is_empty() {
        return Ok(None);
    }
    Ok(Some(start_with(state, schedules, &config, grace, Arc::new(SystemClock))))
}

pub(crate) fn start_with(
    state: &AppState,
    schedules: Vec<Schedule>,
    config: &ScheduleConfig,
    shutdown_grace: Duration,
    clock: Arc<dyn Clock>,
) -> Scheduler {
    let cancel = CancellationToken::new();
    let mut tasks = JoinSet::new();
    let slot_grace = Duration::from_secs(config.grace_secs);
    for schedule in schedules {
        let (state, cancel, clock) = (state.clone(), cancel.clone(), clock.clone());
        tasks.spawn(async move {
            run_schedule(state, schedule, slot_grace, shutdown_grace, clock, cancel).await;
        });
    }
    let (state, prune_cancel) = (state.clone(), cancel.clone());
    let history_days = config.history_retention_days;
    let prune_clock = clock;
    tasks.spawn(async move {
        loop {
            tokio::select! {
                () = prune_cancel.cancelled() => return,
                () = tokio::time::sleep(PRUNE_EVERY) => {}
            }
            match claim::prune(state.db(), prune_clock.now() - KEEP_CLAIMS).await {
                Ok(n) if n > 0 => tracing::info!(deleted = n, "old schedule claims pruned"),
                Ok(_) => {}
                Err(err) => tracing::warn!(%err, "pruning schedule claims failed; retried in an hour"),
            }
            let keep = chrono::Duration::days(i64::from(history_days));
            match super::runs::prune(state.db(), prune_clock.now() - keep).await {
                Ok(n) if n > 0 => tracing::info!(deleted = n, "old schedule runs pruned"),
                Ok(_) => {}
                Err(err) => tracing::warn!(%err, "pruning schedule runs failed; retried in an hour"),
            }
        }
    });
    Scheduler { cancel, tasks }
}

/// One schedule: wait for its next slot, claim it, act; never two claims at
/// once, and never two `run`s at once on this instance.
async fn run_schedule(
    state: AppState,
    schedule: Schedule,
    grace: Duration,
    shutdown_grace: Duration,
    clock: Arc<dyn Clock>,
    cancel: CancellationToken,
) {
    let name = schedule.name.clone();
    // The state row, retried with backoff (a busy or unreachable database at start).
    let mut wait = Duration::from_secs(1);
    while let Err(err) = claim::ensure_state(state.db(), &name).await {
        tracing::warn!(schedule = %name, %err, "schedule state unavailable; retrying");
        tokio::select! {
            () = cancel.cancelled() => return,
            () = tokio::time::sleep(wait) => {}
        }
        wait = (wait * 2).min(MAX_SLEEP);
    }
    // Runs of this schedule: aborted when this task is (the owner dropped).
    let mut runs: JoinSet<()> = JoinSet::new();
    // Their run ids, until recorded: those still going at the stop are `unknown`.
    let active: Arc<std::sync::Mutex<std::collections::HashSet<uuid::Uuid>>> = Arc::default();
    let mut last: DateTime<Utc> = clock.now();
    loop {
        while runs.try_join_next().is_some() {}
        let Some(next) = schedule.rule.next_after(last) else { return };
        let now = clock.now();
        if next > now {
            let wait = (next - now).to_std().unwrap_or_default().min(MAX_SLEEP);
            tokio::select! {
                () = cancel.cancelled() => break,
                () = tokio::time::sleep(wait) => {}
            }
            // A clock that went back: wait for the next slot after now.
            if clock.now() < last {
                last = clock.now();
            }
            continue;
        }
        // Woke at or after a slot: only the latest elapsed one, if in grace.
        let slot = schedule.rule.latest_until(now).unwrap_or(next);
        last = slot;
        if (now - slot).to_std().is_ok_and(|elapsed| elapsed > grace) {
            tracing::info!(schedule = %name, %slot, "slot missed (past the grace): skipped");
            continue;
        }
        if cancel.is_cancelled() {
            break;
        }
        if matches!(schedule.action, Some(Action::Run(_))) && !runs.is_empty() {
            tracing::info!(schedule = %name, %slot, "previous run still in progress: slot skipped");
            continue;
        }
        match claim::claim(&state, &schedule, slot, grace, clock.as_ref(), &cancel).await {
            Ok(Claimed::Mine(job)) => {
                tracing::debug!(schedule = %name, %slot, ?job, "slot claimed");
                // Claimed as the stop came: the slot is spent, nothing new starts.
                if let Some(Action::Run(exec)) = schedule.action.as_ref() {
                    let run_id = run_of(&state, &name, slot).await;
                    if cancel.is_cancelled() {
                        // Claimed as the stop came: the slot is spent, nothing new starts.
                        if let Some(id) = run_id {
                            let _ =
                                super::runs::finish(state.db(), id, "unknown", None, Some("interrupted by shutdown"))
                                    .await;
                        }
                    } else if let Some(run_id) = run_id {
                        let (exec, state, name, timeout) =
                            (exec.clone(), state.clone(), name.clone(), schedule.timeout);
                        active.lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(run_id);
                        let active = active.clone();
                        runs.spawn(async move {
                            let ctx = TaskCtx {
                                state: state.clone(),
                                run_id,
                                trigger: Trigger::Schedule,
                                initiated_by: None,
                            };
                            let (status, summary, error) = match super::runs::execute(&exec, ctx, timeout).await {
                                Ok(summary) => ("succeeded", summary, None),
                                Err(message) => {
                                    tracing::warn!(schedule = %name, %message, "scheduled run failed");
                                    ("failed", None, Some(message))
                                }
                            };
                            if let Err(err) =
                                super::runs::finish(state.db(), run_id, status, summary.as_deref(), error.as_deref())
                                    .await
                            {
                                tracing::warn!(schedule = %name, %err, "schedule run not recorded");
                            }
                            active.lock().unwrap_or_else(std::sync::PoisonError::into_inner).remove(&run_id);
                        });
                    }
                }
            }
            Ok(Claimed::Taken) => {}
            Ok(Claimed::Late) => tracing::info!(schedule = %name, %slot, "slot claimed too late: skipped"),
            Ok(Claimed::Overlapping) => {
                tracing::info!(schedule = %name, %slot, "previous job still pending or running: slot skipped");
            }
            Err(err) => tracing::warn!(schedule = %name, %slot, %err, "slot claim failed: skipped"),
        }
    }
    // Stopping: running actions get the grace, then are aborted (with `runs`).
    let _ = tokio::time::timeout(shutdown_grace, async { while runs.join_next().await.is_some() {} }).await;
    runs.abort_all();
    let left: Vec<uuid::Uuid> = active.lock().unwrap_or_else(std::sync::PoisonError::into_inner).drain().collect();
    for id in left {
        let _ = super::runs::finish(state.db(), id, "unknown", None, Some("interrupted by shutdown")).await;
    }
}

/// The run recorded with the claim of `slot`.
async fn run_of(state: &AppState, name: &str, slot: DateTime<Utc>) -> Option<uuid::Uuid> {
    match schedule_run::Entity::find()
        .filter(schedule_run::Column::Name.eq(name))
        .filter(schedule_run::Column::SlotAt.eq(slot))
        .filter(schedule_run::Column::Trigger.eq("schedule"))
        .one(state.db())
        .await
    {
        Ok(row) => row.map(|r| r.id),
        Err(err) => {
            tracing::warn!(schedule = %name, %err, "schedule run not found; not run");
            None
        }
    }
}
