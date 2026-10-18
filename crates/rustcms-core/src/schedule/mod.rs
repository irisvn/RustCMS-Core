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

pub use commands::{ScheduleList, ScheduleRun, definitions};
pub(crate) use runner::start;

/// Shortest period of [`Schedule::every`].
pub const MIN_PERIOD: Duration = Duration::from_secs(10);
/// Longest period of [`Schedule::every`].
pub const MAX_PERIOD: Duration = Duration::from_secs(7 * 24 * 3600);
/// Names starting with this are the framework's.
const RESERVED: &str = "rustcms.";

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

    pub(crate) fn describe(&self) -> String {
        match self {
            Self::Every(period) => format!("every {period}s"),
            Self::Cron(_, expr) => format!("cron `{expr}` (UTC)"),
        }
    }
}

type Dispatch = Arc<dyn for<'t> Fn(&'t DatabaseTransaction) -> BoxFuture<'t, AppResult<uuid::Uuid>> + Send + Sync>;
type RunFn = Arc<dyn Fn(AppState) -> BoxFuture<'static, AppResult<()>> + Send + Sync>;

/// What a slot does.
#[derive(Clone)]
pub(crate) enum Action {
    /// Enqueues a job in the claim's transaction.
    Job { kind: &'static str, dispatch: Dispatch, register: fn(&mut Jobs) },
    /// Runs in the process.
    Run(RunFn),
}

/// A schedule: a name, when it fires, and what it does.
#[derive(Clone)]
pub struct Schedule {
    pub(crate) name: String,
    pub(crate) rule: Rule,
    pub(crate) action: Option<Action>,
    pub(crate) without_overlapping: bool,
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
        Self { name: name.to_owned(), rule: Rule::Every(period.as_secs()), action: None, without_overlapping: false }
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
        let dispatch: Dispatch = Arc::new(move |txn| {
            let job = make();
            async move { crate::queue::dispatch(txn, &job).await }.boxed()
        });
        self.action = Some(Action::Job { kind: J::KIND, dispatch, register: |jobs| jobs.register::<J>() });
        self
    }

    /// Runs `f` in the process for each slot: best effort, at most once per
    /// claimed slot (a crash during the run loses it). On one instance a run
    /// never overlaps the previous one: that slot is skipped.
    #[must_use]
    pub fn run<F, Fut>(mut self, f: F) -> Self
    where
        F: Fn(AppState) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = AppResult<()>> + Send + 'static,
    {
        self.action = Some(Action::Run(Arc::new(move |state| f(state).boxed())));
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
        assert!(!name.starts_with(RESERVED), "schedule `{name}`: names starting with `{RESERVED}` are reserved");
        assert!(self.0.iter().all(|s| s.name != *name), "schedule `{name}` declared twice");
        match &schedule.action {
            None => panic!("schedule `{name}`: give it a `.job(..)` or a `.run(..)`"),
            Some(Action::Run(_)) => {
                assert!(
                    !schedule.without_overlapping,
                    "schedule `{name}`: `without_overlapping` is for `.job` schedules"
                );
            }
            Some(Action::Job { register, .. }) => register(jobs),
        }
        self.0.push(schedule);
    }

    /// Every schedule.
    pub fn all(&self) -> &[Schedule] {
        &self.0
    }

    pub(crate) fn get(&self, name: &str) -> Option<&Schedule> {
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
}

impl Default for ScheduleConfig {
    fn default() -> Self {
        Self { enabled: true, grace_secs: 30 }
    }
}

impl ScheduleConfig {
    /// The `[schedule]` section, validated.
    pub fn of(config: &crate::Config) -> anyhow::Result<Self> {
        let section: Self = config.section("schedule").unwrap_or_default();
        anyhow::ensure!((1..=300).contains(&section.grace_secs), "schedule.grace_secs must be 1 to 300");
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
        }
    }

    /// Claims `slot` of the registered schedule `name` now.
    pub async fn claim(state: &AppState, name: &str, slot: DateTime<Utc>, grace: Duration) -> AppResult<Claimed> {
        let schedules = state.service::<Schedules>().expect("schedules");
        let schedule = schedules.get(name).expect("a registered schedule").clone();
        claim::ensure_state(state.db(), name).await?;
        claim::claim(state, &schedule, slot, grace, &SystemClock, &uuid::Uuid::now_v7().to_string()).await
    }

    /// Prunes claims older than `before`.
    pub async fn prune(state: &AppState, before: DateTime<Utc>) -> AppResult<u64> {
        claim::prune(state.db(), before).await
    }
}
