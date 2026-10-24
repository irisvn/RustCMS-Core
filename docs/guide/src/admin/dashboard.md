# Dashboard

The panel's home page shows **widgets** above the cards of the resources:
counts, charts and the latest records of a resource, each computed by the
list engine for the user looking at it — their scope, the resource's policy
(record by record when it decides so), its filters and field rules — and
never cached.

```rust,ignore
use rustcms::admin::dashboard::{Bucket, Dashboard, Widget};
use rustcms::admin::resource::FilterOp;

AdminProvider::new().dashboard(
    Dashboard::new()
        .widget(Widget::stat("learners", "learners").label("Learners"))
        .widget(
            Widget::stat("new_enrollments", "enrollments")
                .label("New enrollments")
                .compare_last_days(30, "enrolled_at"),
        )
        .widget(
            Widget::chart("enrollments_per_week", "enrollments", "enrolled_at")
                .per(Bucket::Week)
                .last(12)
                .span(2),
        )
        .widget(
            Widget::recent("new_courses", "courses")
                .columns(&["capacity"])
                .sort("-created_at")
                .limit(5),
        )
        .widget(Widget::stat("published", "posts").filter("published", FilterOp::Eq, json!(true))),
)
```

| Widget | Shows |
|---|---|
| `Widget::stat(name, resource)` | how many records the user may list; with `compare_last_days(days, date)`, those whose `date` is in the last `days`, against the `days` before |
| `Widget::chart(name, resource, date)` | records per `Bucket::Day`, `Week` (ISO, from Monday) or `Month`, the `last(n)` buckets (1–60), UTC |
| `Widget::recent(name, resource)` | the first `limit` (1–20) records of the list in `sort` order, with `columns`, shown as the resource shows them (option labels, the table column's custom cell or badge, dates in the user's language, related records by title) |

Every widget may take `.label()` (translated like other labels; default: the
name in words), `.span(1–4)` (grid columns), and `.filter(field, op, value)`
(only the resource's declared filters; several combine with AND).
`Dashboard::resource_links(false)` hides the resource cards.

**Who sees a widget.** Its permission (`dashboard.<name>`, declared for
`permission:sync`; change it with `.permission()`), *and* listing the resource
(`view_any`), *and* reading every field it uses (filters, date, columns,
sort). Otherwise it is not listed, and its data answers 403. Revoking any of
them applies on the next request.

**Counts.** When the policy decides once for every record, the database
counts the list's own query (its scope's joins and `DISTINCT` hold). When it
decides per record — and for dated counts on SQLite, whose dates are text —
each candidate is read back and checked, at most 10,000; beyond, the widget
says *too many records* instead of a wrong number: narrow it with a filter.
Trashed records are never counted. Over a date-only field, periods are whole
UTC days: "the last 30 days" are today and the 29 days before.

Widgets are checked at boot: a name used twice, an unknown resource, a filter
that is not declared, a date that is not a date field, a column or sort that
is not a field, or a size out of range stops the boot.

## API

| Method | Path | Answer |
|---|---|---|
| GET | `/admin/api/dashboard` | `{widgets: [{name, kind, label, span, resource, bucket?}], resourceLinks}` (the user's) |
| GET | `/admin/api/dashboard/widgets/{name}` | `{kind: "stat", value, previous?}`, `{kind: "chart", buckets: [{start, count}]}`, `{kind: "recent", columns, records}` or `{kind: "too_many"}`; 403 without access, 404 for an unknown name |

Counts are integer strings. A user may load 60 widgets a minute; each
process computes four at a time (others wait their turn, up to ten seconds).
