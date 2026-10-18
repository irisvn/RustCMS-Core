//! Counts for dashboard widgets (ADR 0024): the list's own rows (scope,
//! filters, trash state, policy), counted by the database on the list's
//! query (a subquery keeps its joins and `DISTINCT`) when the policy decides
//! once for every record; otherwise — a policy deciding per record, or dated
//! counts on SQLite (whose dates are text) — every candidate is read back and
//! counted here, within [`MAX_LIST_SCAN`] (beyond: [`Counted::TooMany`]).

use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};

use super::*;

/// What a count found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Counted {
    /// Every row, and the rows whose date falls in each range.
    Counts {
        /// All rows.
        total: u64,
        /// One per range asked for, in order.
        ranges: Vec<u64>,
    },
    /// More candidates than can be checked one by one.
    TooMany,
}

/// The instant of a wire date value (an instant, a local date-time or a date, as UTC).
fn instant_of(value: &serde_json::Value) -> Option<DateTime<Utc>> {
    let text = value.as_str()?;
    if let Ok(at) = DateTime::parse_from_rfc3339(text) {
        return Some(at.with_timezone(&Utc));
    }
    if let Ok(at) = NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S%.f") {
        return Some(at.and_utc());
    }
    NaiveDate::parse_from_str(text, "%Y-%m-%d").ok().and_then(|d| d.and_hms_opt(0, 0, 0)).map(|d| d.and_utc())
}

/// `at` if it is a midnight (UTC), else the midnight after it.
fn next_midnight(at: DateTime<Utc>) -> DateTime<Utc> {
    let midnight = at.date_naive().and_hms_opt(0, 0, 0).map_or(at, |d| d.and_utc());
    if midnight == at { at } else { midnight + chrono::Duration::days(1) }
}

/// Whether `kind` holds a date (`Instant`, `Date`, `LocalDateTime`).
pub(crate) fn is_date(kind: FieldKind) -> bool {
    matches!(kind, FieldKind::Instant | FieldKind::Date | FieldKind::LocalDateTime)
}

impl<R: Resource> ResourceAdapter<R> {
    /// What is wrong with a widget over this resource, checked at boot (no user):
    /// filters must be declared filters, the date a date field, the columns
    /// fields, the sort a sortable field.
    pub(super) fn widget_problem(&self, query: &ListQuery, date: Option<&str>, columns: &[String]) -> Option<String> {
        for filter in &query.filters {
            if !self.field(&filter.field).is_some_and(|f| f.filterable) {
                return Some(format!("`{}` is not a declared filter", filter.field));
            }
            let column = self.column(&filter.field);
            let values: Vec<&serde_json::Value> = match (&filter.op, &filter.value) {
                (crate::resource::FilterOp::In, serde_json::Value::Array(items)) => items.iter().collect(),
                (crate::resource::FilterOp::In, _) => return Some(format!("`{}`: `in` takes a list", filter.field)),
                (_, value) => vec![value],
            };
            if let Some(Err(e)) = values.into_iter().map(|v| column.kind.decode(v)).find(Result::is_err) {
                return Some(format!("`{}`: {}", filter.field, e.0));
            }
        }
        if let Some(date) = date {
            match self.columns.get(date) {
                Some(column) if is_date(column.kind) && self.field(date).is_some() => {}
                _ => return Some(format!("`{date}` is not a date field")),
            }
        }
        if let Some(name) = columns.iter().find(|c| self.field(c).is_none()) {
            return Some(format!("`{name}` is not a field"));
        }
        if let Some(name) = columns.iter().find(|c| self.field(c).is_some_and(|f| f.secret.is_some())) {
            return Some(format!("`{name}` is encrypted"));
        }
        if let Some(sort) = &query.sort {
            let name = sort.trim_start_matches('-');
            if !self.field(name).is_some_and(|f| f.sortable) {
                return Some(format!("`{name}` is not a sortable field"));
            }
        }
        None
    }

    /// Whether the user may read every field of `names`.
    pub(super) fn reads_all(&self, ctx: &Ctx, names: &[&str]) -> bool {
        names.iter().all(|name| self.field(name).is_some_and(|f| f.can_read(ctx)))
    }

    /// The bound value of `at` for date column `column`, as its own kind.
    fn date_bound(&self, column: &str, at: DateTime<Utc>) -> AppResult<sea_orm::Value> {
        let column = self.column(column);
        let wire = match column.kind {
            FieldKind::Date => serde_json::json!(at.format("%Y-%m-%d").to_string()),
            FieldKind::LocalDateTime => serde_json::json!(at.naive_utc().format("%Y-%m-%dT%H:%M:%S%.6f").to_string()),
            _ => serde_json::json!(at.to_rfc3339_opts(chrono::SecondsFormat::Micros, true)),
        };
        let mut values = column.kind.decode(&wire).map_err(|e| AppError::internal(e.0))?;
        Ok(values.swap_remove(0))
    }

    /// Counts the rows of `query` the user may list, and among them those whose
    /// `date` falls in each `[start, end)` of `ranges`.
    pub(super) async fn counted(
        &self,
        ctx: &Ctx,
        query: &ListQuery,
        date: Option<&str>,
        ranges: &[(DateTime<Utc>, DateTime<Utc>)],
    ) -> AppResult<Counted> {
        self.admit_list(ctx, query).await?;
        // A date field holds whole days: every bound moves to the next midnight
        // (UTC), so both counting paths agree and "the last 30 days" includes today.
        let day_ranges: Vec<(DateTime<Utc>, DateTime<Utc>)>;
        let ranges = match date.and_then(|d| self.columns.get(d)) {
            Some(column) if column.kind == FieldKind::Date => {
                day_ranges = ranges.iter().map(|(s, e)| (next_midnight(*s), next_midnight(*e))).collect();
                day_ranges.as_slice()
            }
            _ => ranges,
        };
        let select = self.list_select(ctx, query)?;
        let once = !self.policy.view_depends_on_record() && query.trashed == Trashed::Without;
        if once && !self.policy.allows(ctx, ability::VIEW, None).await {
            return Ok(Counted::Counts { total: 0, ranges: vec![0; ranges.len()] });
        }
        let sqlite = ctx.db.get_database_backend() == sea_orm::DbBackend::Sqlite;
        if once && (ranges.is_empty() || !sqlite) {
            // `count` wraps the list's query in a subquery: its joins and `DISTINCT` hold.
            let total = select.clone().count(&ctx.db).await?;
            let mut counts = Vec::with_capacity(ranges.len());
            if let Some(name) = date {
                let column = self.column(name).column;
                for (start, end) in ranges {
                    let (start, end) = (self.date_bound(name, *start)?, self.date_bound(name, *end)?);
                    counts.push(select.clone().filter(column.gte(start)).filter(column.lt(end)).count(&ctx.db).await?);
                }
            }
            return Ok(Counted::Counts { total, ranges: counts });
        }
        let (keys, complete) =
            self.read_candidate_keys(&ctx.db, &select, LIST_SCAN_BATCH, MAX_LIST_SCAN / LIST_SCAN_BATCH).await?;
        if !complete {
            return Ok(Counted::TooMany);
        }
        let mut total = 0;
        let mut counts = vec![0; ranges.len()];
        for chunk in keys.chunks(LOAD_CHUNK) {
            for (_, record) in self.load_ordered(ctx, &select, chunk).await? {
                if !once && !self.listable(ctx, &record).await {
                    continue;
                }
                total += 1;
                let at = date.and_then(|d| record.get(d)).and_then(instant_of);
                if let Some(at) = at {
                    for (count, (start, end)) in counts.iter_mut().zip(ranges) {
                        if at >= *start && at < *end {
                            *count += 1;
                        }
                    }
                }
            }
        }
        Ok(Counted::Counts { total, ranges: counts })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_dates_are_read_as_utc() {
        let at = |s: &str| instant_of(&serde_json::json!(s)).map(|d| d.to_rfc3339());
        assert_eq!(at("2026-10-25T07:30:00Z").as_deref(), Some("2026-10-25T07:30:00+00:00"));
        assert_eq!(at("2026-10-25T09:30:00+02:00").as_deref(), Some("2026-10-25T07:30:00+00:00"));
        assert_eq!(at("2026-10-25T07:30:00.123456").as_deref(), Some("2026-10-25T07:30:00.123456+00:00"));
        assert_eq!(at("2026-10-25").as_deref(), Some("2026-10-25T00:00:00+00:00"));
        assert_eq!(at("nope"), None);
        assert_eq!(instant_of(&serde_json::json!(5)), None);
    }

    #[test]
    fn day_bounds_move_to_the_next_midnight() {
        let at = |s: &str| DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc);
        assert_eq!(next_midnight(at("2026-10-16T15:00:00Z")), at("2026-10-17T00:00:00Z"));
        assert_eq!(next_midnight(at("2026-10-16T00:00:00Z")), at("2026-10-16T00:00:00Z"));
    }
}
