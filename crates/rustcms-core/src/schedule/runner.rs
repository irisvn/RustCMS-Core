//! Running the schedules in `serve` (ADR 0031): one task per schedule, one
//! claim at a time per schedule, actions supervised, everything owned.

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use super::claim::{self, Claimed};
use super::{Action, Clock, Schedule, ScheduleConfig, Schedules, SystemClock};
use crate::AppState;

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
    let instance = uuid::Uuid::now_v7().to_string();
    let slot_grace = Duration::from_secs(config.grace_secs);
    for schedule in schedules {
        let (state, cancel, clock, instance) = (state.clone(), cancel.clone(), clock.clone(), instance.clone());
        tasks.spawn(async move {
            run_schedule(state, schedule, slot_grace, shutdown_grace, clock, instance, cancel).await;
        });
    }
    let (state, prune_cancel) = (state.clone(), cancel.clone());
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
    instance: String,
    cancel: CancellationToken,
) {
    let name = schedule.name.clone();
    if let Err(err) = claim::ensure_state(state.db(), &name).await {
        tracing::error!(schedule = %name, %err, "schedule state unavailable; the schedule will not run");
        return;
    }
    // Runs of this schedule: aborted when this task is (the owner dropped).
    let mut runs: JoinSet<()> = JoinSet::new();
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
        match claim::claim(&state, &schedule, slot, grace, clock.as_ref(), &instance).await {
            Ok(Claimed::Mine(job)) => {
                tracing::debug!(schedule = %name, %slot, ?job, "slot claimed");
                if let Some(Action::Run(run)) = &schedule.action {
                    let (run, state, name) = (run.clone(), state.clone(), name.clone());
                    runs.spawn(async move {
                        let outcome =
                            futures_util::FutureExt::catch_unwind(std::panic::AssertUnwindSafe(run(state))).await;
                        match outcome {
                            Ok(Ok(())) => {}
                            Ok(Err(err)) => tracing::warn!(schedule = %name, %err, "scheduled run failed"),
                            Err(_) => tracing::error!(schedule = %name, "scheduled run panicked"),
                        }
                    });
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
}
