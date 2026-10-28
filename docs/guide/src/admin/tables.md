# Tables

`table()` describes a resource's list page: its columns, filters and default
sort. Like the form, it is presentation only: which records and fields a
user sees is decided by the scope, the policy and the field rules, on the
server.

![The demo's posts list with its filters open](/img/admin/tables-posts-filters.webp)
*The demo application: the posts list.*

## Quick start

```rust
fn table() -> Table {
    Table::new()
        .columns(vec![TextColumn::make("title"), BooleanColumn::make("published")])
        .filters(vec![TernaryFilter::make("published")])
        .default_sort("title")
}
```

The fields must allow it: a filter needs a `.filterable()` field, a sort a
`.sortable()` field, search a `.searchable()` one.

Without `table()`, the list shows the first six fields that are not
`.guarded()`, encrypted or JSON: a `BooleanColumn` for booleans, a
`DateColumn` for dates and date-times, a `TextColumn` for the rest.

## Full example

The demo's posts: a column hidden on narrow screens, three kinds of filter
and newest first.

<<< @/../../../examples/demo/src/admin/resources/post_resource.rs#table [src/admin/resources/post_resource.rs]

The LMS's enrollments draw their progress with a
[custom cell](customizing.md#custom-cells) and filter by status:

<<< @/../../../examples/lms/src/admin/resources/enrollment_resource.rs#table [src/admin/resources/enrollment_resource.rs]

## Columns

| Column | Shows |
|---|---|
| `TextColumn` | text, numbers, related records by title (`belongs_to` fields) |
| `BadgeColumn` | a badge, `.color(value, color)` per value (`primary`, `success`, `warning`, `danger`, `gray`; any other name shows gray) |
| `BooleanColumn` | a check mark |
| `DateColumn` | dates and times: instants in the user's language and time zone; dates as `YYYY-MM-DD`, local date-times as `YYYY-MM-DD HH:MM` |

Every column takes `.label("…")` (default: the field's label, or its name in
words), `.hide_below(…)` ([narrow screens](#narrow-screens)), and
`.cell("vendor/name")` with `.props(json!(…))` to be drawn by a script of the
application ([custom cells](customizing.md#custom-cells)).

```rust
// A sketch: a `status` text field.
BadgeColumn::make("status").color("published", "success").color("draft", "gray")
```

## Filters

| Filter | Field must be `.filterable()` | Sends |
|---|---|---|
| `SelectFilter` | the user picks one of `.options([(value, "Label"), …])`, or a record of the `belongs_to` target (searched in the target) | `filter[field][in]` |
| `TernaryFilter` | yes / no / any | `filter[field][eq]` |
| `DateRangeFilter` | from / until | dates: `filter[field][gte]`, `filter[field][lte]`; date-times: `gte` the start of the first day and `lt` the start of the day after the last (instants in the browser's time zone) |

Filters take `.label("…")`. The panel's select sends one value; an API client
may send several (`filter[status][in]=active,withdrawn`). Several filters
combine with AND.

## Search, sorting and pages

Search covers the `.searchable()` fields the user may read (case-insensitive;
on SQLite only for ASCII), and is offered only when there is one. Columns of
`.sortable()` fields can be sorted; `default_sort("-field")` sorts
descending. Users pick 10, 25 (the default), 50 or 100 records per page.
Search, sort, filters and the page are kept in the URL.

The same parameters work in the [Admin API](../reference/admin-api.md#lists):

```text
GET /admin/api/resources/posts/records?search=rust&sort=-published_at&page=2&per_page=50
    &filter[published][eq]=true&filter[category_id][in]=1,2
```

Operators are `eq`, `in`, `gt`, `gte`, `lt` and `lte`. Values are read by
field type. `per_page` is clamped to 1–100. A request with more than 10
filters, more than 100 values in an `in` filter or a search over 100
characters answers 422 ([limits](../reference/admin-api.md#limits)).

## Narrow screens

```rust
use rustcms::admin::Breakpoint;

TextColumn::make("category_id").hide_below(Breakpoint::Sm),   // hidden under 640 px
DateColumn::make("published_at").hide_below(Breakpoint::Md),  // hidden under 768 px
TextColumn::make("notes").hide_below(Breakpoint::Lg),         // hidden under 1024 px
```

`hide_below(Breakpoint::Sm | Md | Lg)` drops a column from tables (lists
and relation tables) on narrower screens, with static CSS classes of the
panel's build (no inline style): the records still carry the value, and it
is shown on the record's page. The Media page hides its type, size and date
columns this way.

![The demo's posts list at phone width, without the category column](/img/admin/tables-phone.webp)
*The demo application: the posts list on a phone.*

## Around the table

- **Actions**: rows offer the resource's
  [actions](actions.md#placement-in-lists); bulk ones also appear when rows
  are selected.
- **Trash**: resources with soft deletes have a *Trashed records* selector
  ([the trash](resources.md#the-trash)).
- **Export**: `.exportable()` resources have an Export button that exports
  the list as shown ([export](import-export.md#exporting)).
- **Labels**: column and filter labels and option labels are translated per
  resource ([i18n](i18n.md)).

## What the user sees

Lists only show records the user may view: `scope()` and the policy apply
([Authorization](authorization.md)). Columns and filters of fields the user
may not read are left out, and so is a default sort on such a field.

## Checked at boot

The boot stops when:

- a column is not a field, or is an encrypted field;
- a filter's field is not `.filterable()`;
- the default sort's field is not `.sortable()`;
- a cell name is not `vendor/name` (each part 1 to 32 characters of `a-z`,
  `0-9` and `-`), or its props exceed 4 KiB.

## Related pages

- [Resources](resources.md), [forms](forms.md), [actions](actions.md)
- [Custom cells](customizing.md#custom-cells)
- [Admin API](../reference/admin-api.md)
