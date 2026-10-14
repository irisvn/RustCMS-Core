# Forms

`fields()` define data and permissions; `form()` only describes presentation.
Without `form()`, the panel shows one input per field, chosen by column type.

```rust,ignore
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
| `DatePicker`, `DateTimePicker` | dates, local date-times and instants (edited in UTC) |
| `Hidden` | a value the user does not edit |
| `KeyValue` | a JSON object of text values ([below](#key-value)) |
| `FileUpload`, `RichEditor` | [media and rich text](media.md) |
| `CustomInput::make(field, "vendor/widget")` | a [custom widget](custom-fields.md) |

Common options: `.label(…)`, `.placeholder(…)`, `.help(…)`, `.column_span(n)`.
`SelectInput` is named so it does not clash with `sea_orm::Select`.

## Layouts

| Layout | Does |
|---|---|
| `Section::make(label)` | a titled group |
| `Grid::make(columns)` | columns without a title |
| `Tabs::make()` with `Tab::make(label)` children | tabs |

```rust,ignore
Tabs::make().schema(components![
    Tab::make("Content").schema(components![TextInput::make("title")]),
    Tab::make("Attributes").schema(components![KeyValue::make("specs")]),
])
```

Tabs hold only `Tab`s, and a `Tab` only sits in `Tabs`; a tab's id comes from
its label and must be unique among its siblings. Tabs the user cannot see
anything in are left out. On a failed save the panel opens the first tab with
an error and focuses its input.

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
