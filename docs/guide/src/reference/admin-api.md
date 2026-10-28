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
| GET | `/panel` | `PanelResponse` `{brand, user, navigation: [ResourceInfo], system, settings, pages, home?, permissions, uploads?}`: resources with `view_any` (hidden ones too, `nav: false`; optional `sort`), settings pages with `view`, custom pages the user may open (optional `sort`), the home page's slug when the user may open it, the declared admin permissions the user holds, and `uploads` (`{maxBytes, maxConcurrent}`) when the media library is enabled and the user has `media.create` |
| GET | `/resources/{slug}/schema?operation=create\|edit\|view&key=` | `ResourceSchema` for this user |
| GET | `/resources/{slug}/records?…` | `{data: [Envelope], meta}` |
| POST | `/resources/{slug}/records` | 201 `{data: Envelope}` |
| GET / PATCH / DELETE | `/resources/{slug}/records/{key}` | `{data: Envelope}` / `{data: Envelope}` / 204 |
| POST | `/resources/{slug}/records/{key}/restore` | `{data: Envelope}` (soft deletes; trashed records only) |
| DELETE | `/resources/{slug}/records/{key}/force` | 204 (soft deletes; trashed records only) |
| GET | `/resources/{slug}/records/{key}/relations/{relation}?…` | `{data: [Envelope], meta, pivot?}` of the target (list query string; `pivot`: readable belongs-to-many pivot columns by target key) |
| POST | `/resources/{slug}/records/{key}/relations/{relation}` | 201 `{data: Envelope}` of the target, foreign key set by the server |
| POST | `/resources/{slug}/records/{key}/relations/{relation}/attach` `{keys, pivot?}`, `…/detach` `{keys}` | `{processed, missing}`: belongs-to-many links, see [Relationships](../admin/relationships.md#belongs-to-many-pivot-tables) |
| PATCH | `/resources/{slug}/records/{key}/relations/{relation}/links/{target}` `{…pivot columns}` | `{processed, missing}`: one link's pivot values (`update_pivot_{relation}`) |
| POST | `/resources/{slug}/actions/{action}` `{keys, data?}` | `{processed, denied, missing, replayed?, message?, open?}`: a run on one record hidden by `visible_when` is 403 (nothing runs), hidden records of a run on several are `denied`; `open` only on a run on one record (422 `one_record_only` otherwise) |
| POST | `/media` (multipart, one `file` part) | 201 `{data: MediaItem}` (with `alt`, its alternative text; media library enabled; the media schema's `upload` ability says whether the user may). A media record's `name` and `alt` are changed with `PATCH /resources/media/records/{id}` (`media.update`) |
| GET | `/media/{id}/file` | the file (authorized, safe headers) |
| GET | `/resources/{slug}/fields/{field}/options?search=&keys=a,b&record=` | `{data: [{value, label}]}` (≤ 50) |
| GET | `/resources/{slug}/actions/{action}/fields/{field}/options?search=&keys=` | the same, for a field of an action's form, see [Actions](../admin/actions.md#actions-with-a-form) |
| GET | `/resources/{slug}/export?…`, `/exports`, `/exports/{id}/file`; POST `/resources/{slug}/exports?…` | CSV exports at once or in the background, see [Import and export](../admin/import-export.md#exporting) |
| POST | `/resources/{slug}/imports`, `/imports/{id}/preview`, `/imports/{id}/run`; GET `/imports/{id}` | CSV imports, see [Import and export](../admin/import-export.md#importing) |
| GET | `/dashboard`, `/dashboard/widgets/{name}` | the user's widgets and their values, see [Dashboard](../admin/dashboard.md#api) |
| GET | `/resources/{slug}/records/{key}/relations/{relation}/options?search=&keys=` | `{data: [{value, label, attached}]}`: records to attach to a record (`attach_{relation}` on it) |
| GET | `/resources/{slug}/relations/{relation}/options?search=&keys=` | `{data: [{value, label, attached: false}]}`: records to attach in a create form's [`BelongsToManySelect`](../admin/relationships.md#choosing-links-in-the-records-form) (`create` and `attach_{relation}`) |
| GET / POST | `/system/users` | see [System routes](#system-routes) |
| GET / PATCH / DELETE | `/system/users/{id}` | |
| PUT | `/system/users/{id}/roles`, `…/active`, `…/password` | |
| POST | `/system/users/invitations`, `/system/users/{id}/invitation` | |
| GET / POST | `/system/roles` | |
| GET / DELETE | `/system/roles/{id}` | |
| PUT | `/system/roles/{id}/permissions` | |
| GET | `/system/permissions`, `/system/permission-presets` | |
| GET / PUT | `/settings/{group}` | settings pages, see [Settings](../admin/settings.md#api) |
| POST | `/settings/mail/test` `{to}` | sends a test mail with the saved settings (`mail` feature; whoever may update the mail settings), see [Settings](../admin/settings.md#mail) |
| GET | `/audit`, `/resources/{slug}/records/{key}/history` | the audit log and record histories, see [Audit log](../digging-deeper/audit-log.md#viewing-the-log) |
| GET | `/queue` | counts per queue and status, worker settings, `canManage` (`queue.view`) |
| GET | `/queue/jobs?status=&queue=&kind=&since=&until=&cursor=&per_page=` | `{data: [QueueJob], meta: {nextCursor?}}`, newest first (`queue.view`); `since`/`until` RFC 3339 on dispatch time |
| GET | `/queue/history?status=done\|failed\|cancelled&queue=&kind=&since=&until=&cursor=&per_page=` | `{data: [JobRun], meta: {nextCursor?}}`, finished jobs newest first (`queue.view`) |
| GET | `/queue/jobs/{id}` | `{job?: QueueJob, runs: [JobRun]}` (at most 20 runs); 404 when neither exists (`queue.view`) |
| GET | `/schedules` | `{data: [ScheduleInfo], scheduler, workers, canManage}` (`schedules.view`), see [Scheduling](../digging-deeper/scheduling.md#the-schedules-page) |
| GET | `/schedules/{name}/runs?cursor=&per_page=` | `{data: [ScheduleRunInfo], meta}`, newest first; 404 for an unknown name (`schedules.view`) |
| POST | `/schedules/{name}/run` | 202 `{run}`; 409 `write_conflict` reason `running`; 404 unknown (`schedules.view` + `schedules.manage`, rechecked under the RBAC lock; audited) |
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

## System routes

Users, roles and permissions of the admin guard, used by the System pages
([Users, roles & account](../admin/users-roles.md)). Bodies are camelCase and
refuse unknown fields. Every change goes through `RbacService`, which checks
the permission again, locks, audits and refuses changes to accounts or roles
the user may not manage (privileged roles, super admins, roles they hold).
Writes that start from what the editor saw send it back (`expected…`): if it
changed meanwhile the write is refused with 422 and the form issue `stale`,
and nothing is saved.

| Method | Path | Body | Response | Needs |
|---|---|---|---|---|
| GET | `/system/users?search=&page=&per_page=` | | `{data: [SystemUser], meta}` sorted by name; `per_page` 25 by default, at most 100; `search` (name or email) at most 100 characters | `users.view_any` |
| POST | `/system/users` | `{name, email, password, roleIds?}` | 201 `SystemUser` | `users.create` (`roles.assign` to give roles) |
| GET | `/system/users/{id}` | | `SystemUser` | `users.view`, or the user themselves |
| PATCH | `/system/users/{id}` | `{name, email, expected: {name, email}, currentPassword?}` | `SystemUser` | the user themselves (changing one's own email needs `currentPassword`); for another user `users.update` and `users.view` or `users.view_any`, and changing their email needs a super admin of every guard |
| DELETE | `/system/users/{id}` | | 204 | `users.delete`; another user only |
| PUT | `/system/users/{id}/roles` | `{roleIds, expectedRoleIds}` | `SystemUser` | `roles.assign` and `users.view` or `users.view_any`; another user only |
| PUT | `/system/users/{id}/active` | `{active, expected}` | `SystemUser` | `users.update` and `users.view` or `users.view_any`; another user only |
| PUT | `/system/users/{id}/password` | `{password}` | 204 | `users.update` and a super admin of every guard; another user only (your own password: `POST <admin_path>/api/auth/password`) |
| POST | `/system/users/invitations` | `{name, email, roleIds?}` | 201 `SystemUser` (pending) | `users.create` and `users.invite` (`roles.assign` to give roles); 409 `mail_disabled` when mail is disabled |
| POST | `/system/users/{id}/invitation` | | `SystemUser`; a new invitation mail is queued, and the old link stops working when the new one is issued | `users.invite` and `users.view` |
| GET | `/system/roles` | | `[SystemRole]` sorted by name; permissions only with `roles.view_any` | `roles.view_any` or `roles.assign` |
| POST | `/system/roles` | `{name, permissions?}` | 201 `SystemRole` | `roles.create` |
| GET | `/system/roles/{id}` | | `SystemRole` | `roles.view` or `roles.view_any` |
| DELETE | `/system/roles/{id}` | | 204 (system roles are never deleted) | `roles.delete` |
| PUT | `/system/roles/{id}/permissions` | `{permissions, expectedPermissions}` | `SystemRole` | `roles.update` and `roles.view` or `roles.view_any` |
| GET | `/system/permissions` | | `[{name, group?, groupLabel?, grantable, resource?, ability?, label?, description?, area, areaLabel, areaSort}]` | `roles.view_any` or `permissions.view_any` |
| GET | `/system/permission-presets` | | `[{id, label, description?, permissions}]` | `roles.view_any` or `permissions.view_any` |

- `SystemUser` is `{id, name, email, isActive, invitationPending, invitedAt?,
  roles: [{id, name}], superAdmin, isSelf, abilities}`; `SystemRole` is `{id,
  name, isSystem, isPrivileged, permissions, users, abilities}`. Ids are
  strings. `abilities` tells the page which buttons to show; the server
  decides again on each request.
- Roles: creating one needs every permission it grants; deleting one needs
  its active permissions; changing a role's permissions needs every
  permission added or removed; giving or taking a role needs that role's
  permissions. Nobody changes a role they hold.
- After a change, a caller who may not read the user (no `users.view`, and not
  themselves) gets 204 instead of the record.
- Creating a user and resetting a password hash a password: each has its own
  per-minute quota per acting user (creation: `auth.attempts_per_ip`; reset:
  `auth.attempts_per_identifier`), separate from the sign-in throttle (429
  beyond). Invitations
  are limited to 20 a minute per user, resends to 5.
- Only roles of the admin guard are listed and managed here; API-guard roles
  are managed with `rustcms role:assign`
  ([Limitations](limitations.md#not-supported-yet)).

## Other routes under the same prefix

- **Authentication** of the panel lives at `<admin_path>/api/auth/…`: `csrf`,
  `login`, `logout`, `me`, `password`, `logout-others`, `locale`,
  `forgot-password`, `reset-password`, `invitation`. See
  [Authentication](../security/authentication.md).
- **Your own JSON routes** for panel pages are declared with `AdminRoutes` and
  live at `<admin_path>/api/x/…`, including streams (`AdminRoutes::events`)
  and sockets (`AdminRoutes::socket`). Nothing built in lives there. See
  [Admin routes](../admin/admin-routes.md) and
  [Realtime](../admin/realtime.md).

## Lists

`page`, `per_page` (≤ 100; the panel offers 10, 25, 50 and 100), `sort` (`title` or `-title`), `search`
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
  One exception on a record the user may view: an encrypted field the user may
  only write is sent as its masked state (`state`, `revision`, never the key
  names of a map) so the form can replace it.
- `abilities` also holds one entry per action (`"publish": true`), decided by the policy and the action's `visible_when` on that record.
- `labels` holds titles of related records. A related record the user may not
  see is shown as `#<key>`.
- `trashed: true` marks a record in the trash (soft deletes; absent otherwise).
  Its abilities are `view`, `restore` and `force_delete` only; an active
  record of a soft-delete resource has `restore` and `force_delete` false.

## Retrying writes safely

Creates (`POST …/records`, and `POST …/relations/{relation}` on a record),
`POST …/actions/{action}`, media uploads (`POST /media`), import uploads
(`POST …/imports`) and background exports (`POST …/exports`) accept an
`Idempotency-Key` header (1–128 of `A-Z a-z 0-9 _ -`, otherwise 422 on
`idempotency_key`). The key is claimed in the write's transaction, so the write
and its receipt commit together or not at all. Repeating the request with the
same key, by the same user, within 24 hours:

- same request (body compared canonically: key order does not matter) → the
  first result, not applied again: a create answers its record as the user may
  see it **now** (key only when no longer visible); an action answers the first
  run's outcome with `"replayed": true`; an upload, import or background
  export answers the same media item, import or export. Admission is checked again (403 if the
  user lost the ability). Action outcomes, first or replayed, list processed
  and denied keys only for records the user may view now, and drop `message`
  and `open` unless the user may still view every processed record;
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
| keys per field-options lookup (`keys=`) | 100 |
| keys per attach or detach  | 100 |
| options per lookup         | 50 |
| rows checked one by one    | 10,000 |

- **Rows checked one by one.** A list, an export or a dashboard count reads
  the matching rows in batches and checks each when the policy decides record
  by record (`view_depends_on_record()`), when the list includes trashed
  records (`trashed=with|only`), and, on SQLite, for dashboard counts over
  date ranges. Past 10,000 matching rows:
  - a list or an export at once answers 422 `too_many_matches`;
  - a dashboard widget answers 200 with `{"kind": "too_many"}`;
  - a background export fails and is reported as `revoked`.

  Narrow the search or filters. Other lists and counts (a policy that decides
  once for every record, active records, row filtering done by `scope()`) are
  counted by the database, with no such limit.
- **After a write.** A committed create or update answers with the record as
  the user may see it. If reading it back takes more than 2 seconds (or would
  pass the request deadline), the answer holds the record's key alone; the
  write is committed either way.

