# Wire format

How values travel as JSON between RustCMS and its clients: the admin panel's
API, values given to [custom widgets](../admin/custom-fields.md), and the JSON
that `make:api` generates. Read it when you write a client, a test or a custom
widget. The rule: never lose precision.

## Kinds

Every column the admin exposes gets a kind from its SeaORM column type. The
schema sends the kind as each field's `valueType`.

| Kind | `valueType` | Column types | JSON |
|---|---|---|---|
| `String` | `string` | `char`, `varchar`, `text` | string |
| `I32` | `i32` | `integer` | number |
| `I64` | `i64` | `bigint` | **string** (JavaScript numbers lose precision past 2⁵³) |
| `F64` | `f64` | `float`, `double` | number |
| `Bool` | `bool` | `boolean` | boolean |
| `Decimal` | `decimal` | `decimal(p, s)`, `money` | **string**, at the column scale (`"10.50"`) |
| `Uuid` | `uuid` | `uuid` | lowercase hyphenated string |
| `Date` | `date` | `date` | `"YYYY-MM-DD"` |
| `LocalDateTime` | `local_datetime` | `datetime`, `timestamp` | `"YYYY-MM-DDTHH:MM:SS[.fff]"`, no time zone |
| `Instant` | `instant` | `timestamp with time zone` | RFC 3339; accepted with any offset, written in UTC (`Z`) |
| `Json` | `json` | `json`, `jsonb` | any JSON |

Decimals are rounded and rescaled to the column's scale before they are sent,
so a value reads the same on every database (SQLite, which stores decimals as
REAL, would otherwise drop trailing zeros).

## Decoding

Decoding is strict: a value of the wrong JSON shape is a 422 on its field,
never a silent conversion.

- A number sent for an `I64` field, or a decimal sent as a number, is refused
  (`must be a string`).
- An `I32` value outside the 32-bit range is `out of range`.
- `null` is accepted only for nullable columns.
- In a query string (list filters, `filter[price][gte]=10`), every value is
  text: `I32`, `F64`, `Bool` and `Json` values are parsed from it, the others
  are read as the strings they already are. See
  [Admin API: lists](admin-api.md#lists).

## Keys

Record keys are always strings in URLs and envelopes (`"42"`, a UUID, or a
natural key). A primary key must be an `integer`, `bigint`, text or `uuid`
column, and a single column: a resource on anything else stops the boot.

## Values that never travel as such

- **Encrypted fields** (`Field::encrypted`, `Field::encrypted_map`) never
  carry their value: a record holds a state object (`{"state": "set"}`, or
  the names of a map) and writes send replacements. See
  [Encrypted fields](../admin/forms.md#encrypted-fields).
- **Rich text** is a `Json` column holding a closed, versioned document,
  validated and bounded on the server; HTML is never stored. See
  [Rich text](../admin/media.md#rich-text).
- **Fields the user may not read** are absent from `data`; undeclared columns
  are never sent. The one exception is an encrypted field the user may only
  write: on a record they may view it is sent as its masked state, never its
  value or a map's names.

## Unsupported column types

These column types have no kind, so they cannot be admin fields. Declaring a
field on one stops the boot (`unknown or unsupported column`); a column of
such a type that is not declared as a field is simply left out.

- `smallint`, `tinyint` (Rust `i16`, `i8`) and every unsigned type (`u8` …
  `u64`): use `integer` or `bigint`;
- `time` without a date, `interval`;
- binary columns (`binary`, `varbinary`, `blob`, `bytea`);
- enums (database `ENUM` types) and arrays: store an enum as a string and
  bound it in a hook;
- network, geometric and other database-specific types.

JSON columns edited in a browser cannot hold integers beyond ±2⁵³ exactly:
store large identifiers as strings.

## Related pages

- [Admin API](admin-api.md)
- [Errors](../basics/errors.md) and [Error codes](error-codes.md)
- [Custom fields](../admin/custom-fields.md)
- [Limitations](limitations.md)
