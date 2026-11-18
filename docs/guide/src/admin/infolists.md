# Infolists

`infolist()` describes a record's **view page**: which values it shows and
how, in the same sections, grids, tabs and splits as [forms](forms.md). Each
value is shown by an **entry**: a status as a colored badge, a code with a
copy button, a related record as a link, a number computed on the server, or
a component of your own.

Without `infolist()`, the view page shows the [form](forms.md) read-only:
the panel makes an infolist from it (see [The default view
page](#the-default-view-page)). Declared, the infolist **replaces** the form
on the view page; everything around it stays: the header and its actions,
slots, relation managers and the history.

| Builder | Answers | Example |
|---|---|---|
| `fields()` | what data exists, who may read or write it (server) | `Field::column(Code).required()` |
| `table()` | how a field shows in the **list** | `TextColumn::make("code")` |
| `form()` | how a field is **edited** | `SelectInput::make("grade")` |
| `infolist()` | how a field (or a computed value) shows on the **view page** | `TextEntry::make("grade").badge()` |

## Quick start

```rust
fn infolist() -> Infolist {
    Infolist::new().columns(2).schema(components![
        Section::make("Order").schema(components![
            TextEntry::make("number").copyable(),
            TextEntry::make("status").badge().color("paid", "success").color("refunded", "danger"),
            TextEntry::make("placed_at"),
            RelationEntry::make("customer_id"),
        ]),
        ComputedEntry::make("total").number(2),
        CustomEntry::make("route", "acme/map"),
    ])
}
```

The demo's categories show their slug with its prefix, the number of their
posts, and their color drawn by an extension:

<<< @/../../../examples/demo/src/admin/resources/category_resource.rs#infolist [src/admin/resources/category_resource.rs]

Entries are presentation only. An entry for a field the user may not read is
left out of the page (a section left empty disappears with it); entries
never write and grant nothing.

## Entries

An entry names a field of the resource, except `ComputedEntry`, which names
a [computed value](#computed-values). Every entry takes `label`, `help`,
`column_span` and `shown_when`.

| Entry | For | Options |
|---|---|---|
| `TextEntry` | text, numbers, dates, JSON lists | `badge()` with `color(value, color)`, `copyable()`, `limit(chars)`, `url()`, `number(decimals)`, `list()`, `multiline()`, `password()`, `prefix()`, `suffix()`, `options([(value, label)])`, `date_format(pattern)` |
| `RelationEntry` | `belongs_to` fields | none |
| `BooleanEntry` | boolean columns | `labels(yes, no)`, `color(true, color)` |
| `KeyValueEntry` | key/value fields | none |
| `MediaEntry` | media fields | `size(pixels)`, `preview(false)` (images do not open the [image viewer](media.md#the-image-viewer)); `gallery()` is deprecated: images open in the viewer by default |
| `RichTextEntry` | rich text fields | `outline()` |
| `ComputedEntry` | a computed value | the display options of `TextEntry` |
| `CustomEntry` | any field or computed value | `props(json)` |
| `RepeatableEntry` | a JSON field holding rows ([repeaters](forms.md#repeaters)) | `schema(components![…])`: the entries of each row |

- **Dates and times** follow the field's kind: dates in the user's language,
  instants in their time zone. `date_format("d MMMM yyyy")` on the entry of a
  date field, or the panel's pattern, writes them as a
  [date format](tables.md#date-formats) says (not on computed values, row
  keys or panel pages' data, which have no declared kind).
- **Colors** are `primary`, `success`, `warning`, `danger` and `gray` (the
  default), as in [tables](tables.md).
- **`url()`** links only `http` and `https` values; anything else (a
  `javascript:` text a user typed) is shown as text. Links open in a new tab.
- **`limit(chars)`** cuts the text and shows the whole of it as a tooltip.
- **`RelationEntry`** shows the related record's [title](resources.md#titles-from-several-fields),
  linked to its page when the user may list that resource. A related record
  the user may not view shows as `#key`, without a link.

### Rows

`RepeatableEntry` shows each row of a JSON list with its own entries, which
name keys of the row: `TextEntry`, `BooleanEntry`, `KeyValueEntry`,
`CustomEntry`, and a nested `RepeatableEntry` (three deep at most).

```rust
RepeatableEntry::make("faq").schema(components![
    TextEntry::make("question"),
    BooleanEntry::make("pinned").labels("Pinned", "Not pinned"),
])
```

### Conditions

`shown_when` works as in forms (see [Showing inputs under conditions](forms.md#showing-inputs-under-conditions)),
evaluated on the **stored** values. A condition names another entry of the
infolist, or a field of the form (compared with its kind); a section's
condition applies to everything in it.

On the [default view page](#the-default-view-page) (no `infolist()`),
conditions read fields as the edit form does: the value the form would hold
(a write-only toggle is `false`, a field not sent is empty), and `null` for a
field the form hides under its own conditions, so the page shows what the
edit form shows.

```rust
KeyValueEntry::make("attributes").shown_when(when("published").is_true())
```

On a view page, hiding is presentation, not protection: every value the user
may read comes with the record anyway. Protect data with
[field rules](authorization.md), not conditions.

## The default view page

A resource without `infolist()` gets one made from its form, once at boot,
in the form's sections, grids, tabs and splits. Each input becomes the entry
that shows its value:

| Input | Entry |
|---|---|
| `TextInput`, `Textarea`, `NumberInput` | `TextEntry` (line breaks kept; a password input masked; prefix and suffix kept) |
| `SelectInput` with options, `CheckboxList` | `TextEntry` with the option labels (a list for `CheckboxList`) |
| `SelectInput` of a `belongs_to` field | `RelationEntry`: the related record's title, a link when the user may list that resource |
| `Toggle`, `Checkbox` on a boolean column | `BooleanEntry` (yes or no) |
| `DatePicker`, `DateTimePicker` | `TextEntry`: the date in the user's language |
| `KeyValue`, `FileUpload` (media), `RichEditor` | `KeyValueEntry`, `MediaEntry`, `RichTextEntry` |
| `Repeater` (JSON) | `RepeatableEntry`, each row's inputs as entries |
| `CustomInput` | `TextEntry` (a form widget draws no view; declare a `CustomEntry` for one) |
| `Hidden`, `BelongsToManySelect`, secrets, relationship repeaters | nothing (links and child records show in their relation sections; an encrypted field shows nothing, not even whether it is set) |

An input whose entry does not fit its field (a `Toggle` on a number column)
is shown as text. Labels, column spans and conditions come from the inputs;
help texts do not. Fields the user may not read are left out, as everywhere.
To show a value differently, declare an `infolist()`: it replaces this one
as a whole.

## Computed values

A computed value is not a column: the server computes it from the record
each time the view page opens, then a `ComputedEntry` of the same name shows
it.

```rust
fn computed() -> Vec<Computed<order::Entity>> {
    vec![
        Computed::make("total", |ctx, order: &order::Model| {
            Box::pin(async move { Ok(json!(order_service::total(&ctx.db, order.id).await?)) })
        }),
        Computed::make("margin", |ctx, order: &order::Model| {
            Box::pin(async move { Ok(json!(finance::margin(&ctx.db, order.id).await?)) })
        })
        .readable(|ctx| ctx.can("orders.finance"))
        .timeout(Duration::from_secs(15)),
    ]
}
```

- The closure gets the typed model; annotate it (`order: &order::Model`) when
  you read its fields.
- Values run **only** for a user who may view the record, after the policy
  allowed it, and only when its view page opens: never for lists, exports or
  the result of a write. They are never stored.
- The page loads them **alongside** the record (one request for all of them),
  so a slow value does not delay the rest of the page.
- **A failure stays in its entry.** An error, a value over 64 KiB, or one
  that takes longer than its time limit shows "Not available"; the other
  entries are unaffected, and the server logs a warning naming the value.
  The limit is 5 seconds unless `.timeout(…)` says otherwise (at most 30).
- **Restrict what an aggregate discloses.** A total or a count can reveal
  more than the fields it reads. `.readable(…)` hides the value, and skips
  computing it, for users who may not see it. To count related records as
  the user sees them, go through `ctx.records(…)`: their scope and policy
  apply (the demo counts a category's posts that way).
- Keep them light: one query, not one per related row. Each user may open
  60 view pages with computed values a minute and compute 2 at once, and 8
  pages compute at once across users; beyond that the values show "Not
  available" until the next load.
- The time limit stops a value at its next `.await`. Work that blocks a
  thread (CPU loops, blocking I/O) keeps it busy past the limit, and work
  moved to `spawn_blocking` keeps running after the page gave up waiting,
  outside the limits on pages computing at once. Keep computed values to
  short queries; compute anything heavy in a [job](../digging-deeper/queues.md)
  and show the stored result.

## Custom entries

`CustomEntry::make(field, "vendor/name")` draws the value with a component
your extension registers. With the [React SDK](react-pages.md) (`ui` 3):

<<< @/../../../examples/demo/resources/admin/src/demo/react.tsx#swatch-entry [resources/admin/src/demo/react.tsx]

The component gets the value, the record's readable fields, the entry's
`props` and the usual `t` and `locale`. Until the script is loaded, or if it
fails, the value is shown as text. A script without the SDK registers with
`window.rustcms.registerEntry(name, view)` (see [custom fields](custom-fields.md)).

## Panel pages

A [panel page](customizing.md) can show its data with entries, without a
script: `PageView::Infolist` names the entries, `data()` gives the values by
entry name.

```rust
fn view() -> PageView {
    PageView::Infolist(|| {
        Infolist::new().schema(components![Section::make("Totals").schema(components![
            TextEntry::make("courses").number(0),
            BooleanEntry::make("healthy").labels("Healthy", "Degraded"),
        ])])
    })
}

async fn data(_ctx: &Ctx) -> AppResult<Value> {
    Ok(json!({"courses": 3, "healthy": true}))
}
```

Panel pages use `TextEntry`, `BooleanEntry`, `KeyValueEntry` and
`CustomEntry` (the others need a record), and conditions with `filled()`
only (the values have no declared kind). React pages draw the same entries
with the SDK's `Infolist` component.

## Boot checks

The application refuses to start when an entry names no field (or, for a
`ComputedEntry`, no computed value); an entry does not fit its field (a
`RelationEntry` on a field without `belongs_to`, a `BooleanEntry` on text);
an option does not fit the entry (`badge` on a boolean, `number` on text, `color` on text without `badge`,
`date_format` on another entry than `TextEntry`, or on a field that is not a date);
the field is encrypted; an entry appears twice; a custom entry's name is not
`vendor/name` or its props exceed 4 KiB; a condition breaks the rules of
forms; an infolist holds an input or a form holds an entry; a computed value
has a field's name, is declared twice, or has a time limit over 30 seconds.
