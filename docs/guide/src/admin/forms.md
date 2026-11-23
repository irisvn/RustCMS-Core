# Forms

`form()` describes how a resource's create, edit and record pages lay out
its [fields](resources.md#fields): which input edits each field, in which
sections, tabs or columns. It is presentation only. What a user may read or
write is decided by the fields and checked on the server, whatever the form
shows.

![The demo's post form, on its Publishing tab](/img/admin/forms-post-tabs.webp)
*The demo application: the post form's tabs.*

## Quick start

```rust
fn form() -> Form {
    Form::new().columns(2).schema(components![
        TextInput::make("title").placeholder("A catchy title"),
        SelectInput::make("category_id"),          // options from the `belongs_to` target
        Toggle::make("published"),
        DateTimePicker::make("published_at"),
        Textarea::make("body").column_span(2),
    ])
}
```

Each input names a field. The form is built per user and operation (create,
edit, view): inputs for fields the user may not read or write are left out
or disabled, and the server checks everything again.

Without `form()`, the panel builds one input per field that is not
`.guarded()`:

| Field | Input |
|---|---|
| `.belongs_to(…)` | `SelectInput` (the target's records) |
| `.encrypted()`, `.encrypted_map()` | `SecretInput`, `SecretMapInput` |
| integer, float, decimal | `NumberInput` |
| `bool` | `Toggle` |
| date | `DatePicker` |
| date-time (local or instant) | `DateTimePicker` |
| JSON | `Textarea` (raw JSON) |
| text, UUID | `TextInput` |

A `form()` with only a width (`Form::new().width(FormWidth::Wide)`) keeps the
generated inputs at that width.

## Full example

The demo's post form: three tabs, a two-column tab, media, rich text, the
tags of a belongs-to-many relation and key/value attributes.

<<< @/../../../examples/demo/src/admin/resources/post_resource.rs#form [src/admin/resources/post_resource.rs]

## Inputs

| Input | For |
|---|---|
| `TextInput` (`.email()`, `.password()`, `.url()`) | text |
| `Textarea` | long text |
| `NumberInput` | integers and decimals (kept exact); `.min(n)`, `.max(n)`, `.step(n)` ([below](#number-bounds)) |
| `SelectInput` | static `.options([(value, label)])`, options [computed on the server](#options-computed-on-the-server), or the records of a `belongs_to` field |
| `Toggle`, `Checkbox` | booleans (three states when nullable) |
| `CheckboxList` | a JSON list of option values (`.options(…)`) |
| `DatePicker`, `DateTimePicker` | dates, local date-times and instants (only instants are edited in UTC, local date-times as stored; shown on the record's page in the user's language, instants in their time zone, which is named) |
| `Hidden` | a value the user does not edit |
| `KeyValue` | a JSON object of text values ([below](#key-value)) |
| `FileUpload`, `RichEditor` | [media and rich text](media.md) |
| `CustomInput::make(field, "vendor/widget")` | a [custom widget](custom-fields.md); `.props(json!(…))` passes it settings (at most 4 KiB) |
| `SecretInput`, `SecretMapInput` | [encrypted fields](#encrypted-fields) (chosen for them by default) |
| `BelongsToManySelect::make(relation)` | the links of a [belongs-to-many relation](relationships.md#choosing-links-in-the-records-form) |

Every input takes `.label(…)` (default: the field's label), `.placeholder(…)`,
`.help(…)` (a line under the input) and `.column_span(n)` (columns it takes in
its grid). Text inputs also take [`.prefix(…)` and `.suffix(…)`](#an-editor-layout).
`SelectInput` is named so it does not clash with `sea_orm::Select`. Labels,
placeholders, help and option labels are translated per resource
([i18n](i18n.md)).

A widget name is `vendor/name`: each part 1 to 32 characters of `a-z`, `0-9`
and `-`.

### Options are validated {#options-are-not-validated}

The static `.options(…)` of a `SelectInput` or a `CheckboxList` bound what a
client may write to the field, on every write path: API create and update,
creates through a relation, and CSV imports (a row out of the options fails
with its code). The value's **type** is checked first, as for any field.

| Input | A client may write | Otherwise (422 code) |
|---|---|---|
| `SelectInput` | one of the option values | `not_option` |
| `CheckboxList` | a list of option values, each at most once; `[]` is a value | `invalid` (not a list), `not_option`, `given_twice` |

- `null` is checked as for any field (`required`, or a column that is not
  nullable); it is never compared with the options.
- Select values are compared in their canonical wire form, column-aware:
  `"0012"` and `"12"` are the same `i64`, `"10.5"` and `"10.50"` the same
  `Decimal(10, 2)`, an instant with an offset the same instant in UTC, a UUID
  in any case the same UUID. Options are declared in any of these spellings;
  the schema shows them canonical. Wire types stay strict: `12` (a number) is
  not an `i64` value. The value stored is the option matched: `"10.505"` on a
  `Decimal(10, 2)` rounds to the option `"10.50"`, which is what is written.
  Checkbox list items are compared as JSON values.
- An update that leaves the field out keeps the stored value, even one outside
  the options (stored before the check, or by a hook).
- The guarantee covers values a **client** sends. Hooks
  (`mutate_before_create`, `mutate_before_update`), action handlers and pivot
  columns write what they decide; validate there, or with a database `CHECK`
  constraint, if they must stay within the options too. Action forms and
  `BelongsToManySelect` check their own options.
- A `belongs_to` select is different: its value must be a record the user may
  see, checked on every write; static options on it are not used.
- An input with [`options_fn`](#options-computed-on-the-server) is checked
  against the options its function answers, not its static ones.

Declarations that cannot hold stop the application at boot: an option that is
not a value of its column (`12` for an `i64` column), one that would be stored
as another value or read back differently (`"10.555"` on a column of scale 2,
`"100000000.00"` beyond a `Decimal(10, 2)`'s precision, `0.1` on an `f32`
column, a timestamp with a fraction finer than microseconds), a
`CheckboxList` on a column that is not JSON, or a field in several inputs that
do not offer the same options (another set, a select and a checkbox list, or
an input without options). The same options in another order or with other
labels are allowed. `SecretInput` and `SecretMapInput` take no options.

The LMS offers two statuses in its enrollment form:

<<< @/../../../examples/lms/src/admin/resources/enrollment_resource.rs#statuses [src/admin/resources/enrollment_resource.rs]

<<< @/../../../examples/lms/src/admin/resources/enrollment_resource.rs#form [src/admin/resources/enrollment_resource.rs]

The panel refuses any other status from a client. The write hooks still
check it through the service they call (see
[validating in a hook](resources.md#validating-in-a-hook)): the course's
enroll action and the seeder write enrollments through that service, not
through the form.

<<< @/../../../examples/lms/src/services/enrollment_service.rs#check [src/services/enrollment_service.rs]

### Options computed on the server

When a select's or checkbox list's choices depend on other inputs or on data
(the topics of the chosen category, the rooms free on a date), compute them on
the server with `.options_fn(f)` and name what they depend on with
`.depends_on([...])`:

```rust
SelectInput::make("category").options([("news", "News"), ("guides", "Guides")]),
SelectInput::make("topic").depends_on(["category"]).options_fn(|ctx, state| {
    let category = state.text("category").to_owned();
    Box::pin(async move {
        let topics = topics_of(ctx, &category).await?; // your query
        Ok(topics.into_iter().map(|t| SelectOption { value: json!(t.id), label: t.name }).collect())
    })
}),
```

`f` takes the request's `Ctx` and a [`FormState`](#reacting-to-changes) (the
same values a reaction sees: fields the user may read, what they typed in
inputs they may write, the conditions applied, a hidden input `null`), and
returns `SelectOption`s. Its values are of the field's kind, as static
options are; labels are translated per resource.

- **In the panel.** The form asks for the options when it opens and when an
  input of `depends_on` changes (once typing pauses; only the latest answer
  is used). The declared `.options(…)` show until the answer comes, and stay
  when asking fails. A value no longer among the options stays shown,
  disabled, until the user changes it.
- **On save.** A create, or an update that sends the input or one of its
  dependencies, runs `f` again **before the write's transaction**, with no
  transaction or lock held, then checks under the record's lock that the
  dependencies have not changed meanwhile (else 409 `stale`: send it again)
  and that the value is one of the answer's options (`not_option`; a
  checkbox list is still a list without repeats). On a create, an input left
  out is checked with the default it would store; an empty value is not
  checked, nor an update that leaves the input and its dependencies alone.
  If `f` fails, times out or answers options the server refuses, the save
  fails with the form error `options_failed` and nothing is saved. API
  writes and [CSV imports](import-export.md#importing) are checked the same
  way; a retry of a committed write answers its first result without running
  `f`.
- **Only what the user can see.** The check never depends on a value the
  user may not read: a stored value of a field they may not read (or of a
  record they may not view) is neither checked nor compared.
- **Not a data invariant.** `f` runs outside the write's transaction: what
  else it reads may change before the write commits, and hooks, actions and
  other writes are not checked against it. `depends_on` names everything a
  save compares again. A rule that must always hold belongs in a hook
  (`mutate_before_create`, `mutate_before_update`) or a database constraint.
- **Limits.** `.options_timeout(d)`: 5 seconds by default, at most 30. At
  most 500 options and 64 KiB per answer, no value twice. Asking shares the
  [reactions'](#reacting-to-changes) limits (120 requests per user per
  minute, 2 at once). The route is `POST {admin}/api/resources/{slug}/form/options`
  ([admin API](../reference/admin-api.md)); users who may not read or write
  the input get 404.

`options_fn` is for the own inputs of a resource's create and edit forms.
The boot refuses it without `depends_on`, with a dependency that is not an
input of the same form holding a value (hidden, encrypted, link, repeater or
JSON inputs), on a `belongs_to` select or an encrypted field, on a field
with two inputs, in repeater rows, on settings pages and in action forms,
and on a resource that is the child of a relationship repeater (its rows are
saved inside the parent's transaction, where `f` cannot run).

### Number bounds

`.min(n)` and `.max(n)` on a `NumberInput` bound what a client may write to
the field, like a select's options: on every write path, `must be at least
{min}` (`min_value`) or `may not be greater than {max}` (`max_value`).
`.step(n)` sets the step of the input's arrows and keyboard; it is a hint,
not checked.

```rust
NumberInput::make("level").min(1).max(5),
NumberInput::make("price").min(0.5).step(0.01),
```

- `i32` and `f64` columns get a native number input (step 1 for `i32`
  without one). `i64` and decimal columns keep a text input, so no digit is
  lost; their bounds are still checked by the server.
- As with options, the bounds cover what a **client** sends: hooks and
  actions write what they decide.
- Integers and decimals are compared exactly with the bound as written
  (`0.1` is 0.1). A decimal is also checked as its column rounds it: with
  `.max(4.995)` on a `decimal(10, 2)` column, `4.995` is refused, since it
  would be stored as `5.00`.
- A bound is an `f64`: an `i64` bound above 2^53 is the nearest `f64`.
- On an `f32` column a value is checked as the `f32` it is stored as, against
  the bounds as `f32`s: with `.max(0.1)`, the value read back
  (`0.10000000149…`) is accepted again. Bounds that round to 0 as an `f32`
  (`1e-50`) refuse to boot.
- Action forms have no bounds yet: check the value in the action's handler.
- Number inputs of [action forms](actions.md) take no bounds.

## Layouts

| Layout | Does |
|---|---|
| `Section::make(label)` | a titled group |
| `Grid::make(columns)` | columns without a title |
| `Tabs::make()` with `Tab::make(label)` children | tabs |
| `Split::make().main(…).aside(…)` | a main column and an aside ([below](#an-editor-layout)) |

Layouts hold inputs and other layouts with `.schema(components![…])`.
`Form::columns(n)` (default 1), and `.columns(n)` on a section, a grid or a tab,
lay their children out in `n` columns; `.column_span(n)` on an input widens
it. The LMS course form uses two columns with a wide summary:

<<< @/../../../examples/lms/src/admin/resources/course_resource.rs#form [src/admin/resources/course_resource.rs]

Tabs hold only `Tab`s, and a `Tab` only sits in `Tabs`; a tab's id comes from
its label and must be unique among its siblings. Tabs the user cannot see
anything in are left out. On a failed save the panel opens the first tab with
an error and focuses its input.

### An editor layout

For long content (a post, a page), lay the form out like an editor: the
whole width, a main column and a narrow aside with the details. The demo's
categories use it:

<<< @/../../../examples/demo/src/admin/resources/category_resource.rs#form [src/admin/resources/category_resource.rs]

![The demo's category form: the main column and the Display aside](/img/admin/forms-editor-layout.webp)
*The demo application: the category form at 1440 px.*

- `Form::width(FormWidth::Default | Wide | Full)`: how wide the record's
  page is (create, edit and view; settings pages too): about 56rem (the
  default), 72rem, or the whole page.
- `Split::make().main(…).aside(…)`: from the `lg` breakpoint (1024px) the
  aside is 20rem wide on the right and stays in view while the main column
  scrolls; narrower screens show one column, the aside after the main one
  (`.aside_first_on_mobile()` puts it first). One split at most, at the
  form's top level; both columns hold inputs and layouts (sections, grids,
  tabs). A column the user cannot see anything in is left out. The view
  page lays the record out the same way.
- `Section::make(…).collapsible(collapsed)`: the user folds and unfolds the
  section with its title (a button with `aria-expanded`), starting folded
  when `collapsed` is true. Its inputs stay in the form while folded, and it
  opens by itself when one of them has an error. Nothing is remembered.
- `Section::make(…).compact()`: less padding and spacing, for the aside.
- `TextInput::make(…).prefix(text)` and `.suffix(text)`: fixed text inside
  the input's border (`/posts/` before a slug, `.html` after it), shown as
  written, never part of the value, and read by screen readers with the
  input; the record's page shows the value with them. Text inputs only.

Misusing them (a split inside a section, two splits, a grid that folds, a
prefix on a number input) stops the boot.

## Showing inputs under conditions

`.shown_when(condition)` on an input or a layout shows it only while the
condition holds on the form's values:

```rust
SelectInput::make("link_type").options([("internal", "Internal"), ("external", "External")]),
TextInput::make("url").shown_when(when("link_type").eq("external")),
Toggle::make("on_sale"),
Section::make("Sale").shown_when(when("on_sale").is_true()).schema(components![
    NumberInput::make("discount").clear_when_hidden(),
]),
```

| Condition | Holds when the input's value |
|---|---|
| `when("x").eq(v)`, `.ne(v)` | equals (differs from) `v` |
| `when("x").is_in([a, b])` | is one of the values |
| `when("x").filled()` | is set: not `null`, blank text, `[]` or `{}` |
| `when("x").is_true()` | is `true` (a boolean) |
| `Condition::all([…])`, `Condition::any([…])` | every / one of the conditions hold |

- **Live in the browser.** The panel re-evaluates conditions on every change
  (each keystroke, selection or toggle), without a request: the inputs a
  condition reads need no extra declaration. A hidden input keeps what the
  user typed until the form is saved, so showing it again brings it back.
- **The server decides.** On every write it evaluates the conditions again,
  on what the user's form holds (the record's values the user may read, or
  the declared defaults on create) with the request over it. A value sent
  for a hidden input is never stored; a hidden field keeps its stored value.
  With `.clear_when_hidden()`, a field that a write hides takes its column's
  default, else `null`, else an empty text or list.
- **Required when shown.** A `.required()` field under a condition is only
  required while it is shown: when it is sent empty, on create, or when the
  write shows it.
- A layout's condition applies to everything in it; a hidden input counts as
  empty for the conditions of other inputs. A field shown by another input
  without a condition is always shown.
- Conditions compare values; logic that needs the database belongs in hooks.
- Condition only on fields every user of the form may read. For a user who
  may not read a field (or view the record), the panel and the server treat
  it as empty, so the inputs it shows stay hidden for that user; nothing
  about its value can be learned from what is saved.
- On a form opened from a [relation](relationships.md), the key the relation sets
  is read by the conditions with the parent's key.
- In a [repeater's rows](#repeaters), `when` names an input of the same row;
  `when_root("x")` names an input of the record's form and `when_parent("x")`
  one of the enclosing row (a JSON repeater nested in a row), with the same
  tests. `when_root` and `when_parent` are refused outside rows.

The same works on [settings pages](settings.md) and in
[action forms](actions.md) (`ActionField::shown_when`; a hidden field reaches
the handler as `null`).

## Reacting to changes

`.reacts(f)` runs `f` on the server when the user changes the input, once
typing pauses (about 400 ms), and the panel applies the changes it answers
with: a slug filled from a name, a total from order lines, rows kept in step
with a text.

<<< @/../../../examples/demo/src/admin/resources/category_resource.rs#reacts [src/admin/resources/category_resource.rs]

`f` takes the request's `Ctx` (it may read the database; it should not
write) and a `FormState` (`text(name)`, `get(name)`, `rows(name)`,
`relation_rows(name)` ([below](#reactions-on-child-records)),
`operation()`, `key()`, `changed()`, and `previous()`: the changed input's
value when the panel last applied an answer, as the slug above uses it), and
returns a list of `FormPatch`es:

| Patch | Effect |
|---|---|
| `FormPatch::set(field, value)` | sets the field (a wire value) |
| `FormPatch::set_if_empty(field, value)` | sets it when it is `null`, `""` or `[]` |
| `FormPatch::add_row(field, row)`, `add_row_at(field, index, row)` | adds a row to a JSON repeater |
| `FormPatch::remove_row(field, index)`, `move_row(field, from, to)` | removes or moves a row |
| `FormPatch::flag_row(field, index, message)` | a warning under a row; changes nothing |
| `FormPatch::notice(message)` | a message to the user |
| `FormPatch::add_relation_row(field, data)`, `update_relation_row(field, id, data)`, `remove_relation_row(field, id)`, `flag_relation_row(field, id, message)` | [rows of a relationship repeater](#reactions-on-child-records) |

```rust
Repeater::make("lines")
    .schema(components![TextInput::make("name"), NumberInput::make("qty").integer()])
    .reacts(|_ctx, state| {
        let total: i64 = state.rows("lines").iter().map(|r| r["qty"].as_i64().unwrap_or(0)).sum();
        Box::pin(async move { Ok(vec![FormPatch::set("total", total)]) })
    }),
NumberInput::make("total"),
```

- **What it sees.** What the user's form holds, as conditions read it: the
  stored values of the fields the user may read (on an edit form, only when
  the user may view the record), then what the user typed in the inputs they
  may write (a value that does not decode yet, a number half typed, is left
  out), and on a create form opened from a relation section, the key that
  relation binds. Never encrypted fields or links, nor a field the user may
  not read; the rows of relationship repeaters are apart
  (`relation_rows`). **These values are user input**: check an id the
  reaction looks up (a `belongs_to` key, a media id) against what the user may
  see, with `ctx.records(…)`.
- **What it may change.** Patches apply in order (a row index is the row's
  place after the previous patches), conditions evaluated again after each
  one. The server drops, and logs, a patch the user could not make: a field
  not in the form, not readable and writable by the user, hidden by its
  conditions, encrypted, or a value its input refuses; `add_row` and the other
  index patches name JSON repeaters, the `*_relation_row` patches
  relationship repeaters. Patches that change nothing are dropped. Values are sent as
  a save stores them (a decimal at the column's scale, rows without their
  hidden inputs). At most 200 patches, messages of at most 500 characters.
  Changes made by a patch do not set off other reactions.
- **Who may ask.** The form's own rule: `create` on a create form (a
  relation's create form as that relation creates), `update` on the record
  (in the user's scope) on an edit form. Never on view pages, lists, exports
  or saves. 120 requests per user per minute, 2 at once per user; a JSON
  repeater sent with more rows than it holds is refused (422).
- **Failing alone.** An error, or a reaction slower than its time limit
  (`.reaction_timeout(d)`: 5 seconds by default, at most 30), is logged and
  the form goes on without it. The panel ignores an answer once the user has
  changed the form again.
- **Not a rule.** Reactions help the user fill the form; saves never run
  them (the API, imports and actions write without them). What must always
  hold belongs in `mutate_before_create`/`mutate_before_update` or validation.

Reactions are for resource forms: settings pages and action forms do not take
them yet, nor inputs inside repeater rows.

### Reactions on child records

A reaction also reads and changes the rows of the form's
[relationship repeaters](#child-records), and a relationship repeater may
react itself: the panel asks when rows are added, removed, reordered or
edited. The demo's categories give a new post switched to published a title:

<<< @/../../../examples/demo/src/admin/resources/category_resource.rs#posts-reacts [src/admin/resources/category_resource.rs]

- **Rows in the state.** `state.relation_rows(name)` gives the repeater's
  rows as the form holds them (stored rows with the user's unsaved edits,
  then new rows), each a `StateRow`: `id()`, `key()` (a stored row),
  `reference()` (a new row, `n1`, `n2`, …), `get(name)`, `text(name)`,
  `values()`. A row holds the repeater's inputs the user may read. The list is
  empty when the user's form does not hold the repeater (the relation not
  readable, the repeater hidden, more stored rows than the form edits).
- **Rows by identity.** A row patch names its row by `RowId`: `RowId::Key`
  for a stored row, `RowId::Ref` for a new one (pass `row.id().clone()`). A
  key and a reference never name the same row, whatever their text.
  `add_relation_row_as(field, "n1", data)` names the added row so later
  patches of the same answer can change it.
- **What is dropped.** As for other patches, the server keeps only what the
  user could do in the form: adding needs `create` on the child (with the
  parent bound) and the repeater `addable`, under `max_items`; changing a
  stored row needs `update` on that record, removing it `delete` and the
  repeater `deletable`, above `min_items`. A stored row is read for the user
  (the child's scope, the parent's rows of the relation, its filters, the
  view policy); a key not found that way drops the patch. Values go to
  fields the user may read and write for that operation (create for a new
  row, update for a stored one), never the parent key, a filtered field or
  the order column, and inputs hidden by the rows' conditions are dropped.
  On a create form there are no stored rows: only new rows are added,
  changed or removed.
- **Nothing is written.** The panel shows the changes as unsaved edits; the
  save checks every row as usual.

## Leaving with unsaved changes

A create or edit form with changes not saved asks before the user leaves it:
the panel's dialog (*Keep editing* or *Discard changes*) for links and the
browser's Back button, the browser's own prompt for closing or reloading the
tab. It does not ask after a save, while a save is in flight or its outcome
is unknown (the resource is paused and the retry is offered on any page), or
when the session sends the tab elsewhere (sign-out, another user signing in).

## Key-value

`KeyValue` edits a JSON column holding an object of text values
(`{"size": "M"}`). The field is a key/value field when it declares
`.key_value()` or `.max_items(n)`, or a `KeyValue` input uses it; its bounds
then apply to every write, API and hooks included, whatever the form shows:
`max_items` pairs (default 50), keys up to 255 characters, values up to
`max_length` (default 1,000). A stored value that is not an object of text is
shown read-only and never rewritten; a nullable field can be cleared to "not
set" (distinct from an empty object). The column must be JSON.

```rust
Field::column(post::Column::Attributes).key_value().max_items(20).max_length(200),
// in the form
KeyValue::make("attributes").help("Extra details shown with the post, e.g. reading_time = 5 min"),
```

## Repeaters

A `Repeater` edits a list of rows, each row several inputs, as Filament's
does. Its value is a JSON array of objects in the column of its name
(`[{"question": "…", "answer": "…"}]`); the column must be JSON.

```rust
Repeater::make("faq")
    .schema(components![
        TextInput::make("question").required().max_length(200),
        Textarea::make("answer").required(),
        SelectInput::make("kind").options([("text", "Text"), ("link", "Link")]),
        TextInput::make("url").shown_when(when("kind").eq("link")),
    ])
    .min_items(1)
    .max_items(20)
    .reorderable()
    .collapsible()
    .item_label("{question}")
    .add_action_label("Add a question")
    .cloneable()
```

- **A row's rules are on its inputs.** A row is part of one column, with no
  field of its own, so its inputs say what they need: `.required()`,
  `.max_length(n)` on text, `.integer()`, `.min(n)`, `.max(n)` on numbers,
  `options` on selects (rows have no relationships), and for files and
  documents `.accept([…])`, `.max_size(bytes)`, `.multiple(n)` (a list of up
  to `n` files), `.collection(name)` and `.nodes([…])` (a rich editor's
  application elements). Elsewhere these rules come from the field, and the
  boot refuses them on inputs.
- **Values in a row:** text for `TextInput` and `Textarea`, a JSON number for
  `NumberInput`, `true`/`false` for `Toggle` and `Checkbox`, the option's value
  for `SelectInput`, a list for `CheckboxList`, `YYYY-MM-DD` for `DatePicker`,
  an RFC 3339 instant for `DateTimePicker`, an object of text for `KeyValue`,
  any JSON for `CustomInput`, a file id (a list with `.multiple`) for
  `FileUpload`, a document for `RichEditor`, and a list for a nested
  `Repeater` (three deep at most). Empty inputs are left out of the row.
- **Files and documents in rows** are checked as a field's on every write:
  each file must be in the media library, visible to the user, of an accepted
  type and size; a document is checked as rich text, and its images as files.
  Errors name the row (`faq.2.icon`). A file only a row names has no foreign
  key in the database: deleting it from the library leaves the row showing it
  as unavailable, and [restoring](resources.md#soft-deletes) a record does not
  check its rows' files again. One save refers to at most 2,000 distinct files
  ([media fields](media.md#media-fields)). A row's rich editor shows a compact toolbar (bold, italic,
  underline, superscript, subscript, images, application elements);
  `.full()` gives the full one. The document's rules do not change with the
  toolbar.
- **Each row is checked on the server**, like a form: a key the row does not
  declare is refused, then each input's rules, then `min_items` and
  `max_items`. Errors name the row and the input: `faq.2.question`
  (`faq.2.links.0.url` nested, `faq` for the list); the panel shows them in
  their row.
- **Conditions** (`shown_when`) in a row name inputs of the same row, or of
  the record's form with `when_root` (`when_parent` for the enclosing row of a
  nested repeater). An input hidden by its condition is removed from the
  stored row, and `required` means "required when shown". Inputs whose
  conditions read the form are checked once the form's values are known (on
  update, the stored record with the request over it).
- **Options:** `min_items`, `max_items`, `reorderable` (buttons, or drag the
  handle), `collapsible` / `collapsed`, `item_label("{input}")` (each row's
  heading, a template as [record titles](resources.md#titles-from-several-fields)),
  `add_action_label`, `addable(false)`, `deletable(false)`,
  `default_items(n)` (rows a create form starts with), `cloneable`, `table()`
  (below).
- **`table()`** lays each row out as a table row: one column per input (the
  labels in a header; each cell keeps its label for screen readers), the
  row's buttons last; below the `md` breakpoint the rows stack as usual. An
  input hidden by its condition leaves its cell empty. Table rows hold inputs
  only (no sections, grids, nested repeaters, file uploads or rich editors)
  and do not fold.
- **Not in rows:** secrets, belongs-to-many inputs, record selects
  (`belongs_to`: a JSON row has no field to choose from), tabs and splits.

On view pages each row is shown with the entries of its inputs (a
`RepeatableEntry`, see [the default view page](infolists.md#the-default-view-page));
an [infolist](infolists.md) can choose them. [Settings pages](settings.md#kinds) take
repeaters too.

### Child records

With `.relationship()`, the rows are the child records of the has-many
[relation](relationships.md) named like the repeater, edited in the parent's
form (the lines of an order, the answers of a question):

```rust
// The parent (orders):
fn relations() -> Vec<Relation> {
    vec![Relation::has_many("lines", "order_lines", "order_id")]
}

Repeater::make("lines")
    .relationship()
    .order_column("position")
    .schema(components![TextInput::make("product"), NumberInput::make("quantity")])

// The child (order_lines) locks its parent: every write of a line locks its order first.
ResourceMeta::new("Line").lock_parent("order_id", "orders")
```

- **The child locks its parent** (`lock_parent` through this relation;
  required): saves of an order and of its lines never interleave.
- **Row inputs are the child's fields**, with the child's rules: its field
  rules, validation, options and bounds (declared on the child's form, shown
  in the rows), `scope()`, policy and hooks apply to every row, as when the row
  is written from the relation section. The child's key to the parent and the
  order column are set by the server, never inputs.
- **Media, rich text and `belongs_to` fields in rows** are the child's, with
  its rules: a row's files and images are checked with the parent's, once,
  after every other record the save locks (also those a child's hook adds);
  a `belongs_to` select offers the child's options and its target must be
  visible to the user. Errors name the row (`photos.n1.image`). Rich editors
  show the compact toolbar (`.full()` for the full one).
- **Not in rows:** encrypted and belongs-to-many fields (edit them on the
  child's page). Hooks of the child should not write other resources.
- **A row's `after_save` runs before the save's file check**, which comes
  last: if a file turns out to be unavailable, the whole save is rolled back
  after the hook ran. Do work outside the database from a
  [job dispatched in the transaction](resources.md#hooks-after-a-write)
  (`ctx.dispatch(txn, …)`), never directly from the hook.
- **Several repeaters on one child resource** need [filtered relations](relationships.md#filtered-relations)
  that cannot share a row (a question's right and wrong answers, one table
  filtered by `where_eq("correct", true)` and `where_eq("correct", false)`). A resource with relationship repeaters cannot link
  media records (belongs-to-many), and cannot lock a parent of its own
  (`lock_parent` has no chains).
- **Concurrent saves** of one parent and its children wait for each other;
  a rare conflict through other records answers 409 ("try again"), nothing
  saved.
- **A save sends a change, not the list:** the rows added, the fields changed
  in each row, the rows removed, and the order. Rows the user did not touch,
  rows another user added meanwhile, and rows the user cannot see are never
  written or removed.
- **One write:** the parent row, then its rows (removed, changed, added,
  reordered), then the parent's `after_save`, which sees every row. A refusal
  anywhere (a row's validation, a child's policy or hook) rolls back the
  parent and every row.
- **Permissions:** adding rows needs `create` on the child, changing them
  `update`, removing them `delete` (each row's policy decides too); the form
  offers only what the user may do.
- **Removing** a row of a child with soft deletes moves it to the trash.
- **`order_column(field)`** (an integer field of the child) keeps the rows in
  the user's order: they take the positions the visible rows held; rows the
  user cannot see keep theirs. A unique `(parent, position)` is fine. Moving
  a stored row needs `update` on it; without an order column the rows cannot
  be reordered.
- **`min_items` / `max_items`** are checked by the server on the parent's
  rows when a save removes or adds rows, and `min_items` on the rows a new
  parent creates (a create without the repeater has none). Writes to the child
  elsewhere (its own page, the API) are not bounded by them.
- **`addable(false)` / `deletable(false)`** are enforced by the server too.
- New rows on an edit page show the child's create form inputs.
- **Rows edit their inputs only:** a value for another field of the child is
  refused (`lines.8.status`: "is not allowed"), even one its own form writes.
- **Errors** name the row by key or, for a new row, its reference:
  `lines.8.quantity`, `lines.n1.quantity`.
- **History:** the parent's history shows "Rows changed" (how many rows were
  added, changed, removed, and whether they moved); each row has its own
  entries in the child's history.
- **How many rows:** the form edits up to `max_items` rows, and at most
  [`[admin] repeater_rows`](../getting-started/configuration.md#admin-configadmintoml)
  (50 by default, up to 500); with more stored rows, the repeater is
  read-only and the relation section edits them. A save may add, change or
  remove at most that many rows in each part, and reorder a parent with at
  most that many rows (422 `too_many_rows` beyond). Applications whose
  parents hold more rows raise the setting. The [default view page](infolists.md#the-default-view-page)
  shows the rows under the record, as a `RepeatableEntry` of the relation.
- **Conditions in rows.** When the repeater's `.schema()` declares no
  condition, rows are checked with the child form's own conditions, as on the
  child's page (the panel does not evaluate them in the rows). When it
  declares any (`shown_when` on a row input or a layout, reading the row and,
  with `when_root`, the record's form), those are the rows' conditions, also
  evaluated by the panel, and the child form's no longer apply to the rows. A
  row input under a condition needs a child column that stores a hidden value
  (nullable, or with a default), as on forms.
- **A condition on the repeater** (its own `shown_when`, or a layout's around
  it) hides it as any input. **Its rows are kept**: switching a question from
  "single choice" to "fill in the blanks" hides its options but does not
  delete them, and they are back if the question is switched again. A change
  sent for a hidden repeater is still checked (permissions, shape), then
  ignored; `min_items` does not apply while it is hidden.
  `clear_when_hidden` does not apply. To remove the rows when the repeater is
  hidden, do it in the parent's `mutate_before_update` or `after_save` hook.
- **Not in rows:** repeaters (JSON or relationship) inside relationship rows.

The demo's categories edit their posts this way (each row is a post):

<<< @/../../../examples/demo/src/admin/resources/category_resource.rs#posts-repeater [src/admin/resources/category_resource.rs]

The LMS's quiz questions edit their options (each with an image and an
explanation, hints read from the question's form) or their blanks, depending
on the question's kind:

<<< @/../../../examples/lms/src/admin/resources/quiz_question_resource.rs#quiz-form [src/admin/resources/quiz_question_resource.rs]

### Repeaters in lists, history and CSV

- A [count column](tables.md#counts) shows the number of rows:
  `TextColumn::make("faq").count()` for a JSON repeater,
  `TextColumn::make("lines_count").counts("lines")` for a relationship.
- The history shows a JSON repeater's change by row and input
  (`faq › 2 › question`), rows matched by position.
- [CSV exports](import-export.md) write a JSON repeater as its JSON text;
  imports read that text and check each row as a save does (errors at
  `faq.0.question`).

## Encrypted fields

The demo's integrations keep an API token and environment variables
encrypted:

<<< @/../../../examples/demo/src/admin/resources/integration_resource.rs#fields [src/admin/resources/integration_resource.rs]

Both hold secrets sealed with `APP_KEY` in a text column without a default;
the model's field is an [`Encrypted<T>`](../digging-deeper/encryption.md):

- `encrypted()` takes an `Encrypted<String>`;
- `encrypted_map()` takes an `Encrypted<BTreeMap<String, String>>`.

The panel never shows them:

- **A secret string** (`SecretInput`) shows *Set*, *Not set* or *Cannot be
  read*, with Replace and Clear. Leaving it alone keeps it.
- **Named secrets** (`SecretMapInput`):
  - the names are listed, and each one is kept, replaced or removed;
  - new rows take a name and a value;
  - values are never shown or prefilled.
- What the API returns and takes (for scripts and tests):

  | | Record value | Write |
  |---|---|---|
  | string | `{"state": "set"}` (`unset`, `unreadable`) | `{"replace": "…"}`, `{"clear": true}`; left out: kept |
  | map | `{"state", "keys", "revision"}` | `{"revision", "set": {…}, "keep": ["NAME"]}`, `{"revision", "clear": true}` |

- **Map rules:**
  - Names neither kept nor set are removed.
  - On an existing record every change needs the `revision` it was read
    with. If someone changed the map meanwhile the save gets 409: reload and
    try again.
  - Keeping a name of a map that cannot be read (another `APP_KEY`) is
    refused; replacing every value recovers it.
  - A nullable map can be cleared back to *not set* (Clear in the form).
  - After a 409 the form offers Reload.
  - The key/value bounds hold on the result.
- **Who sees what:** users who may read the field see the names. Users who
  may only write it see the state and can still replace everything.
- **Never allowed:** encrypted fields are always write-only, never sortable,
  searchable, filterable, a title, a key, a relationship, media, rich text, part
  of a unique constraint, a table column, a pivot field or a dashboard column.
  They are never exported or imported, and always redacted in the audit log.
  `SecretInput` and `SecretMapInput` edit only encrypted fields, and those
  fields take no other input. Anything else stops the boot.
- **In hooks and actions:** read the stored value with
  `model.env.open(ctx.keyring()?, &context)`, and write it with
  `Encrypted::seal`. `rustcms key:reencrypt` re-seals them after a key
  rotation ([rotating the key](../digging-deeper/encryption.md#rotating-the-key)).

## Checked at boot

Besides the layout and encrypted-field rules above, the boot stops when:

- an input names something that is not a field;
- a `BelongsToManySelect` does not name a belongs-to-many relation, shares its
  name with a field, appears twice, has options or a widget, targets the media
  library, or its relation has `.required()` pivot fields;
- a `CustomInput` has an invalid widget name, or props over 4 KiB;
- a key/value field is not a JSON column;
- `min`, `max` or `step` is on an input that is not a `NumberInput`, or on a
  column that does not hold numbers; `min` is above `max`; the step is not
  positive; or two inputs of one field have different bounds;
- a repeater's row is empty, holds tabs, splits, media, rich text, secrets or
  belongs-to-many inputs, or a select without options; a row input repeats a
  name; repeaters nest deeper than three; `item_label` names no input of the
  row; `min_items` or `default_items` is above `max_items`; the column is not
  JSON; `required`, `max_length` or `integer` is on an input outside a JSON
  row; repeater options are on an input that is not a repeater;
- a condition names something that is not an input of the form, or an
  encrypted field; `is_true` is on a field that is not a boolean; `eq`, `ne`
  or `is_in` is on a JSON, rich text, media, date-time or key/value field, or
  compares with a value that is not of the field's kind; conditions depend on
  each other in a cycle; a `BelongsToManySelect` or an encrypted field has a
  condition; a relationship field has `clear_when_hidden`;
- an input under a condition has a column that is not nullable and has no
  default (a record created while it is hidden could not be stored: declare a
  default or make the column nullable); `clear_when_hidden` is on an input
  without a condition, or on a non-nullable number, date or boolean column
  without a declared default;
- `.reaction_timeout()` is on an input without `.reacts()`;
  the timeout is zero or over 30 seconds; `.reacts()` is on a hidden,
  encrypted or link input, an input that is not a field (or a relationship
  repeater), an input in a repeater's row, or a settings input;
- `.options_fn()` is used where it is refused
  ([above](#options-computed-on-the-server)), `.depends_on()` or
  `.options_timeout()` comes without it, or the timeout is zero or over 30
  seconds.

[Settings pages](settings.md) follow the same layout rules.

## Related pages

- [Resources](resources.md), [tables](tables.md), [relationships](relationships.md)
- [Media and rich text](media.md), [custom fields](custom-fields.md)
- [Encrypted columns](../digging-deeper/encryption.md)
- [Authorization](authorization.md)
