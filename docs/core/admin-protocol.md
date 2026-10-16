# Admin protocol

Contract between the rustcms server and admin clients. `schemaVersion` and
`vocabularyVersion` are bumped on any breaking change.

## Endpoints

All under `<admin_path>/api` (default `/admin/api`), behind the admin session
guard (CSRF and `Origin` checks on writes, see Security). Every response,
errors included, is `Cache-Control: private, no-store`; unknown paths under
`/api` are JSON 404.

Labels (resources, fields, options, tabs, actions, permission groups) come in
the language the panel shows (`X-Rustcms-Locale`), else the user's saved
choice, else `Accept-Language`, else `app.locale` ([Languages](../guide/src/admin/i18n.md)). `GET /auth/me` returns `locale`,
`preferredLocale` and `locales`; `POST /auth/locale {"locale": "vi" | null}`
saves the choice (422 for a language not offered). The shell's
`<meta name="rustcms-admin">` lists `locale` and `locales` for the sign-in page.

| Method | Path | Response |
|---|---|---|
| GET | `/panel` | `PanelResponse` `{brand, user, navigation: [ResourceInfo]}` (resources with `view_any`) |
| GET | `/resources/{slug}/schema?operation=create\|edit\|view&key=` | `ResourceSchema` for this user |
| GET | `/resources/{slug}/records?…` | `{data: [Envelope], meta}` |
| POST | `/resources/{slug}/records` | 201 `{data: Envelope}` |
| GET / PATCH / DELETE | `/resources/{slug}/records/{key}` | `{data: Envelope}` / `{data: Envelope}` / 204 |
| POST | `/resources/{slug}/records/{key}/restore` | `{data: Envelope}` (soft deletes; trashed records only) |
| DELETE | `/resources/{slug}/records/{key}/force` | 204 (soft deletes; trashed records only) |
| GET | `/resources/{slug}/records/{key}/relations/{relation}?…` | `LinkPage` `{data: [Envelope], meta, pivot?}` of the target (list query string; `pivot`: readable pivot columns by target key, belongs-to-many) |
| POST | `/resources/{slug}/records/{key}/relations/{relation}` | 201 `{data: Envelope}` of the target, foreign key set by the server (has-many; belongs-to-many: 405) |
| POST | `…/relations/{relation}/attach` `{keys, pivot?}` | `LinkOutcome {processed, missing}`; admitted by `attach_{relation}` before the body (403), then 422 (1–100 keys, pivot by field rules), then parent 404/403 |
| POST | `…/relations/{relation}/detach` `{keys}` | `LinkOutcome`; `detach_{relation}`; links to targets not visible are kept and listed in `missing` |
| PATCH | `…/relations/{relation}/links/{target}` `{…pivot}` | `LinkOutcome`; `update_pivot_{relation}`; 404 when not linked or not visible |
| GET | `…/relations/{relation}/options?search=&keys=` | `{data: [LinkOption {value, label, attached}]}`; `attach_{relation}` admitted, then on the parent record |
| POST | `/resources/{slug}/actions/{action}` `{keys, data?}` | `{processed, denied, missing}`; admitted before the body; `data` checked against the action's form (none: refused) |
| GET | `/resources/{slug}/export?{list query}` | `text/csv` file (≤ 1 000 rows, else 422 `too_many_to_export`); `export` + `view_any` admitted first; 404 when not exportable |
| POST | `/resources/{slug}/exports?{list query}` | 201 `ExportView`; background job (needs storage), ≤ 3 waiting per user (429) |
| GET | `/exports` | `[ExportView]` of the user (not expired, newest first) |
| POST | `/resources/{slug}/imports` (CSV body) | 201 `ImportView`; `import` + `create` (or `update` with an import key) admitted before the body; 422 `file`; ≤ 3 waiting (429) |
| GET | `/imports/{id}` | `ImportView` (owner only, else 404) |
| POST | `/imports/{id}/preview` `{mapping}` | `ImportPreview {rows, valid, keyed, report}`; no write, no hook |
| POST | `/imports/{id}/run` `{mapping}` | `ImportView` (running); once, then 409 `import_started` |
| GET | `/exports/{id}/file` | `text/csv`; owner only (else 404), 409 `export_not_ready` / `export_stale` (credentials, permission, fields or any record no longer visible) |
| GET | `/resources/{slug}/actions/{action}/fields/{field}/options?search=&keys=` | `{data: [SelectOption]}` of a form's `belongs_to` field (keys as text) |
| POST | `/media` (multipart, one `file` part) | 201 `{data: MediaItem}` (media library enabled) |
| GET | `/media/{id}/file` | the file (authorized, safe headers) |
| GET | `/resources/{slug}/fields/{field}/options?search=&keys=a,b&record=` | `{data: [{value, label}]}` (≤ 50) |
| … | `/system/…` | users, roles and permissions, see [Users, roles & account](../guide/src/admin/users-roles.md) |
| GET / PUT | `/settings/{group}` | `SettingsResponse`; PUT `{version, data}` (see below) |
| POST | `/settings/mail/test` | `{to}` → `{ok, code?}` (feature `mail`; `settings.mail.update` before the body; 5/min/user) |
| GET | `/audit?…` | `AuditPage` (`audit.view_all`; auth families with `audit.view_auth`) |
| GET | `/resources/{slug}/records/{key}/history?cursor=&per_page=` | `AuditPage` of the record (`audit.view` + record `view`) |
| GET | `/pages/{slug}` | `CustomPageResponse` (`content`: tagged `html` / `script`); 403 for users with no page, else 404 unknown / 403 not allowed; `data` only when allowed |
| GET | `/slots?resource=&key=` | `SlotsResponse`; unknown resource 404, then the resource's `admits(view)` (403) before 422 for unknown, repeated or missing parameters; record as `find` (404 / 403); slots without their permissions omitted |
| GET | `/queue` | `QueueOverview` (`queue.view`) |
| GET | `/queue/jobs?status=&queue=&kind=&cursor=&per_page=` | `QueueJobPage`, keyset `(created_at, id)` desc, exact-instant cursor (`queue.view`) |
| POST | `/queue/jobs/{id}/{retry,forget,cancel}` | 204; 409 `write_conflict`: `not_failed` (retry/forget of any non-failed job), `running` / `not_waiting` (cancel); 404 missing or malformed id; 403 without `queue.view` + `queue.manage` (checked before the id is parsed, at admission and again under the RBAC lock) |

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

`trashed=with|only` lists trashed records too, or only them, on resources with
soft deletes (`table.softDeletes`); elsewhere it is a 422. It needs the
`restore` or `force_delete` ability (schema `abilities.restore` /
`abilities.forceDelete`), otherwise 403, and a trashed record is listed only if
the policy allows `view` and `restore` or `force_delete` on it.

### Schema

`ResourceSchema` = `{schemaVersion, vocabularyVersion, resource, operation,
abilities, form, table, relations?}` (`schemaVersion` 2: actions may carry a
`form`; vocabulary 7: relations carry `kind`, `attach`, `detach`, `pivot`,
`attachForm`, `pivotForm`). It contains only what the user may see: inputs for
fields they may write (create), readable fields with `disabled` set when not
writable (edit), and actions they may run. The reference payload is
[`fixtures/contract/posts.schema.json`](https://github.com/irisvn/rustcms/blob/main/fixtures/contract/posts.schema.json),
checked by the server tests and shared with the admin SPA. Each input carries
`valueType` (the wire kind), `nullable` (an emptied input sends `null`; for text
kinds that are not nullable, `""`) and client `rules` (`required`, `max_length`
in characters); the server re-validates everything. List pages request
`operation=view` without a key. Inputs marked `writeOnly` (writable but not readable, e.g. a
password) carry no current value; omit them from the payload to keep the stored value.

Vocabulary 5 adds `secret` inputs (settings pages): the value is never sent;
`SettingsResponse.secrets` gives each one's state (`set`, `unset`,
`unreadable`), and a save sends a string (replace), `null` (clear) or nothing
(keep). `PanelResponse.settings` lists the pages the user may view.

### Settings

`GET /settings/{group}` answers `{schemaVersion, vocabularyVersion, group,
label, form, data, secrets, version, abilities: {update}}` (`version` is an
i64 string, `"0"` when never saved). `PUT` takes `{version, data}` with only
the settings to change; a stale version is a 422 form issue `stale`; an
unreadable secret not replaced or cleared is a 422 `unreadable_secret` on it.
Without `view` (and `update` for PUT) the answer is 403 before the body is
read; an unknown group is 404 only for users who may view some group.
Fixtures: `fixtures/contract/settings.json`, `settings.vi.json`.

Vocabulary 4 adds `custom` inputs: `widget` (`vendor/name`) and optional
`props` (JSON), edited by a widget from an extension script (see
[Custom fields](../guide/src/admin/custom-fields.md)); the value is the field's wire value.
`ResourceSchema.relations` (view schemas with a key, omitted when empty) lists
`{name, label, resource, field, create}` for each has-many relation whose
target the user may list (see [Relationships](../guide/src/admin/relationships.md)).

Vocabulary 3 adds `file_upload` inputs (`media: {multiple, maxItems, accept,
maxSize?}`; the value is a media id, or a list of ids) and `rich_editor`
inputs (the value is a rich text document, see
[Media, uploads & rich text](../guide/src/admin/media.md)).

Vocabulary 2 adds `key_value` inputs (a JSON object of strings; rules
`max_items` and `max_length` per key and value) and the `tabs` layout, whose
children are `tab` layouts with a stable `id` and a `label`. Clients that only
know vocabulary 1 render them as read-only text and sections.

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
- `trashed: true` marks a record in the trash (soft deletes; absent otherwise).
  Its abilities are `view`, `restore` and `force_delete` only; an active
  record of a soft-delete resource has `restore` and `force_delete` false.

## Errors

```json
{ "message": "The given data was invalid.", "code": "validation_failed",
  "errors": { "slug": ["has already been taken"] }, "formErrors": [],
  "issues": { "slug": [{ "code": "taken", "params": {} }] }, "formIssues": [] }
```

`issues` and `formIssues` give the stable code and parameters (text) of each
message, in the same order, for clients that translate them (codes:
[Languages](../guide/src/admin/i18n.md#validation-messages)); messages stay English. A 409
`write_conflict` carries a `reason` (`busy`, `upload_expired`,
`receipt_expired`).

| Status | `code`              |
|--------|---------------------|
| 401    | `unauthenticated`   |
| 403    | `forbidden`         |
| 403    | `csrf_mismatch` (missing or stale `X-CSRF-Token`: fetch `/auth/csrf` again; do not replay blindly) |
| 404    | `not_found` (also for records outside the user's scope; a user without the ability gets 403 for any key) |
| 413    | `payload_too_large` |
| 422    | `validation_failed` |
| 500    | `internal_error` (details are logged, never returned) |

## Idempotency keys

`POST …/records` (create) and `POST …/actions/{action}` accept an
`Idempotency-Key` header (1–128 of `A-Z a-z 0-9 _ -`, otherwise 422 on
`idempotency_key`). The key is claimed in the write's transaction, so the write
and its receipt commit together or not at all. Repeating the request with the
same key, by the same user, within 24 hours:

- same request (body compared canonically: key order does not matter) → the
  first result, not applied again: a create answers its record as the user may
  see it **now** (key only when no longer visible); an action answers the first
  run's outcome with `"replayed": true`. Admission is checked again (403 if the
  user lost the ability). Action outcomes, first or replayed, list processed
  and denied keys only for records the user may view now;
- different request → 422 on `idempotency_key`;
- the first request still running → the retry waits (up to 5 s), then answers
  409 `write_conflict` (nothing applied; retry later).

Keys are per user, resource and operation. At most 1,000 live keys per user
(429 beyond). `idempotency:prune` deletes expired receipts (`serve` does it
hourly). Updates and deletes take no key: repeating them converges.

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
  unknown outcome: the panel says so and blocks other writes to that resource
  until the page is reloaded. Creates and actions carry an `Idempotency-Key`
  (a new UUID per submission): the panel offers **Retry safely**, which re-sends
  the same request with the same key; only a success lifts the pause.
- `schemaVersion` newer than the client: render read-only. `vocabularyVersion`
  newer: render unknown components as read-only text and never submit them.
- TypeScript types for every DTO are generated from the server
  (`admin-ui/src/api/types.gen.ts`).
