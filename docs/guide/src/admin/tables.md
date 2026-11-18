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
| `TextColumn` | text, numbers, related records by title (`belongs_to` fields), [option labels](#option-labels) |
| `BadgeColumn` | a badge, `.color(value, color)` per value (`primary`, `info`, `success`, `warning`, `danger`, `gray`; any other name shows gray), [option labels](#option-labels); a list (a checkbox list's values) as one badge per value |
| `BooleanColumn` | a check mark |
| `DateColumn` | dates and times: instants in the user's language and time zone; dates as `YYYY-MM-DD`, local date-times as `YYYY-MM-DD HH:MM`; or with a [date format](#date-formats) |

Every column takes `.label("…")` (default: the field's label, or its name in
words), `.hide_below(…)` ([narrow screens](#narrow-screens)), and
`.cell("vendor/name")` with `.props(json!(…))` to be drawn by a script of the
application ([custom cells](customizing.md#custom-cells)).

```rust
// A sketch: a `status` text field.
BadgeColumn::make("status").color("published", "success").color("draft", "gray")
```

Values in `.color(…)` are compared as the API sends them: an `i64` or a
decimal column's values are strings (`.color("2", "danger")`), as a select's
options. A value the column cannot hold refuses to boot.

### Option labels

A text or badge column shows the label of each value instead of the value
(`Draft` for `draft`). Without labels of its own, the column takes the options
of the form's select or checkbox list of the same field. The demo's tags show
their kind this way, colored by value:

<<< @/../../../examples/demo/src/admin/resources/tag_resource.rs#table [src/admin/resources/tag_resource.rs]

`.options([(value, label), …])` gives a column its own labels, over the
form's: `BadgeColumn::make("level").options([("1", "Minor"), ("2", "Major")])`
for an `i64` field (its values are strings on the wire). `.raw()` keeps the
values: `TextColumn::make("code").raw()`.

Labels are translated as other labels. Colors, sorting, search, filters and
exports still use the values, and a value without a label is shown as it is.
A `belongs_to` field's column shows the related record's title and takes no
options.

### Date formats

`.date_format(pattern)` on the column of a date, local date-time or instant
field shows its values with `pattern`; the demo's posts above use
`"MMM d, yyyy, h:mm a"`. Instants are shown in the user's time zone, dates and
local date-times as they are; month and weekday names, and AM/PM, are in the
user's language.

| Pattern | Shows | Example |
|---|---|---|
| `yyyy`, `yy` | the year | `2026`, `26` |
| `MMMM`, `MMM`, `MM`, `M` | the month: name, short name, two digits, digits | `November`, `Nov`, `11`, `11` |
| `dd`, `d` | the day | `08`, `8` |
| `EEEE`, `EEE` | the weekday | `Wednesday`, `Wed` |
| `HH`, `H` | the hour, 0 to 23 | `09`, `9` |
| `hh`, `h` | the hour, 1 to 12 | `09`, `9` |
| `mm`, `ss` | minutes, seconds | `05`, `07` |
| `a` | AM or PM | `PM` |
| `z` | the time zone (instants only) | `GMT+7` |
| `'text'` | the text as it is (`''` is a quote) | `'at'` |

Other letters must be quoted; other characters (`/`, `-`, `:`, `,`, spaces)
are kept. A pattern holds at most 64 characters. A pattern that is not one,
a time on a date field, or `z` on a local date-time refuses to boot.

Without a pattern of its own, a column uses the panel's, set once for the
whole panel; history, queue and schedule times use the date-time one too:

```rust
AdminProvider::new()
    .date_format("dd/MM/yyyy")                // date fields
    .datetime_format("HH:mm:ss dd/MM/yyyy")   // local date-times and instants
```

The panel's date-time pattern also writes local date-times, which have no
time zone: it may not hold `z` (a column or entry of an instant may).

Without either, dates show as in the table above. Entries of the view page
take the same patterns ([infolists](infolists.md#entries)).

### Counts

Two columns show a number instead of the value, as Filament's `counts()`:

- `.count()` on the column of a JSON field (a [repeater](forms.md#repeaters),
  a list) shows how many items it holds: `TextColumn::make("faq").count()`.
- `TextColumn::make("posts_count").counts("posts")` shows how many records
  of the has-many relation `posts` each record has. The column is named by
  itself (not a field) and is not sortable. Only records the user may list
  are counted (the related resource's scope, policy and trash), and the
  column is shown only to users who may list the relation. When the related
  resource's policy decides record by record, the page's related records are
  checked one by one, at most 10,000 of them: beyond, the column shows no
  numbers.

The demo's categories count their posts:

<<< @/../../../examples/demo/src/admin/resources/category_resource.rs#table [src/admin/resources/category_resource.rs]

## Filters

| Filter | Field must be `.filterable()` | Sends |
|---|---|---|
| `SelectFilter` | the user picks one of `.options([(value, "Label"), …])`, or a record of the `belongs_to` target (searched in the target) | `filter[field][in]` |
| `TernaryFilter` | yes / no / any | `filter[field][eq]` |
| `DateRangeFilter` | from / until | dates: `filter[field][gte]`, `filter[field][lte]`; date-times: `gte` the start of the first day and `lt` the start of the day after the last (instants in the browser's time zone) |

Filters take `.label("…")`. The panel's select sends one value; an API client
may send several (`filter[status][in]=active,withdrawn`). Several filters
combine with AND.

On a nullable column, `SelectFilter::make("collection").none("Unassigned")`
adds a last choice for records without a value. It is sent as `~none` in the
list (`filter[collection][in]=~none`, or `~none,covers` for either) and
matched as `IS NULL` (in a JSON list too). Without
`.none(..)`, `~none` is compared as any other value.

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
- a filter with `.none(..)` is not a `SelectFilter` on a nullable column, or is
  on a `belongs_to` field;
- the default sort's field is not `.sortable()`;
- a cell name is not `vendor/name` (each part 1 to 32 characters of `a-z`,
  `0-9` and `-`), or its props exceed 4 KiB;
- [option labels](#option-labels) are given to a column that takes none
  (boolean, date, count, related records), together with `.raw()`, twice for a
  value, in values the column cannot hold, or miss a colored value;
- a [date format](#date-formats) is not a pattern, does not fit the field
  (not a date, a time on a date, a zone on a local date-time), or is given to a
  boolean, count or [custom cell](customizing.md#custom-cells) column.

## Related pages

- [Resources](resources.md), [forms](forms.md), [actions](actions.md)
- [Custom cells](customizing.md#custom-cells)
- [Admin API](../reference/admin-api.md)
