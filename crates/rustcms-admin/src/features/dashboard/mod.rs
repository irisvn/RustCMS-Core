//! The dashboard (ADR 0024): built-in widgets counting a resource's records —
//! [`Widget::stat`], [`Widget::chart`] — and listing its latest
//! ([`Widget::recent`]), each with its own permission (`dashboard.<name>`),
//! shown only to users who may also list the resource and read every field
//! it uses, computed by the list engine (scope, policy, field rules), never
//! cached.
//!
//! ```rust,ignore
//! AdminProvider::new().dashboard(
//!     Dashboard::new()
//!         .widget(Widget::stat("learners", "learners").label("Learners").compare_last_days(30, "created_at"))
//!         .widget(Widget::chart("enrollments", "enrollments", "enrolled_at").per(Bucket::Week).last(12).span(2))
//!         .widget(Widget::recent("new_courses", "courses").columns(&["title", "created_at"]).sort("-created_at")),
//! )
//! ```

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::extract::{Path, State};
use axum::http::Method;
use axum::routing::get;
use chrono::{DateTime, Datelike, Months, Utc};
use rustcms_core::{AppError, AppResult, AppState, Json};
use serde::Serialize;

use crate::context::Ctx;
use crate::http::{AdminCtx, AdminPanel};
use crate::registry::{Counted, Registry};
use crate::resource::{Filter, FilterOp, ListQuery};

/// Longest chart.
pub const MAX_BUCKETS: u32 = 60;
/// Most records of a recent list.
pub const MAX_RECENT: u64 = 20;
/// Widget data requests per user per minute.
const REQUESTS_PER_MINUTE: u32 = 60;
/// Widgets computed at once (per process); others wait their turn.
static COMPUTING: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(4);
/// Longest wait for a turn.
const WAIT: Duration = Duration::from_secs(10);

/// Buckets of a chart (UTC).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Bucket {
    /// Days.
    Day,
    /// ISO weeks (from Monday).
    Week,
    /// Calendar months.
    Month,
}

#[derive(Debug, Clone)]
enum Kind {
    Stat { compare: Option<(i64, String)> },
    Chart { date: String, bucket: Bucket, last: u32 },
    Recent { columns: Vec<String>, limit: u64 },
}

/// A dashboard widget over one resource.
#[derive(Debug, Clone)]
pub struct Widget {
    name: String,
    resource: String,
    label: Option<String>,
    span: u8,
    permission: Option<String>,
    filters: Vec<Filter>,
    sort: Option<String>,
    kind: Kind,
}

impl Widget {
    fn new(name: &str, resource: &str, kind: Kind) -> Self {
        Self {
            name: name.to_owned(),
            resource: resource.to_owned(),
            label: None,
            span: 1,
            permission: None,
            filters: Vec::new(),
            sort: None,
            kind,
        }
    }

    /// How many records of `resource` the user may list.
    pub fn stat(name: &str, resource: &str) -> Self {
        Self::new(name, resource, Kind::Stat { compare: None })
    }

    /// Records per `bucket` (`per`) by the date field `date`, the last ones.
    pub fn chart(name: &str, resource: &str, date: &str) -> Self {
        Self::new(name, resource, Kind::Chart { date: date.to_owned(), bucket: Bucket::Day, last: 14 })
    }

    /// The latest records, by `sort` (default: the table's).
    pub fn recent(name: &str, resource: &str) -> Self {
        Self::new(name, resource, Kind::Recent { columns: Vec::new(), limit: 5 })
    }

    /// Title (translated in the user's language).
    pub fn label(mut self, label: &str) -> Self {
        self.label = Some(label.to_owned());
        self
    }

    /// Columns of the dashboard grid it takes (1 to 4).
    pub fn span(mut self, span: u8) -> Self {
        self.span = span;
        self
    }

    /// The permission to see it (default `dashboard.<name>`).
    pub fn permission(mut self, permission: &str) -> Self {
        self.permission = Some(permission.to_owned());
        self
    }

    /// Only records passing this declared filter of the resource (several: AND).
    pub fn filter(mut self, field: &str, op: FilterOp, value: serde_json::Value) -> Self {
        self.filters.push(Filter { field: field.to_owned(), op, value });
        self
    }

    /// Stat: records whose `date` is in the last `days`, against the `days` before.
    pub fn compare_last_days(mut self, days: i64, date: &str) -> Self {
        if let Kind::Stat { compare } = &mut self.kind {
            *compare = Some((days, date.to_owned()));
        }
        self
    }

    /// Chart: the size of its buckets.
    pub fn per(mut self, bucket: Bucket) -> Self {
        if let Kind::Chart { bucket: b, .. } = &mut self.kind {
            *b = bucket;
        }
        self
    }

    /// Chart: how many buckets (1 to 60).
    pub fn last(mut self, buckets: u32) -> Self {
        if let Kind::Chart { last, .. } = &mut self.kind {
            *last = buckets;
        }
        self
    }

    /// Recent: the fields shown (besides the title).
    pub fn columns(mut self, columns: &[&str]) -> Self {
        if let Kind::Recent { columns: c, .. } = &mut self.kind {
            *c = columns.iter().map(|c| (*c).to_owned()).collect();
        }
        self
    }

    /// Recent: how many records (1 to 20).
    pub fn limit(mut self, limit: u64) -> Self {
        if let Kind::Recent { limit: l, .. } = &mut self.kind {
            *l = limit;
        }
        self
    }

    /// Recent: the order (`-created_at`), a sortable field.
    pub fn sort(mut self, sort: &str) -> Self {
        self.sort = Some(sort.to_owned());
        self
    }

    /// Its title in English: the label, else the name in words.
    fn title(&self) -> String {
        self.label.clone().unwrap_or_else(|| crate::ui::humanize(&self.name))
    }

    /// Its permission.
    pub fn required_permission(&self) -> String {
        self.permission.clone().unwrap_or_else(|| format!("dashboard.{}", self.name))
    }

    fn query(&self, per_page: Option<u64>) -> ListQuery {
        ListQuery {
            filters: self.filters.clone(),
            sort: self.sort.clone(),
            per_page,
            page: Some(1),
            ..Default::default()
        }
    }

    fn date(&self) -> Option<&str> {
        match &self.kind {
            Kind::Stat { compare } => compare.as_ref().map(|(_, d)| d.as_str()),
            Kind::Chart { date, .. } => Some(date),
            Kind::Recent { .. } => None,
        }
    }

    /// Every field it reads (filters, date, columns, sort).
    fn fields(&self) -> Vec<String> {
        let mut out: Vec<String> = self.filters.iter().map(|f| f.field.clone()).collect();
        out.extend(self.date().map(str::to_owned));
        if let Kind::Recent { columns, .. } = &self.kind {
            out.extend(columns.iter().cloned());
        }
        out.extend(self.sort.as_deref().map(|s| s.trim_start_matches('-').to_owned()));
        out
    }

    fn kind_name(&self) -> &'static str {
        match self.kind {
            Kind::Stat { .. } => "stat",
            Kind::Chart { .. } => "chart",
            Kind::Recent { .. } => "recent",
        }
    }
}

/// The dashboard: widgets, then the cards of the resources (unless turned off).
#[derive(Debug, Clone)]
pub struct Dashboard {
    widgets: Vec<Widget>,
    resource_links: bool,
}

impl Default for Dashboard {
    fn default() -> Self {
        Self { widgets: Vec::new(), resource_links: true }
    }
}

impl Dashboard {
    /// An empty dashboard (the resource cards only).
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a widget.
    pub fn widget(mut self, widget: Widget) -> Self {
        self.widgets.push(widget);
        self
    }

    /// Whether the cards of the resources are shown under the widgets.
    pub fn resource_links(mut self, shown: bool) -> Self {
        self.resource_links = shown;
        self
    }

    /// Labels shown (for `lang:missing`).
    pub(crate) fn display_strings(&self) -> Vec<String> {
        self.widgets.iter().map(Widget::title).collect()
    }

    /// The permissions of its widgets.
    pub(crate) fn permissions(&self) -> Vec<String> {
        self.widgets.iter().map(Widget::required_permission).collect()
    }

    /// Checks every widget against `registry` (static: no user).
    ///
    /// # Panics
    /// On a bad name, a name used twice, an unknown resource, a filter that is
    /// not declared, a field that is not a date, a column or sort that is not a
    /// field, or a size out of range.
    pub(crate) fn check(&self, registry: &Registry) {
        let mut names = HashSet::new();
        for widget in &self.widgets {
            let name = &widget.name;
            let valid = (1..=64).contains(&name.len())
                && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
            assert!(valid, "dashboard widget `{name}`: names are 1–64 of `a-z`, `0-9`, `_`");
            assert!(names.insert(name.clone()), "dashboard widget `{name}` declared twice");
            assert!((1..=4).contains(&widget.span), "dashboard widget `{name}`: span is 1 to 4");
            let resource = registry
                .get(&widget.resource)
                .unwrap_or_else(|| panic!("dashboard widget `{name}`: no resource `{}`", widget.resource));
            match &widget.kind {
                Kind::Chart { last, .. } => {
                    assert!((1..=MAX_BUCKETS).contains(last), "dashboard widget `{name}`: 1 to {MAX_BUCKETS} buckets");
                }
                Kind::Recent { limit, .. } => {
                    assert!((1..=MAX_RECENT).contains(limit), "dashboard widget `{name}`: 1 to {MAX_RECENT} records");
                }
                Kind::Stat { compare } => {
                    assert!(
                        compare.as_ref().is_none_or(|(d, _)| (1..=3_660).contains(d)),
                        "dashboard widget `{name}`: 1 to 3660 days"
                    );
                }
            }
            let columns = match &widget.kind {
                Kind::Recent { columns, .. } => columns.clone(),
                _ => Vec::new(),
            };
            if let Some(problem) = resource.widget_problem(&widget.query(None), widget.date(), &columns) {
                panic!("dashboard widget `{name}`: {problem}");
            }
        }
    }
}

/// Every dashboard route as `(method, path)`.
pub(crate) fn routes(admin_path: &str) -> Vec<(Method, String)> {
    let base = format!("{}/api", admin_path.trim_end_matches('/'));
    vec![(Method::GET, format!("{base}/dashboard")), (Method::GET, format!("{base}/dashboard/widgets/{{name}}"))]
}

pub(crate) fn router(router: Router<AppState>, admin_path: &str) -> Router<AppState> {
    let base = format!("{}/api", admin_path.trim_end_matches('/'));
    router
        .route(&format!("{base}/dashboard"), get(show))
        .route(&format!("{base}/dashboard/widgets/{{name}}"), get(data))
}

/// A widget the user may see (`GET dashboard`).
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct WidgetInfo {
    /// Name (`GET dashboard/widgets/{name}`).
    pub name: String,
    /// `stat`, `chart` or `recent`.
    pub kind: &'static str,
    /// Title in the user's language.
    pub label: String,
    /// Grid columns taken (1 to 4).
    pub span: u8,
    /// The resource it counts or lists.
    pub resource: String,
    /// Charts: their buckets.
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bucket: Option<Bucket>,
}

/// `GET dashboard`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct DashboardView {
    /// The widgets the user may see, in order.
    pub widgets: Vec<WidgetInfo>,
    /// Show the resource cards.
    pub resource_links: bool,
}

/// One bucket of a chart.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChartBucket {
    /// First day (`YYYY-MM-DD`, UTC).
    pub start: String,
    /// Records (an integer string, ADR 0003).
    pub count: String,
}

/// A column of a recent list.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct RecentColumn {
    /// Field.
    pub name: String,
    /// Label in the user's language.
    pub label: String,
    /// Wire kind of the value (`instant`, `date`, …).
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value_type: Option<&'static str>,
    /// Labels of the field's static options (its form input's), in the user's language.
    #[cfg_attr(feature = "ts", ts(as = "Option<Vec<crate::ui::SelectOption>>", optional))]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<crate::ui::SelectOption>,
    /// The resource table's column of the field (type, badge colors, custom cell), when it has one.
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<crate::schema::ColumnSchema>,
}

/// A record of a recent list.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct RecentRecord {
    /// Key (`/r/{resource}/records/{key}`).
    pub key: String,
    /// Title.
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Text of each column (readable fields only; related records by title).
    pub values: BTreeMap<String, String>,
    /// The value of each column, in wire format (readable fields only), for
    /// option labels, custom cells and dates.
    #[cfg_attr(feature = "ts", ts(type = "Record<string, JsonValue>"))]
    pub data: BTreeMap<String, serde_json::Value>,
}

/// `GET dashboard/widgets/{name}`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WidgetData {
    /// A count (with the previous period when compared).
    Stat {
        /// Records (an integer string).
        value: String,
        /// The period before (compared stats).
        #[cfg_attr(feature = "ts", ts(optional))]
        #[serde(skip_serializing_if = "Option::is_none")]
        previous: Option<String>,
    },
    /// Counts per bucket, oldest first.
    Chart {
        /// Buckets.
        buckets: Vec<ChartBucket>,
    },
    /// The latest records.
    Recent {
        /// Columns shown.
        columns: Vec<RecentColumn>,
        /// Records.
        records: Vec<RecentRecord>,
    },
    /// Too many records to check one by one: narrow the widget's filters.
    TooMany,
}

fn panel(app: &AppState) -> AppResult<Arc<AdminPanel>> {
    app.service::<AdminPanel>().ok_or_else(|| AppError::internal("AdminProvider not registered"))
}

/// The widget if the user may see it now: its permission, listing its
/// resource, reading every field it uses (403 otherwise).
async fn admitted(panel: &AdminPanel, ctx: &Ctx, widget: &Widget) -> AppResult<Arc<dyn crate::registry::DynResource>> {
    if !ctx.can(&widget.required_permission()) {
        return Err(AppError::Forbidden);
    }
    let resource = panel.registry.get(&widget.resource).cloned().ok_or(AppError::Forbidden)?;
    resource.admit_list(ctx, &widget.query(None)).await?;
    let fields = widget.fields();
    let names: Vec<&str> = fields.iter().map(String::as_str).collect();
    if !resource.reads_all(ctx, &names) {
        return Err(AppError::Forbidden);
    }
    Ok(resource)
}

/// `GET dashboard`: the widgets the user may see.
async fn show(State(app): State<AppState>, AdminCtx(ctx): AdminCtx) -> AppResult<Json<DashboardView>> {
    let panel = panel(&app)?;
    let mut widgets = Vec::new();
    for widget in &panel.dashboard.widgets {
        if admitted(&panel, &ctx, widget).await.is_err() {
            continue;
        }
        let bucket = match &widget.kind {
            Kind::Chart { bucket, .. } => Some(*bucket),
            _ => None,
        };
        widgets.push(WidgetInfo {
            name: widget.name.clone(),
            kind: widget.kind_name(),
            label: ctx.t(&widget.title()),
            span: widget.span,
            resource: widget.resource.clone(),
            bucket,
        });
    }
    Ok(Json(DashboardView { widgets, resource_links: panel.dashboard.resource_links }))
}

/// The start of the bucket holding `at`.
fn bucket_start(bucket: Bucket, at: DateTime<Utc>) -> DateTime<Utc> {
    let day = at.date_naive();
    let first = match bucket {
        Bucket::Day => day,
        Bucket::Week => day - chrono::Duration::days(i64::from(day.weekday().num_days_from_monday())),
        Bucket::Month => day.with_day(1).unwrap_or(day),
    };
    first.and_hms_opt(0, 0, 0).unwrap_or_default().and_utc()
}

fn previous(bucket: Bucket, start: DateTime<Utc>) -> DateTime<Utc> {
    match bucket {
        Bucket::Day => start - chrono::Duration::days(1),
        Bucket::Week => start - chrono::Duration::days(7),
        Bucket::Month => start.checked_sub_months(Months::new(1)).unwrap_or(start),
    }
}

/// The `last` buckets up to the one holding `now`, oldest first.
pub(crate) fn buckets(bucket: Bucket, last: u32, now: DateTime<Utc>) -> Vec<(DateTime<Utc>, DateTime<Utc>)> {
    let current = bucket_start(bucket, now);
    let mut starts = vec![current];
    for _ in 1..last {
        let earlier = previous(bucket, *starts.last().unwrap_or(&current));
        starts.push(earlier);
    }
    starts.reverse();
    let ends = starts.iter().skip(1).copied().chain(std::iter::once(match bucket {
        Bucket::Day => current + chrono::Duration::days(1),
        Bucket::Week => current + chrono::Duration::days(7),
        Bucket::Month => current.checked_add_months(Months::new(1)).unwrap_or(current),
    }));
    starts.iter().copied().zip(ends).collect()
}

/// A record's value as text: related records by title, `—` when empty.
fn text(value: Option<&serde_json::Value>, label: Option<&String>) -> String {
    if let Some(label) = label {
        return label.clone();
    }
    match value {
        None | Some(serde_json::Value::Null) => "—".into(),
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    }
}

/// How the column `name` of a recent list is shown: its kind, the options of
/// its form input and its table column (from the user's view schema).
fn recent_column(
    ctx: &Ctx,
    resource: &dyn crate::registry::DynResource,
    schema: Option<&crate::schema::ResourceSchema>,
    name: &str,
) -> RecentColumn {
    fn input<'a>(
        components: &'a [crate::schema::ComponentSchema],
        name: &str,
    ) -> Option<&'a crate::schema::InputSchema> {
        components.iter().find_map(|c| match c {
            crate::schema::ComponentSchema::Input(i) if i.name == name => Some(i),
            crate::schema::ComponentSchema::Input(_) => None,
            crate::schema::ComponentSchema::Layout(l) => input(&l.components, name),
        })
    }
    let column = schema.and_then(|s| s.table.columns.iter().find(|c| c.name == name)).cloned();
    let options = schema
        .and_then(|s| input(&s.form.components, name))
        .filter(|i| i.relationship.is_none())
        .map(|i| i.options.clone())
        .unwrap_or_default();
    RecentColumn {
        name: name.to_owned(),
        label: resource.field_label(ctx, name),
        value_type: resource.field_link(name).map(|(_, kind)| kind),
        options,
        column,
    }
}

/// `GET dashboard/widgets/{name}`: its data, computed now.
async fn data(
    State(app): State<AppState>,
    AdminCtx(ctx): AdminCtx,
    Path(name): Path<String>,
) -> AppResult<Json<WidgetData>> {
    let panel = panel(&app)?;
    let widget = panel.dashboard.widgets.iter().find(|w| w.name == name).ok_or(AppError::NotFound)?;
    let resource = admitted(&panel, &ctx, widget).await?;
    let auth =
        app.service::<rustcms_auth::AuthState>().ok_or_else(|| AppError::internal("AuthProvider not registered"))?;
    auth.limiter.reserve(&[(&format!("dashboard:{}", ctx.principal.id), REQUESTS_PER_MINUTE)]).await?;
    // A bounded wait for a turn, after admission and before any query.
    let _turn = tokio::time::timeout(WAIT, COMPUTING.acquire())
        .await
        .map_err(|_| AppError::TooManyRequests { retry_after: 5 })?
        .map_err(AppError::internal)?;
    let now = Utc::now();
    let data = match &widget.kind {
        Kind::Stat { compare: None } => match resource.counted(&ctx, &widget.query(None), None, &[]).await? {
            Counted::Counts { total, .. } => WidgetData::Stat { value: total.to_string(), previous: None },
            Counted::TooMany => WidgetData::TooMany,
        },
        Kind::Stat { compare: Some((days, date)) } => {
            let period = chrono::Duration::days(*days);
            let ranges = [(now - period, now), (now - period - period, now - period)];
            match resource.counted(&ctx, &widget.query(None), Some(date), &ranges).await? {
                Counted::Counts { ranges, .. } => {
                    WidgetData::Stat { value: ranges[0].to_string(), previous: Some(ranges[1].to_string()) }
                }
                Counted::TooMany => WidgetData::TooMany,
            }
        }
        Kind::Chart { date, bucket, last } => {
            let ranges = buckets(*bucket, *last, now);
            match resource.counted(&ctx, &widget.query(None), Some(date), &ranges).await? {
                Counted::Counts { ranges: counts, .. } => WidgetData::Chart {
                    buckets: ranges
                        .iter()
                        .zip(counts)
                        .map(|((start, _), count)| ChartBucket {
                            start: start.format("%Y-%m-%d").to_string(),
                            count: count.to_string(),
                        })
                        .collect(),
                },
                Counted::TooMany => WidgetData::TooMany,
            }
        }
        Kind::Recent { columns, limit } => match resource.list(&ctx, &widget.query(Some(*limit))).await {
            Ok(page) => {
                // Shown as the resource shows them: its table's columns and its form's options.
                let schema = resource.schema(&ctx, crate::schema::SchemaOperation::View, None).await.ok();
                WidgetData::Recent {
                    columns: columns
                        .iter()
                        .map(|c| recent_column(&ctx, resource.as_ref(), schema.as_ref(), c))
                        .collect(),
                    records: page
                        .data
                        .into_iter()
                        .map(|envelope| RecentRecord {
                            values: columns
                                .iter()
                                .map(|c| (c.clone(), text(envelope.data.get(c), envelope.labels.get(c))))
                                .collect(),
                            data: columns
                                .iter()
                                .filter_map(|c| envelope.data.get(c).map(|v| (c.clone(), v.clone())))
                                .collect(),
                            key: envelope.key,
                            title: envelope.title,
                        })
                        .collect(),
                }
            }
            // The list's scan limit (its stable code, not its text).
            Err(AppError::Validation(errors))
                if errors.form_errors.iter().any(|m| rustcms_core::i18n::Issue::of(m).code == "too_many_matches") =>
            {
                WidgetData::TooMany
            }
            Err(err) => return Err(err),
        },
    };
    Ok(Json(data))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    #[test]
    fn buckets_are_aligned_and_contiguous() {
        let now = at("2026-01-07T15:00:00Z"); // a Wednesday
        let weeks = buckets(Bucket::Week, 3, now);
        let days: Vec<String> = weeks.iter().map(|(s, _)| s.format("%Y-%m-%d").to_string()).collect();
        assert_eq!(days, ["2025-12-22", "2025-12-29", "2026-01-05"], "ISO weeks across the year");
        assert!(weeks.windows(2).all(|w| w[0].1 == w[1].0));
        assert_eq!(weeks[2].1, at("2026-01-12T00:00:00Z"));
        let months = buckets(Bucket::Month, 3, at("2026-03-31T23:59:59Z"));
        let firsts: Vec<String> = months.iter().map(|(s, _)| s.format("%Y-%m-%d").to_string()).collect();
        assert_eq!(firsts, ["2026-01-01", "2026-02-01", "2026-03-01"]);
        assert_eq!(months[2].1, at("2026-04-01T00:00:00Z"));
        let days = buckets(Bucket::Day, 2, at("2026-03-01T00:00:00Z"));
        assert_eq!(days[0].0, at("2026-02-28T00:00:00Z"));
        assert_eq!(days[1], (at("2026-03-01T00:00:00Z"), at("2026-03-02T00:00:00Z")));
    }
}
