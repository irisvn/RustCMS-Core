# Tables

`table()` describes the list page. Without it, the first fields are columns.

```rust,ignore
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
| `BadgeColumn` | a badge, `.color(value, color)` per value |
| `BooleanColumn` | a check mark |
| `DateColumn` | dates and times |

| Filters | Field must be `.filterable()` |
|---|---|
| `SelectFilter` | one of the options (static or the `belongs_to` records) |
| `TernaryFilter` | yes / no / any |
| `DateRangeFilter` | from / until |

Columns and filters for fields the user may not read are left out.

## Search and sorting

Search covers the `.searchable()` fields (case-insensitive; on SQLite only for
ASCII). Columns of `.sortable()` fields can be sorted; `default_sort("-field")`
sorts descending. Search, sort, filters and the page are kept in the URL.

Lists only show records the user may view: `scope()` and the policy apply
([Authorization](authorization.md)). Bulk [actions](actions.md) appear when
rows are selected.
