# Admin protocol

Contract between the rustcms server and admin clients. `schemaVersion` and
`vocabularyVersion` are bumped on any breaking change.

## Wire format

| Kind            | Column types                     | JSON                                                    |
|-----------------|----------------------------------|---------------------------------------------------------|
| `String`        | `char`, `varchar`, `text`        | string                                                  |
| `I32`           | `integer`                        | number                                                  |
| `I64`           | `bigint`                         | **string** (JavaScript numbers lose precision past 2⁵³) |
| `F64`           | `float`, `double`                | number                                                  |
| `Bool`          | `boolean`                        | boolean                                                 |
| `Decimal`       | `decimal(p, s)`, `money`         | **string**, rendered with the column scale (`"10.50"`)  |
| `Uuid`          | `uuid`                           | lowercase hyphenated string                             |
| `Date`          | `date`                           | `"YYYY-MM-DD"`                                          |
| `LocalDateTime` | `datetime`, `timestamp`          | `"YYYY-MM-DDTHH:MM:SS[.fff]"`, no timezone              |
| `Instant`       | `timestamp with time zone`       | RFC 3339; accepted with any offset, rendered in UTC (`Z`) |
| `Json`          | `json`, `jsonb`                  | any JSON                                                |

Decoding is strict: a number sent for an `I64` field or a decimal sent as a
number is rejected with 422. `null` is accepted only for nullable columns.

Record keys are strings in URLs and envelopes (`"42"`, a UUID, or a natural key).

## Record envelope

```json
{
  "key": "0199d3a4-…",
  "title": "Hello",
  "abilities": { "delete": true, "update": true, "view": true },
  "data": { "title": "Hello", "author_id": "7", "price": "10.50", "category_id": 1 },
  "labels": { "category_id": "News" }
}
```

- `data` holds readable fields only.
- `labels` holds titles of related records. A related record the user may not
  see is shown as `#<key>`.

## Errors

```json
{ "message": "The given data was invalid.", "code": "validation_failed",
  "errors": { "slug": ["has already been taken"] }, "formErrors": [] }
```

| Status | `code`              |
|--------|---------------------|
| 401    | `unauthenticated`   |
| 403    | `forbidden`         |
| 404    | `not_found` (also for records outside the user's scope) |
| 413    | `payload_too_large` |
| 422    | `validation_failed` |
| 500    | `internal_error` (details are logged, never returned) |

## Limits

| Parameter                  | Default |
|----------------------------|---------|
| `per_page`                 | capped at 100 |
| `search`                   | 100 characters |
| filters per request        | 10 |
| items in an `in` filter    | 100 |
| keys per action or lookup  | 500 |
