# Forms

`fields()` define data and permissions; `form()` only describes presentation.
Without `form()`, the panel shows one input per field, chosen by column type.

```rust
fn form() -> Form {
    Form::new().columns(2).schema(components![
        Section::make("Content").schema(components![
            TextInput::make("title").placeholder("A catchy title"),
            Textarea::make("excerpt").column_span(2),
        ]),
        SelectInput::make("category_id"),          // options from the `belongs_to` target
        Toggle::make("published"),
        DateTimePicker::make("published_at"),
    ])
}
```

Each input names a [field](resources.md#fields); the form is built per user
and operation (create, edit, view): inputs for fields the user may not read or
write are left out or disabled, and the server checks everything again.

## Inputs

| Input | For |
|---|---|
| `TextInput` (`.email()`, `.password()`, `.url()`) | text |
| `Textarea` | long text |
| `NumberInput` | integers and decimals (kept exact) |
| `SelectInput` | static `.options([(value, label)])`, or the records of a `belongs_to` field |
| `Toggle`, `Checkbox` | booleans (three states when nullable) |
| `CheckboxList` | a JSON list of option values |
| `DatePicker`, `DateTimePicker` | dates, local date-times and instants (only instants are edited in UTC, local date-times as stored; shown on the record's page in the user's language, instants in their time zone, which is named) |
| `Hidden` | a value the user does not edit |
| `KeyValue` | a JSON object of text values ([below](#key-value)) |
| `FileUpload`, `RichEditor` | [media and rich text](media.md) |
| `CustomInput::make(field, "vendor/widget")` | a [custom widget](custom-fields.md) |
| `SecretInput`, `SecretMapInput` | [encrypted fields](#encrypted-fields) (chosen for them by default) |
| `BelongsToManySelect::make(relation)` | the links of a [belongs-to-many relation](relationships.md#choosing-links-in-the-records-form) |

Common options: `.label(…)`, `.placeholder(…)`, `.help(…)`, `.column_span(n)`.
`SelectInput` is named so it does not clash with `sea_orm::Select`.

## Leaving with unsaved changes

A create or edit form with changes not saved asks before the user leaves it:
the panel's dialog (*Keep editing* or *Discard changes*) for links and the
browser's Back button, the browser's own prompt for closing or reloading the
tab. It does not ask after a save, while a save is in flight or its outcome
is unknown (the resource is paused and the retry is offered on any page), or
when the session sends the tab elsewhere (sign-out, another user signing in).

## Layouts

| Layout | Does |
|---|---|
| `Section::make(label)` | a titled group |
| `Grid::make(columns)` | columns without a title |
| `Tabs::make()` with `Tab::make(label)` children | tabs |

```rust
Tabs::make().schema(components![
    Tab::make("Content").schema(components![TextInput::make("title")]),
    Tab::make("Attributes").schema(components![KeyValue::make("specs")]),
])
```

Tabs hold only `Tab`s, and a `Tab` only sits in `Tabs`; a tab's id comes from
its label and must be unique among its siblings. Tabs the user cannot see
anything in are left out. On a failed save the panel opens the first tab with
an error and focuses its input.

### An editor layout

For long content (a post, a page), lay the form out like an editor: the
whole width, a main column and a narrow aside with the details.

```rust
Form::new().width(FormWidth::Full).schema(components![Split::make()
    .main(components![
        TextInput::make("title"),
        TextInput::make("slug").prefix("/posts/"),
        RichEditor::make("body"),
        Section::make("SEO").collapsible(true).schema(components![
            TextInput::make("meta_title"),
            Textarea::make("meta_description"),
        ]),
    ])
    .aside(components![
        Section::make("Publishing").compact().schema(components![
            SelectInput::make("status"),
            DateTimePicker::make("published_at"),
        ]),
        FileUpload::make("cover"),
        BelongsToManySelect::make("tags"),
    ])])
```

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
prefix on a number input) panics when the application starts.

Misconfiguration (an input for an unknown field, a filter on a field that is
not `.filterable()`, a default sort on a field that is not `.sortable()`)
panics when the application starts.

## Key-value

`KeyValue` edits a JSON column holding an object of text values
(`{"size": "M"}`). The field is a key/value field when it declares
`.key_value()` or `.max_items(n)`, or a `KeyValue` input uses it; its bounds
then apply to every write, API and hooks included, whatever the form shows:
`max_items` pairs (default 50), keys up to 255 characters, values up to
`max_length` (default 1,000). A stored value that is not an object of text is
shown read-only and never rewritten; a nullable field can be cleared to "not
set" (distinct from an empty object).

## Encrypted fields

```rust
Field::column(integration::Column::ApiToken).encrypted(),
Field::column(integration::Column::Env).encrypted_map().max_items(20),
```

Both hold secrets sealed with `APP_KEY` in a text column; the model's field
is an [`Encrypted<T>`](../digging-deeper/encryption.md):
- `encrypted()` takes an `Encrypted<String>`;
- `encrypted_map()` takes an `Encrypted<BTreeMap<String, String>>`.

The panel never shows them.
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
  searchable, filterable, a title or a key, a pivot field or a dashboard
  column. They are never exported or
  imported, and always redacted in the audit log. Anything else panics at
  start.
- **In hooks and actions:** read the stored value with
  `model.env.open(ctx.keyring()?, &context)`, and write it with
  `Encrypted::seal`.

