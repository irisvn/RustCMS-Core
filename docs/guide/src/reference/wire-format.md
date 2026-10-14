# Wire format

How values travel as JSON between RustCMS and its clients: the admin panel's
API, values given to [custom widgets](../admin/custom-fields.md), and the JSON
that `make:api` generates. The rule: never lose precision.

| Kind | Column types | JSON |
|---|---|---|
| `String` | `char`, `varchar`, `text` | string |
| `I32` | `integer` | number |
| `I64` | `bigint` | **string** (JavaScript numbers lose precision past 2⁵³) |
| `F64` | `float`, `double` | number |
| `Bool` | `boolean` | boolean |
| `Decimal` | `decimal(p, s)`, `money` | **string**, at the column scale (`"10.50"`) |
| `Uuid` | `uuid` | lowercase hyphenated string |
| `Date` | `date` | `"YYYY-MM-DD"` |
| `LocalDateTime` | `datetime`, `timestamp` | `"YYYY-MM-DDTHH:MM:SS[.fff]"`, no time zone |
| `Instant` | `timestamp with time zone` | RFC 3339; accepted with any offset, written in UTC (`Z`) |
| `Json` | `json`, `jsonb` | any JSON |

Decoding is strict: a number sent for an `I64` field, or a decimal sent as a
number, is refused with 422. `null` is accepted only for nullable columns.
Record keys are strings (`"42"`, a UUID, or a natural key).

JSON columns edited in a browser cannot hold integers beyond ±2⁵³ exactly:
store large identifiers as strings.
