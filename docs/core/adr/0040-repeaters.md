# ADR 0040: Repeaters

- Status: accepted (0.13.0, vocabulary 15; relationship repeaters amended by ADR 0041, vocabulary 16, and by ADR
  0044 in 0.14.0: conditions, filtered relations, view pages, media in rows, `[admin] repeater_rows`,
  vocabularies 24 and 25)
- Date: 2026-11-15

> **0.13:** relationship repeaters (part 2) follow the lock contract of ADR 0041 (child `lock_parent` required, one locking pass, rows without references or media, counts-only history, tagged refs).

## Context

Forms edit one value per input. Applications need lists of rows, each row
several inputs, in two shapes:

- **In one JSON column** of the record: FAQs, specifications, social links,
  opening hours, price tiers. Today a JSON column is edited as a key/value
  list (`KeyValue`) or raw text.
- **As child records** of another resource: the lines of an order, the
  answers of a quiz question, the lessons of a course. Today they are edited
  one by one in the record page's relation manager (has-many), never in the
  parent's form.

Filament covers both with one component, `Repeater` (`->relationship()` for
child records), and shows them on view pages with `RepeatableEntry`. The
owner wants the same names and behaviour where the architecture allows.

Constraints from the existing design:

- **The server decides.** Every row is checked on the server: kinds,
  required values, lengths, options, bounds, field rules, policies; the SPA
  only mirrors.
- **JSON rows have no fields.** Field rules (`Field::required`,
  `max_length`, `readable`) belong to columns; the inputs of a JSON row
  describe parts of one column's value, so their kind and rules must be
  declared on the inputs themselves.
- **Replacing a set loses data** (ADR 0035). A child-record repeater that
  sends "the rows are now {a, b}" deletes rows another user added meanwhile,
  and rows the user cannot see (the child's `scope()` or view policy). Filament
  does exactly that (it deletes the related records missing from the form's
  state); here it is not acceptable.
- **One write, one lock order.** A parent's write that also writes children
  must lock parents before children (`Registry::lock_rank`, ADR 0018/0020/0035),
  keep the child's own checks (scope, policy, field rules, references, hooks)
  and roll back as a whole.
- **Media references** (`media::check_references`) are checked per field;
  a media id buried in a JSON row would escape them.
- Forms are shared by resources, settings pages and action forms.

## Decision

### Declaring

```rust
// JSON: one column holding [{question, answer}, …]
Repeater::make("faq")
    .schema(components![
        TextInput::make("question").required().max_length(200),
        Textarea::make("answer").required(),
    ])
    .min_items(1)
    .max_items(20)
    .reorderable()
    .collapsible()
    .item_label("{question}")
    .cloneable()

// Child records: the `lines` has-many relation of the resource
Repeater::make("lines")
    .relationship()
    .order_column("position")
    .max_items(100)
    .schema(components![
        SelectInput::make("product_id"),
        NumberInput::make("quantity"),
        Repeater::make("options").schema(components![TextInput::make("name").required()]),
    ])
```

- `Repeater` is an input (`InputType::Repeater`, `repeater` on the wire)
  whose `schema` is a list of components: inputs and the layouts `Section`
  and `Grid` (tabs and splits stay at the form's top level).
- Options, as in Filament: `min_items`, `max_items`, `reorderable` (a JSON
  repeater keeps the order the user sets) or `order_column(field)`
  (relationship), `collapsible`, `collapsed`, `item_label("{input}")` (a
  template over the row's inputs, as record titles in ADR 0038; Filament's
  closure cannot reach the browser), `add_action_label`, `addable(false)`,
  `deletable(false)`, `default_items(n)` (rows a create form starts with),
  `cloneable`. Table, single-input and grid layouts, and the `Builder` (rows
  of several block types), come in a later branch.
- **JSON rows declare their rules on their inputs.** `TextInput` /
  `Textarea` hold text (`required`, `max_length`), `NumberInput` a JSON number
  (`integer()` for whole numbers within ±2^53, `min`/`max`), `Toggle` /
  `Checkbox` a boolean, `SelectInput` one of its static options,
  `CheckboxList` several, `DatePicker` a date (`YYYY-MM-DD`), `DateTimePicker`
  an instant (RFC 3339 UTC), `KeyValue` an object of strings, `CustomInput`
  any JSON value (`required` only), `Repeater` a nested list. `required` and
  `max_length` on inputs exist only for JSON rows (refused elsewhere: there
  the field decides).
- **Relationship rows are the child's fields.** `relationship()` names the
  `Relation::has_many` of the repeater's name; each input of its schema names
  a field of the child resource, with that field's rules. The child's foreign
  key to the parent and the order column are never inputs. One level only:
  no repeater of any kind in a relationship row (nested relationship
  repeaters, ADR 0044 part C, were not accepted).

### JSON repeaters

- **The value is the whole list**: a JSON array of objects whose keys are
  the row's inputs (a key the row does not declare is a 422). It is one field
  of one record, written like any other field, so replacing it loses nothing
  that was not in the form.
- **Each row is checked like a form**, with the rules above, then
  `min_items` / `max_items`. Nesting goes three repeaters deep at most.
- **Conditions** (ADR 0037) work inside rows and name inputs of the same row
  (the enclosing form's inputs are left for later: a hidden parent input
  would have to count as `null` across levels). A row input hidden by its
  condition is removed from the stored row; `required` means "required when
  shown"; `clear_when_hidden` does not apply inside rows (the row is
  rewritten anyway).
- **Media and rich text inputs are refused in JSON rows** (until ADR 0044,
  part B, which checks their references with the record's) (`FileUpload`,
  `RichEditor`): their ids would escape the reference checks. Secrets and
  belongs-to-many inputs are refused too; a `SelectInput` takes static
  options only (no relationship).
- **Settings pages** take JSON repeaters: `SettingField::repeater(name)`
  holding a `Vec<T>` (`T: Serialize + Deserialize`), validated the same way.
- **Imports and exports** carry the value as JSON text, checked like a
  client write.

### Relationship repeaters

**The value is a change, not a set** (as ADR 0035 for links):

```json
"lines": {
  "create": [{"ref": "n1", "data": {"product_id": 4, "quantity": 2}}],
  "update": [{"key": "8", "data": {"quantity": 3}}],
  "delete": ["9"],
  "order":  ["8", {"new": "n1"}, "10"]
}
```

- Each part is optional; at most `max_items` (and `[admin] repeater_rows`,
  ADR 0044; 100 before 0.14) entries per part;
  `ref`s are `[a-z0-9-]{1,16}` and unique; a key appears in at most one of
  `update` and `delete`; `delete` and `update` on create are a 422. Rows not
  named are never touched, so a save never deletes a row another user added
  or a row the user cannot see.
- `data` holds the repeater's row inputs only: any other field of the child
  is a 422 at `lines.<key or ref>.<field>`, even one the child's own form
  writes (deny by default). On create, the rows created must reach
  `min_items` (the repeater left out creates none).
- **`order`** (with `order_column`) lists every row the user sees after the
  change (kept, updated and created), each once; one missing or extra is a
  422 (`lines`). Visible rows take, in that order, the positions visible rows
  held before (deleted ones removed), new rows the positions after the
  largest; rows the user cannot see keep theirs. Positions are written in two
  passes (temporary values outside the used range, then the final ones), so
  a `unique (parent, position)` holds on SQLite, PostgreSQL and MySQL.
- **Authorization, the relation manager's rules on the parent's write:**
  after the parent's own admission (`create` or `update`), `create`,
  `update` and `delete` on the child resource are admitted for the parts sent
  (403), before any key is read; then the shape (422 on `lines`). In the
  parent's transaction, in lock order (the parent's ancestors, the parent,
  then the child rows named, key order, through `lock_visible`: child scope
  and view policy, re-checked under the lock), a key not visible or not a
  child of this parent is a 422 (`lines.9`). Each row then passes the child's
  field rules for its operation (`can_write`), its references (`belongs_to`,
  media) and validation; the foreign key to the parent is set by the server
  (as relation creates do). After the parent's row is written, the child's
  policy decides `update` / `delete` on each locked row and `create` with the
  parent in context (403 rolls everything back).
- **Writes, in one transaction:** the parent row, then deletes, updates,
  creates (refs get their keys), positions. The child's hooks run per row;
  the parent's `after_save` runs last and sees every child. Any error rolls
  back the parent and every row.
- **Deleting a child with soft deletes moves it to the trash** (as Filament's
  `delete()`); trashed rows are neither shown nor ordered.
- **History, on both:** the parent's entry counts the relation's change
  (`created`, `updated`, `deleted`, `reordered`; never child keys, ADR 0041),
  and each child gets its own entry, as when written from the relation manager.
- **Reads go through the relation's list** (`GET …/relations/{rel}`: child
  scope, view policy, read rules), up to `max_items` (at most `[admin]
  repeater_rows`, default 50, ADR 0044; 100 before 0.14); with more
  rows the repeater is read-only and points to the relation section.
- **Errors are keyed by row**: `lines.8.quantity`, `lines.n1.quantity`.
- A create's `Idempotency-Key` claim covers the rows (it hashes the whole
  input).
- Conditions on a relationship repeater and in its rows came with ADR 0044
  (part A; a hidden repeater keeps its rows); JSON repeaters in its rows are
  still refused at boot. New rows take positions after every row
  of the parent (trashed ones included) before the order is applied.
- The parent's history entry is `rows_changed`, its `meta` holding the
  relation, how many rows were created, updated and deleted, and whether rows
  were reordered (the history shows it; which rows and their field changes
  stay on each child).

### Schema (vocabulary 15)

`InputSchema` of type `repeater` gains `repeater`: `components` (the row's
inputs and layouts, each as an `InputSchema`), `minItems`, `maxItems`,
`reorderable`, `collapsible`, `collapsed`, `itemLabel`, `addActionLabel`,
`addable`, `deletable`, `defaultItems`, `cloneable`, and for relationship
repeaters `relationship: {resource, relation, create, update, delete}`, what
the user may do (create decided without a record on create pages and on the
record on edit; update and delete admitted, then per row from each row's
envelope abilities). Row inputs of fields the user may not read are left
out; inputs the user may not write are disabled, as in forms.

Error paths: `name.index.input` for JSON rows (`faq.2.question`, nested
`faq.2.links.0.url`), `name.key.input` / `name.ref.input` for relationship
rows; `name.index` / `name.key` for a row, `name` for the list.

Older SPAs see vocabulary 15 as unknown: they show the input read-only and
never send it.

### View pages and lists

- `RepeatableEntry::make(name).schema(components![…])` shows a JSON repeater's
  rows with entries (recursively), or a relationship's child records read
  through the relation's list (scope, view policy, read rules; the first
  `limit` rows, default 10, with a link to the relation section). It does
  not hide the relation section (as in Filament: drop it from `relations()`
  if it is not wanted).
- `TextColumn::make("faq").count()` shows a JSON repeater's number of rows
  (counted by the panel from the record's value).
- `TextColumn::make("lines_count").counts("lines")` shows each record's number
  of children, as Filament's `counts()`: list envelopes carry `counts`, the
  children the user may list (the child's list query: scope, trash, view
  policy). When the child's policy decides once, one count of the list's
  query per record of the page (at most 100) rather than one grouped query,
  which would have to rebuild the scope's joins and `DISTINCT` outside the
  list's own query; when it decides per record, one bounded scan of the
  page's children (at most 10,000 candidates for the whole page, each
  checked; beyond, no counts). Users who may not list the relation get
  neither the column nor the counts. Not sortable. Computed by the list route
  only (relation lists, the repeater widget's reads and Rust callers of
  `list` go without). `count()` columns are neither sortable nor searchable.
- The history shows a JSON repeater's change row by row and input, matched
  by position (`faq › 2 › question`; a row added or removed whole), and a
  relationship repeater's `rows_changed` entry as the keys created, updated
  and deleted.
- CSV: exports write a JSON repeater as its JSON text; imports read it back
  and check its rows as a form save does (errors at `faq.0.question`).

### Boot checks

A repeater is refused when its schema is empty or holds tabs, splits,
secrets, belongs-to-many inputs, or (JSON) media or rich text inputs or a
relationship select; a JSON row input repeats a name; `required` or
`max_length` is on an input outside JSON rows; nesting goes deeper than
three; `item_label` names no input of the row; `min_items` exceeds
`max_items`; `default_items` exceeds `max_items`; the column of a JSON
repeater is not JSON; `relationship()` names no has-many relation, a row
input is not a field of the child, the child's foreign key or order column
is an input, the order column is not an integer field, a relationship row
holds a relationship repeater, or a relationship repeater has
`clear_when_hidden`; a condition in a row names no input of the row; the
child locks a parent (`lock_parent`) other than this relation's (its lock
would come after its own rows). A count
column is refused when `count()` is on a non-JSON field or has a custom cell,
or `counts()` names no has-many relation or takes a field's name.

## Consequences

- One component for both shapes, with Filament's names; the difference that
  matters (a change instead of a set for child records) is invisible to the
  user.
- Inputs gain `required` and `max_length` for JSON rows only, and
  `NumberInput::integer`. `InputType`, `InputSchema` (`repeater`),
  `EntryType` (`repeatable_entry`) and `SettingKind` gain members: exhaustive
  matches and struct literals in applications need them.
- Relationship repeaters write several resources in one request; their lock
  order and checks are the relation manager's, so the two paths stay one.
- Media inside JSON rows, the Builder, table/simple/grid layouts, row ids,
  nested relationship repeaters and deleting hidden rows are left for later.
