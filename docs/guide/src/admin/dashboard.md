# Dashboard

The panel's home page shows **widgets** above the cards of the resources:
counts, charts and the latest records of a resource. Each widget is computed
by the list engine for the user looking at it (their scope, the resource's
policy, its filters and field rules) and is never cached. Use widgets for the
few numbers your team checks every day.

![The LMS dashboard: a compared stat, a weekly chart and the newest courses](/img/admin/dashboard.webp)
*The LMS example: widgets above the resource cards.*

## Quick start

```rust
use rustcms::admin::dashboard::{Dashboard, Widget};

AdminProvider::new()
    .resource::<PostResource>()
    .dashboard(Dashboard::new().widget(Widget::stat("posts", "posts").label("Posts")))
```

Run `rustcms permission:sync`, then give roles the permission
`dashboard.posts` (each widget has its own).

## Full example

The LMS dashboard: a count, a count compared with the previous 30 days, a
weekly chart two columns wide, and the newest courses.

<<< @/../../../examples/lms/src/providers/admin_panel_provider.rs#dashboard [src/providers/admin_panel_provider.rs]

A widget can count only some records with a declared filter of the resource:

```rust
use rustcms::admin::FilterOp;

Widget::stat("published", "posts").label("Published").filter("published", FilterOp::Eq, json!(true))
```

## Widgets

| Widget | Shows |
|---|---|
| `Widget::stat(name, resource)` | how many records the user may list; with `compare_last_days(days, date)` (1 to 3,660 days), those whose `date` is in the last `days`, against the `days` before |
| `Widget::chart(name, resource, date)` | records per `Bucket::Day` (default), `Week` (ISO, from Monday) or `Month`, by the date field `date`, for the `last(n)` buckets (1 to 60, default 14), in UTC |
| `Widget::recent(name, resource)` | the first `limit(n)` records (1 to 20, default 5) of the list in `sort` order (default: the table's `default_sort`), with `columns`, shown as the resource shows them (option labels, the table column's custom cell or badge, dates as the table shows them, related records by title) |

Every widget also takes:

| Method | Default | Does |
|---|---|---|
| `.label(text)` | the name in words | its title, translated like other labels |
| `.span(n)` | 1 | grid columns it takes (1 to 4) |
| `.permission(name)` | `dashboard.<name>` | the permission to see it, declared for `permission:sync` |
| `.filter(field, op, value)` | none | only records passing this declared filter of the resource; several combine with AND |

`Dashboard::resource_links(false)` hides the resource cards. The cards
follow the [navigation order](overview.md#order).

## Who sees a widget

A user sees a widget when they have its permission (`dashboard.<name>`),
*and* may list the resource (`view_any`), *and* may read every field it uses
(filters, date, columns, sort). Otherwise it is not listed, and its data
answers 403. Revoking any of them applies on the next request.

## Counts

When the policy decides once for every record, the database counts the
list's own query (its scope's joins and `DISTINCT` hold). When it decides per
record, and for dated counts on SQLite, whose dates are text, each candidate
is read back and checked, at most 10,000. Beyond that the widget says
*too many records* instead of a wrong number: narrow it with a filter.
Trashed records are never counted. Over a date-only field, periods are whole
UTC days: "the last 30 days" are today and the 29 days before.

## Checked at boot

These stop the boot with a message naming the widget:

- a name that is not 1 to 64 characters of `a-z`, `0-9` and `_`, or a name
  used twice;
- an unknown resource;
- a filter the resource does not declare;
- a date that is not a date field;
- a column or sort that is not a field;
- a span, bucket count, record count or number of days out of range.

## API

| Method | Path | Answer |
|---|---|---|
| GET | `/admin/api/dashboard` | `{widgets: [{name, kind, label, span, resource, bucket?}], resourceLinks}`: the widgets this user may see, in order |
| GET | `/admin/api/dashboard/widgets/{name}` | `{kind: "stat", value, previous?}`, `{kind: "chart", buckets: [{start, count}]}`, `{kind: "recent", columns, records}` or `{kind: "too_many"}` |

- Counts are integer strings. A chart bucket's `start` is its first day,
  `YYYY-MM-DD` in UTC; buckets come oldest first.
- A recent record is `{key, title?, values, data}`: `values` holds the text
  of each column, `data` the values in wire format (readable fields only).
  Columns are `{name, label, valueType?, options?, column?}`.
- An unknown widget name answers 404 before any permission check; a widget
  the user may not see answers 403.
- A user may load 60 widgets a minute (429 beyond). Each process computes
  four widgets at a time; a request that waits more than ten seconds for its
  turn answers 429 with `Retry-After: 5`.

## Related pages

- [Admin panel](overview.md), [tables](tables.md) (filters and sorting)
- [Authorization](authorization.md)
- [Admin API](../reference/admin-api.md)
