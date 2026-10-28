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
| `NumberInput` | integers and decimals (kept exact) |
| `SelectInput` | static `.options([(value, label)])`, or the records of a `belongs_to` field |
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
- a key/value field is not a JSON column.

[Settings pages](settings.md) follow the same layout rules.

## Related pages

- [Resources](resources.md), [tables](tables.md), [relationships](relationships.md)
- [Media and rich text](media.md), [custom fields](custom-fields.md)
- [Encrypted columns](../digging-deeper/encryption.md)
- [Authorization](authorization.md)
