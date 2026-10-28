# Tables

`table()` describes the list page. Without it, the first fields are columns.

```rust
fn table() -> Table {
    Table::new()
        .columns(vec![
            TextColumn::make("title"),
            BadgeColumn::make("status").color("published", "success"),
            BooleanColumn::make("featured"),
            DateColumn::make("published_at"),
        ])
        .filters(vec![
            TernaryFilter::make("featured"),
            SelectFilter::make("category_id"),
            DateRangeFilter::make("published_at"),
        ])
        .default_sort("-published_at")
}
```

| Columns | |
|---|---|
| `TextColumn` | text, numbers, relationship titles |
| `BadgeColumn` | a badge, `.color(value, color)` per value (`primary`, `success`, `warning`, `danger`, `gray`; any other name shows gray) |
| `BooleanColumn` | a check mark |
| `DateColumn` | dates and times |

| Filters | Field must be `.filterable()` |
|---|---|
| `SelectFilter` | one of the options: `.options([(value, "Label"), …])`, or the `belongs_to` records |
| `TernaryFilter` | yes / no / any |
| `DateRangeFilter` | from / until |

Columns and filters take `.label("…")` (default: the field's label, or its
name in words). A column can be drawn by a script of the application with
`.cell("vendor/name")` and `.props(json!(…))`
([custom cells](customizing.md#custom-cells)).

Columns and filters for fields the user may not read are left out.

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

## Search and sorting

Search covers the `.searchable()` fields (case-insensitive; on SQLite only for
ASCII). Columns of `.sortable()` fields can be sorted; `default_sort("-field")`
sorts descending. Search, sort, filters and the page are kept in the URL.

Lists only show records the user may view: `scope()` and the policy apply
([Authorization](authorization.md)). Rows offer the resource's
[actions](actions.md#placement-in-lists); bulk ones also appear when rows are
selected.
