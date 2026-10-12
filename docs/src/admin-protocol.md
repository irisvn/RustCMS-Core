# Admin protocol

Contract between the rustcms server and admin clients. `schemaVersion` and
`vocabularyVersion` are bumped on any breaking change.

## Endpoints

All under `<admin_path>/api` (default `/admin/api`), behind the admin session
guard (CSRF and `Origin` checks on writes, see Security). Every response,
errors included, is `Cache-Control: private, no-store`; unknown paths under
`/api` are JSON 404.

| Method | Path | Response |
|---|---|---|
| GET | `/panel` | `PanelResponse` `{brand, user, navigation: [ResourceInfo]}` (resources with `view_any`) |
| GET | `/resources/{slug}/schema?operation=create\|edit\|view&key=` | `ResourceSchema` for this user |
| GET | `/resources/{slug}/records?…` | `{data: [Envelope], meta}` |
| POST | `/resources/{slug}/records` | 201 `{data: Envelope}` |
| GET / PATCH / DELETE | `/resources/{slug}/records/{key}` | `{data: Envelope}` / `{data: Envelope}` / 204 |
| POST | `/resources/{slug}/actions/{action}` `{keys, data?}` | `{processed, denied, missing}` |
| GET | `/resources/{slug}/fields/{field}/options?search=&keys=a,b&record=` | `{data: [{value, label}]}` (≤ 50) |
| … | `/system/…` | users, roles and permissions, see [Users, roles & account](admin/users-roles.md) |

Options are available to users who may use the field. When a record-level
policy hides more than 1,000 candidates before 50 visible ones are found, the
endpoint answers 422 on `search`: refine the search. Edit and view forms send
`record=<key>`: the record is loaded in the user's scope and the policy decides
on it (`update` with a writable field, or `view` with a readable one), exactly
as for the form and the update. Without `record`: read the field with
`view_any`, or write it with `create` or `update`. Which records appear is
decided by the target resource's scope and policy.

### List query string

`page`, `per_page` (≤ 100), `sort` (`title` or `-title`), `search`
(case-insensitive as the database folds case: Unicode on PostgreSQL, ASCII on
SQLite; `%` and `_` are literal), and filters
`filter[<field>][<op>]=<value>` with `op` in `eq`, `in`, `gt`, `gte`, `lt`,
`lte`. Values are read by field type: `filter[published][eq]=true`,
`filter[category_id][in]=1,2`. JSON clients use the strict wire format.

### Schema

`ResourceSchema` = `{schemaVersion, vocabularyVersion, resource, operation,
abilities, form, table}`. It contains only what the user may see: inputs for
fields they may write (create), readable fields with `disabled` set when not
writable (edit), and actions they may run. The reference payload is
[`fixtures/contract/posts.schema.json`](https://github.com/irisvn/rustcms/blob/main/fixtures/contract/posts.schema.json),
checked by the server tests and shared with the admin SPA. Each input carries
`valueType` (the wire kind), `nullable` (an emptied input sends `null`; for text
kinds that are not nullable, `""`) and client `rules` (`required`, `max_length`
in characters); the server re-validates everything. List pages request
`operation=view` without a key. Inputs marked `writeOnly` (writable but not readable, e.g. a
password) carry no current value; omit them from the payload to keep the stored value.

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

- `data` holds readable fields only, and is empty (no `title`, no `labels`) when
  the user may not view the record, e.g. the result of an update-only write.
- `abilities` also holds one entry per action (`"publish": true`).
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
| 403    | `csrf_mismatch` (missing or stale `X-CSRF-Token`: fetch `/auth/csrf` again; do not replay blindly) |
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

## Client behaviour

- Fetch `/auth/csrf` before the first write of a page load and after sign-in or
  sign-out (both rotate the token); keep it in memory only. It answers
  `{csrfToken, userId}`: if `userId` is not the user the page was built for (the
  cookie changed accounts in another tab), discard the page's data and drafts
  instead of writing.
- Create forms start from each input's `default` (the entity's declared column
  default) and send those plus what the user filled in; other untouched inputs are
  omitted so database defaults apply. Nullable booleans have three states.
- `json` values are edited as text in the browser: integers beyond ±2^53 cannot
  be represented there, so such values are shown read-only and new ones are
  refused (store large identifiers as strings).
- A write answered by 5xx/408, a cut-off body or a dropped connection has an
  unknown outcome: the panel says so and blocks resubmission until the page is reloaded.
- `schemaVersion` newer than the client: render read-only. `vocabularyVersion`
  newer: render unknown components as read-only text and never submit them.
- TypeScript types for every DTO are generated from the server
  (`admin-ui/src/api/types.gen.ts`).
