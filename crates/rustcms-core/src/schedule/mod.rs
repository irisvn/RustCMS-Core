//! The scheduler (ADR 0031): periodic work declared with
//! [`AppBuilder::schedule`](crate::AppBuilder::schedule), run only by `serve`.
//!
//! ```rust,ignore
//! app.schedule(Schedule::every("reconcile", Duration::from_secs(60)).job(|| Reconcile {}).without_overlapping());
//! app.schedule(Schedule::cron("docker-prune", "0 3 * * *")?.job(|| Prune {}));
//! app.schedule(Schedule::every("warm-cache", Duration::from_secs(300)).run(|state| async move { warm(&state).await }));
//! ```
//!
//! Slots are instants shared by every instance (multiples of the period since
//! the Unix epoch, or the cron's fire times in UTC). Each is claimed once in the
//! database; missed slots are skipped, never caught up.

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, TimeZone, Utc};
use futures_util::FutureExt;
use futures_util::future::BoxFuture;
use sea_orm::DatabaseTransaction;
use serde::Deserialize;

use crate::queue::{Job, Jobs};
use crate::{AppResult, AppState};

mod claim;
mod commands;
mod runner;
mod runs;

pub use commands::{ScheduleList, ScheduleRun, definitions};
pub(crate) use runner::start;
pub(crate) use runs::prune as prune_runs;
pub use runs::{RunNowError, run_now, runs, states};

/// Shortest period of [`Schedule::every`].
pub const MIN_PERIOD: Duration = Duration::from_secs(10);
/// Longest period of [`Schedule::every`].
pub const MAX_PERIOD: Duration = Duration::from_secs(7 * 24 * 3600);
/// Names starting with this are the framework's.
const RESERVED: &str = "rustcms.";
/// Longest run of a closure schedule unless [`Schedule::timeout`] says otherwise.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(3600);

/// Why a schedule was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ScheduleError {
    /// Not a five-field cron expression of numbers, ranges, lists, steps and
    /// month or weekday names.
    #[error("`{0}` is not a five-field cron expression (UTC)")]
    Cron(String),
    /// A cron expression that never fires in the next five years.
    #[error("`{0}` never fires")]
    Never(String),
}

/// When a schedule fires.
#[derive(Clone)]
pub(crate) enum Rule {
    Every(u64),
    Cron(Arc<croner::Cron>, String),
}

impl Rule {
    /// The first slot strictly after `t`.
    pub(crate) fn next_after(&self, t: DateTime<Utc>) -> Option<DateTime<Utc>> {
        match self {
            Self::Every(period) => {
                let period = i64::try_from(*period).ok()?;
                let next = (t.timestamp().div_euclid(period) + 1).checked_mul(period)?;
                Utc.timestamp_opt(next, 0).single()
            }
            Self::Cron(cron, _) => {
                let start = Utc.timestamp_opt(t.timestamp(), 0).single()?;
                cron.find_next_occurrence(&start, false).ok()
            }
        }
    }

    /// The latest slot at or before `t`.
    pub(crate) fn latest_until(&self, t: DateTime<Utc>) -> Option<DateTime<Utc>> {
        match self {
            Self::Every(period) => {
                let period = i64::try_from(*period).ok()?;
                Utc.timestamp_opt(t.timestamp().div_euclid(period).checked_mul(period)?, 0).single()
            }
            Self::Cron(cron, _) => {
                let start = Utc.timestamp_opt(t.timestamp(), 0).single()?;
                cron.find_previous_occurrence(&start, true).ok()
            }
        }
    }

    pub(crate) fn describe_rule(&self) -> String {
        match self {
            Self::Every(period) => format!("every {period}s"),
            Self::Cron(_, expr) => format!("cron `{expr}` (UTC)"),
        }
    }
}

type Dispatch =
    Arc<dyn for<'t> Fn(&'t DatabaseTransaction, Option<i64>) -> BoxFuture<'t, AppResult<uuid::Uuid>> + Send + Sync>;
/// A closure's run: its summary, or a safe message (never an error's own text).
pub(crate) type Exec = Arc<dyn Fn(TaskCtx) -> BoxFuture<'static, Result<Option<String>, String>> + Send + Sync>;

/// What a slot does.
#[derive(Clone)]
pub(crate) enum Action {
    /// Enqueues a job in the claim's transaction.
    Job { kind: &'static str, dispatch: Dispatch, register: fn(&mut Jobs) },
    /// Runs a closure (`.run` or `.task`) in the process.
    Run(Exec),
}

/// Why a run started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    /// Its slot came.
    Schedule,
    /// Someone ran it (the panel's Run now, `schedule:run`).
    Manual,
}

impl Trigger {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Schedule => "schedule",
            Self::Manual => "manual",
        }
    }
}

/// What a [`Schedule::task`] receives.
#[derive(Clone)]
pub struct TaskCtx {
    /// The application.
    pub state: AppState,
    /// This run (`schedule_runs.id`): stable, a key for the task's own idempotency.
    pub run_id: uuid::Uuid,
    /// Why it runs.
    pub trigger: Trigger,
    /// Who ran it, for a manual run by a user.
    pub initiated_by: Option<i64>,
}

/// What a schedule does, as the panel shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScheduleAction {
    /// Enqueues this job kind.
    Job(&'static str),
    /// Runs a closure (`.run` or `.task`).
    Closure,
}

/// A schedule: a name, when it fires, and what it does.
#[derive(Clone)]
pub struct Schedule {
    pub(crate) name: String,
    pub(crate) rule: Rule,
    pub(crate) action: Option<Action>,
    pub(crate) without_overlapping: bool,
    pub(crate) timeout: Duration,
    pub(crate) framework: bool,
}

impl Schedule {
    /// Every `period` (10 s to 7 days, whole seconds), on its multiples since
    /// the Unix epoch, the same instants on every instance.
    ///
    /// # Panics
    /// On a period out of range or with a fraction of a second.
    pub fn every(name: &str, period: Duration) -> Self {
        assert!(
            (MIN_PERIOD..=MAX_PERIOD).contains(&period) && period.subsec_nanos() == 0,
            "schedule `{name}`: the period must be whole seconds, 10 s to 7 days"
        );
        Self {
            name: name.to_owned(),
            rule: Rule::Every(period.as_secs()),
            action: None,
            without_overlapping: false,
            timeout: DEFAULT_TIMEOUT,
            framework: false,
        }
    }

    /// At the fire times of a five-field cron expression (minute hour
    /// day-of-month month day-of-week), in UTC. Day-of-month and day-of-week
    /// combine with OR, as in Vixie cron; no seconds, years or extensions.
    pub fn cron(name: &str, expr: &str) -> Result<Self, ScheduleError> {
        let cron = parse_cron(expr)?;
        Ok(Self {
            name: name.to_owned(),
            rule: Rule::Cron(Arc::new(cron), expr.trim().to_owned()),
            action: None,
            without_overlapping: false,
            timeout: DEFAULT_TIMEOUT,
            framework: false,
        })
    }

    /// Enqueues the job `make` builds for each slot, in the slot's claim
    /// transaction (registers `J` with the queue). Workers serving its queue
    /// run it, with the queue's retries and leases.
    #[must_use]
    pub fn job<J: Job, F>(mut self, make: F) -> Self
    where
        F: Fn() -> J + Send + Sync + 'static,
    {
        let make = Arc::new(make);
        let dispatch: Dispatch = Arc::new(move |txn, initiated_by| {
            let job = make();
            async move { crate::queue::dispatch_with(txn, &job, None, initiated_by).await }.boxed()
        });
        self.action = Some(Action::Job { kind: J::KIND, dispatch, register: |jobs| jobs.register::<J>() });
        self
    }

    /// Runs `f` in the process for each slot: best effort, at most once per
    /// claimed slot (a crash during the run loses it). On one instance a run
    /// never overlaps the previous one: that slot is skipped.
    /// Its error is recorded as "failed (see the logs)"; use [`Schedule::task`]
    /// to record a summary or a safe message.
    #[must_use]
    pub fn run<F, Fut>(mut self, f: F) -> Self
    where
        F: Fn(AppState) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = AppResult<()>> + Send + 'static,
    {
        let f = Arc::new(f);
        let name = self.name.clone();
        self.action = Some(Action::Run(Arc::new(move |ctx: TaskCtx| {
            let (f, name) = (f.clone(), name.clone());
            async move {
                match f(ctx.state).await {
                    Ok(()) => Ok(None),
                    Err(err) => {
                        tracing::warn!(schedule = %name, %err, "scheduled run failed");
                        Err("failed (see the logs)".to_owned())
                    }
                }
            }
            .boxed()
        })));
        self
    }

    /// Like [`run`](Self::run), with a context and a result the panel's
    /// Schedules page shows: `Ok(Some(summary))` ("3 databases backed up",
    /// at most 200 characters), or an error whose [`crate::queue::Failure`]
    /// message is kept (any other error is kept as "failed (see the logs)").
    /// Holders of `schedules.view` see both: never put secrets in them.
    #[must_use]
    pub fn task<F, Fut>(mut self, f: F) -> Self
    where
        F: Fn(TaskCtx) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = anyhow::Result<Option<String>>> + Send + 'static,
    {
        let f = Arc::new(f);
        let name = self.name.clone();
        self.action = Some(Action::Run(Arc::new(move |ctx: TaskCtx| {
            let (f, name) = (f.clone(), name.clone());
            async move {
                match f(ctx).await {
                    Ok(summary) => Ok(summary),
                    Err(err) => {
                        tracing::warn!(schedule = %name, err = %format!("{err:#}"), "scheduled task failed");
                        Err(crate::queue::safe_message(&err))
                    }
                }
            }
            .boxed()
        })));
        self
    }

    /// Longest run of a `.run` or `.task` schedule (default 1 hour): then it
    /// is stopped and recorded as failed.
    ///
    /// # Panics
    /// On zero or more than a day.
    #[must_use]
    pub fn timeout(mut self, timeout: Duration) -> Self {
        assert!(
            !timeout.is_zero() && timeout <= Duration::from_secs(86_400),
            "schedule `{}`: the timeout must be 1 s to 1 day",
            self.name
        );
        self.timeout = timeout;
        self
    }

    /// Marks one of the framework's own schedules (`rustcms.auth-prune`, …):
    /// only these may use the reserved `rustcms.` prefix. Every other check
    /// (characters, length, uniqueness, action) still applies. A trusted-code
    /// opt-in for the framework's crates, not an access boundary.
    ///
    /// # Panics
    /// When the name does not start with `rustcms.`.
    #[doc(hidden)]
    #[must_use]
    pub fn framework(mut self) -> Self {
        assert!(
            self.name.starts_with(RESERVED),
            "schedule `{}`: framework schedules are named `{RESERVED}…`",
            self.name
        );
        self.framework = true;
        self
    }

    /// For [`job`](Self::job) schedules: a slot is skipped while the job of
    /// the last claimed slot is still pending or running (its queue lease is
    /// the lock). Manual runs and `queue:retry` are not held back.
    #[must_use]
    pub fn without_overlapping(mut self) -> Self {
        self.without_overlapping = true;
        self
    }

    /// The name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// When it fires, in words ("every 300s", "cron `0 3 * * *` (UTC)").
    pub fn describe(&self) -> String {
        self.rule.describe_rule()
    }

    /// The first slot strictly after `t`.
    pub fn next_after(&self, t: DateTime<Utc>) -> Option<DateTime<Utc>> {
        self.rule.next_after(t)
    }

    /// What it does.
    pub fn action(&self) -> ScheduleAction {
        match &self.action {
            Some(Action::Job { kind, .. }) => ScheduleAction::Job(kind),
            _ => ScheduleAction::Closure,
        }
    }

    /// Whether its slots wait for the previous job (`.job` schedules).
    pub fn is_without_overlapping(&self) -> bool {
        self.without_overlapping
    }

    /// Longest run of its closure.
    pub fn run_timeout(&self) -> Duration {
        self.timeout
    }
}

/// A 5-field expression of numbers, `*`, ranges, lists, steps and 3-letter
/// month or weekday names: no seconds, years, `L`, `W`, `#`, `?` or `@…`.
fn parse_cron(expr: &str) -> Result<croner::Cron, ScheduleError> {
    const NAMES: [&str; 19] = [
        "JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC", "SUN", "MON", "TUE", "WED",
        "THU", "FRI", "SAT",
    ];
    let bad = || ScheduleError::Cron(expr.to_owned());
    let fields: Vec<&str> = expr.split_whitespace().collect();
    if fields.len() != 5 {
        return Err(bad());
    }
    for field in &fields {
        for atom in field.split([',', '-', '/']) {
            let ok = atom == "*"
                || atom.bytes().all(|b| b.is_ascii_digit()) && !atom.is_empty()
                || NAMES.contains(&atom.to_ascii_uppercase().as_str());
            if !ok {
                return Err(bad());
            }
        }
    }
    let parser = croner::parser::CronParser::builder()
        .seconds(croner::parser::Seconds::Disallowed)
        .year(croner::parser::Year::Disallowed)
        .dom_and_dow(false)
        .build();
    let cron = parser.parse(&fields.join(" ")).map_err(|_| bad())?;
    let now = Utc::now();
    let horizon = now + chrono::Duration::days(5 * 366);
    match cron.find_next_occurrence(&now, false) {
        Ok(next) if next <= horizon => Ok(cron),
        _ => Err(ScheduleError::Never(expr.to_owned())),
    }
}

/// The schedules of an application.
#[derive(Clone, Default)]
pub struct Schedules(pub(crate) Vec<Schedule>);

impl Schedules {
    /// Adds `schedule`, registering its job.
    ///
    /// # Panics
    /// On an invalid, reserved or repeated name, a schedule without an action,
    /// or `without_overlapping` on a `run` schedule.
    pub(crate) fn add(&mut self, schedule: Schedule, jobs: &mut Jobs) {
        let name = &schedule.name;
        assert!(
            !name.is_empty()
                && name.len() <= 64
                && name.bytes().all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b'-')),
            "schedule `{name}`: names are 1–64 of `a-z`, `0-9`, `_`, `.`, `-`"
        );
        assert!(
            schedule.framework || !name.starts_with(RESERVED),
            "schedule `{name}`: names starting with `{RESERVED}` are reserved"
        );
        assert!(self.0.iter().all(|s| s.name != *name), "schedule `{name}` declared twice");
        match &schedule.action {
            None => panic!("schedule `{name}`: give it a `.job(..)`, `.run(..)` or `.task(..)`"),
            Some(Action::Run(_)) => {
                assert!(
                    !schedule.without_overlapping,
                    "schedule `{name}`: `without_overlapping` is for `.job` schedules"
                );
                // Run now runs closures through the queue.
                runs::register(jobs);
            }
            Some(Action::Job { register, .. }) => register(jobs),
        }
        self.0.push(schedule);
    }

    /// Every schedule.
    pub fn all(&self) -> &[Schedule] {
        &self.0
    }

    /// The schedule `name`.
    pub fn get(&self, name: &str) -> Option<&Schedule> {
        self.0.iter().find(|s| s.name == name)
    }
}

/// `[schedule]`.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ScheduleConfig {
    /// Run the scheduler in `serve` on this instance.
    pub enabled: bool,
    /// Seconds a slot may still be claimed after its instant (1 to 300).
    pub grace_secs: u64,
    /// Runs are kept in the history this many days (1 to 3650).
    pub history_retention_days: u32,
}

impl Default for ScheduleConfig {
    fn default() -> Self {
        Self { enabled: true, grace_secs: 30, history_retention_days: 7 }
    }
}

impl ScheduleConfig {
    /// The `[schedule]` section, validated.
    pub fn of(config: &crate::Config) -> anyhow::Result<Self> {
        // An absent section is the defaults; a malformed one is an error.
        let section: Self = config.section("schedule").map_err(|e| anyhow::anyhow!("invalid [schedule]: {e}"))?;
        anyhow::ensure!((1..=300).contains(&section.grace_secs), "schedule.grace_secs must be 1 to 300");
        anyhow::ensure!(
            (1..=3_650).contains(&section.history_retention_days),
            "schedule.history_retention_days must be 1 to 3650"
        );
        Ok(section)
    }
}

/// The time the scheduler reads (a test clock follows tokio's paused time).
pub(crate) trait Clock: Send + Sync + 'static {
    fn now(&self) -> DateTime<Utc>;
}

/// The system clock.
pub(crate) struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text).unwrap().with_timezone(&Utc)
    }

    #[test]
    fn every_aligns_on_the_epoch() {
        let rule = Rule::Every(60);
        assert_eq!(rule.next_after(at("2026-10-18T10:00:30Z")), Some(at("2026-10-18T10:01:00Z")));
        assert_eq!(rule.next_after(at("2026-10-18T10:01:00Z")), Some(at("2026-10-18T10:02:00Z")), "strictly after");
        assert_eq!(rule.latest_until(at("2026-10-18T10:01:00Z")), Some(at("2026-10-18T10:01:00Z")));
        assert_eq!(rule.latest_until(at("2026-10-18T10:01:59Z")), Some(at("2026-10-18T10:01:00Z")));
        // Two instances agree whatever their start.
        let a = Rule::Every(300).next_after(at("2026-10-18T10:02:13Z"));
        let b = Rule::Every(300).next_after(at("2026-10-18T10:04:59Z"));
        assert_eq!(a, b);
    }

    #[test]
    fn cron_is_utc_five_fields_with_or_days() {
        let s = Schedule::cron("x", "0 3 * * *").unwrap();
        assert_eq!(s.rule.next_after(at("2026-10-18T03:00:00Z")), Some(at("2026-10-19T03:00:00Z")));
        assert_eq!(s.rule.latest_until(at("2026-10-18T03:00:00Z")), Some(at("2026-10-18T03:00:00Z")));
        // Month boundary, and a leap day.
        let s = Schedule::cron("x", "30 12 29 2 *").unwrap();
        assert_eq!(s.rule.next_after(at("2026-10-18T00:00:00Z")), Some(at("2028-02-29T12:30:00Z")));
        // Day-of-month OR day-of-week (Vixie): the 1st, or any Monday.
        let s = Schedule::cron("x", "0 0 1 * MON").unwrap();
        assert_eq!(s.rule.next_after(at("2026-10-18T00:00:00Z")), Some(at("2026-10-19T00:00:00Z")), "a Monday");
        assert_eq!(s.rule.next_after(at("2026-10-26T00:00:00Z")), Some(at("2026-11-01T00:00:00Z")), "the 1st");
        for bad in ["* * * *", "0 * * * * *", "@daily", "0 0 L * *", "0 0 ? * MON", "0 0 * * MON#2", "a b c d e"] {
            assert!(matches!(Schedule::cron("x", bad), Err(ScheduleError::Cron(_))), "{bad}");
        }
        assert!(matches!(Schedule::cron("x", "0 0 30 2 *"), Err(ScheduleError::Never(_))));
    }

    #[test]
    #[should_panic(expected = "10 s to 7 days")]
    fn periods_are_bounded() {
        let _ = Schedule::every("x", Duration::from_secs(5));
    }

    #[test]
    fn names_are_checked() {
        let mut jobs = Jobs::default();
        let run = |name: &str| Schedule::every(name, MIN_PERIOD).run(|_| async { Ok(()) });
        let mut schedules = Schedules::default();
        schedules.add(run("ok.name-1"), &mut jobs);
        for bad in ["", "Bad", "rustcms.prune", "ok.name-1"] {
            let mut copy = schedules.clone();
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| copy.add(run(bad), &mut Jobs::default())));
            assert!(result.is_err(), "{bad:?}");
        }
    }

    #[test]
    fn only_framework_schedules_use_the_reserved_prefix() {
        let mut jobs = Jobs::default();
        let run = |name: &str| Schedule::every(name, MIN_PERIOD).run(|_| async { Ok(()) });
        let mut schedules = Schedules::default();
        schedules.add(run("rustcms.prune").framework(), &mut jobs);
        assert!(schedules.get("rustcms.prune").is_some());
        let refused = |schedules: &Schedules, schedule: Schedule| {
            let mut copy = schedules.clone();
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| copy.add(schedule, &mut Jobs::default()))).is_err()
        };
        // Without the opt-in the prefix stays reserved.
        assert!(refused(&schedules, run("rustcms.other")));
        // The other checks still apply to framework schedules.
        assert!(refused(&schedules, run("rustcms.prune").framework()), "declared twice");
        assert!(refused(&schedules, run("rustcms.Bad").framework()), "charset");
        assert!(refused(&schedules, run(&format!("rustcms.{}", "x".repeat(60))).framework()), "length");
        assert!(refused(&schedules, Schedule::every("rustcms.none", MIN_PERIOD).framework()), "no action");
    }

    #[test]
    #[should_panic(expected = "framework schedules are named `rustcms.…`")]
    fn framework_schedules_need_the_prefix() {
        let _ = Schedule::every("prune", MIN_PERIOD).framework();
    }
}

/// Hooks for the framework's own tests (not a stable API).
#[doc(hidden)]
pub mod testing {
    use std::time::Duration;

    use chrono::{DateTime, Utc};

    use super::{Rule, Schedule, Schedules, SystemClock, claim};
    use crate::{AppResult, AppState};

    pub use super::claim::Claimed;

    /// `Schedule::every` without the 10-second minimum (whole seconds).
    pub fn every(name: &str, period: Duration) -> Schedule {
        Schedule {
            name: name.to_owned(),
            rule: Rule::Every(period.as_secs().max(1)),
            action: None,
            without_overlapping: false,
            timeout: super::DEFAULT_TIMEOUT,
            framework: false,
        }
    }

    /// Claims `slot` of the registered schedule `name` now.
    pub async fn claim(state: &AppState, name: &str, slot: DateTime<Utc>, grace: Duration) -> AppResult<Claimed> {
        let schedules = state.service::<Schedules>().expect("schedules");
        let schedule = schedules.get(name).expect("a registered schedule").clone();
        claim::ensure_state(state.db(), name).await?;
        claim::claim(state, &schedule, slot, grace, &SystemClock, &tokio_util::sync::CancellationToken::new()).await
    }

    /// Prunes claims older than `before`.
    pub async fn prune(state: &AppState, before: DateTime<Utc>) -> AppResult<u64> {
        claim::prune(state.db(), before).await
    }

    /// A scheduler clock that follows tokio's time from `start`: bound with
    /// `app.bind(..)`, `serve`'s scheduler reads it instead of the system
    /// clock, so a test that pauses and advances tokio's time crosses slots.
    #[derive(Clone)]
    pub struct TokioClock {
        start: DateTime<Utc>,
        at: tokio::time::Instant,
    }

    impl TokioClock {
        /// Reads `start` now, then moves with tokio's time.
        pub fn starting_at(start: DateTime<Utc>) -> Self {
            Self { start, at: tokio::time::Instant::now() }
        }

        /// The time it reads.
        pub fn now(&self) -> DateTime<Utc> {
            self.start + chrono::Duration::from_std(self.at.elapsed()).unwrap_or_default()
        }
    }

    impl super::Clock for TokioClock {
        fn now(&self) -> DateTime<Utc> {
            TokioClock::now(self)
        }
    }
}
