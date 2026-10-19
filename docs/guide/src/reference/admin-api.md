# Admin API

The admin panel talks to the server through a JSON API under
`<admin_path>/api` (default `/admin/api`). You rarely call it yourself, but it
is what [resources](../admin/resources.md) expose, and its rules are the rules
of every client: tests, scripts, a custom front end.

Every request uses the admin session (cookie) and, for writes, the CSRF token
and an allowed `Origin` ([Authentication](../security/authentication.md#csrf)).
Responses, errors included, are `Cache-Control: private, no-store`; unknown
paths under `/api` are JSON 404. Errors follow the [standard format](../basics/errors.md);
values follow the [wire format](wire-format.md).

## Endpoints

| Method | Path | Response |
|---|---|---|
| GET | `/panel` | `PanelResponse` `{brand, user, navigation: [ResourceInfo], system, settings, pages, home?, permissions}`: resources with `view_any` (hidden ones too, `nav: false`; optional `sort`), settings pages with `view`, custom pages the user may open (optional `sort`), the home page's slug when the user may open it, the declared admin permissions the user holds |
| GET | `/resources/{slug}/schema?operation=create\|edit\|view&key=` | `ResourceSchema` for this user |
| GET | `/resources/{slug}/records?…` | `{data: [Envelope], meta}` |
| POST | `/resources/{slug}/records` | 201 `{data: Envelope}` |
| GET / PATCH / DELETE | `/resources/{slug}/records/{key}` | `{data: Envelope}` / `{data: Envelope}` / 204 |
| POST | `/resources/{slug}/records/{key}/restore` | `{data: Envelope}` (soft deletes; trashed records only) |
| DELETE | `/resources/{slug}/records/{key}/force` | 204 (soft deletes; trashed records only) |
| GET | `/resources/{slug}/records/{key}/relations/{relation}?…` | `{data: [Envelope], meta, pivot?}` of the target (list query string; `pivot`: readable belongs-to-many pivot columns by target key) |
| POST | `/resources/{slug}/records/{key}/relations/{relation}` | 201 `{data: Envelope}` of the target, foreign key set by the server |
| POST | `/resources/{slug}/actions/{action}` `{keys, data?}` | `{processed, denied, missing}` |
| POST | `/media` (multipart, one `file` part) | 201 `{data: MediaItem}` (media library enabled) |
| GET | `/media/{id}/file` | the file (authorized, safe headers) |
| GET | `/resources/{slug}/fields/{field}/options?search=&keys=a,b&record=` | `{data: [{value, label}]}` (≤ 50) |
| … | `/system/…` | users, roles and permissions, see [Users, roles & account](../admin/users-roles.md) |
| GET / PUT | `/settings/{group}` | settings pages, see [Settings](../admin/settings.md#api) |
| GET | `/audit`, `/resources/{slug}/records/{key}/history` | the audit log and record histories, see [Audit log](../digging-deeper/audit-log.md#viewing-the-log) |
| GET | `/queue` | counts per queue and status, worker settings, `canManage` (`queue.view`) |
| GET | `/queue/jobs?status=&queue=&kind=&cursor=&per_page=` | `{data: [QueueJob], meta: {nextCursor?}}`, newest first (`queue.view`) |
| GET | `/pages/{slug}` | `{slug, title, content}` of a custom page: `{kind: "html", html}` or `{kind: "script", script, data}`, see [Customizing](../admin/customizing.md#custom-pages) |
| GET | `/pages/{slug}/{*path}` | the same response for a sub-page; `Ctx::page_path()` gives the canonical sub-path; an invalid sub-path is a 422 on `path`, after the permission checks |
| GET | `/slots?resource=&key=` | `{data: [{name, label?, content}]}` beside a record |
| POST | `/queue/jobs/{id}/retry`, `/forget`, `/cancel` | 204; 409 `write_conflict` with reason `not_failed` (retry, forget), `running` or `not_waiting` (cancel) (`queue.view` + `queue.manage`), see [Queues](../digging-deeper/queues.md#the-queue-page) |

Options are available to users who may use the field. When a record-level
policy hides more than 1,000 candidates before 50 visible ones are found, the
endpoint answers 422 on `search`: refine the search. Edit and view forms send
`record=<key>`: the record is loaded in the user's scope and the policy decides
on it (`update` with a writable field, or `view` with a readable one), exactly
as for the form and the update. Without `record`: read the field with
`view_any`, or write it with `create` or `update`. Which records appear is
decided by the target resource's scope and policy.


## Lists

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

## Records

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

## Retrying writes safely

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

