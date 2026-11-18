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
`<meta name="rustcms-admin">` lists `locale` and `locales` for the sign-in page,
`brand` and `theme` (`colorScheme`; `logo`, `logoDark`, `favicon` as
`assets/brand/{kind}?v={hash8}`; the theme stylesheet is the first of
`extensions.styles`, `assets/brand/theme.css?v=…`). With a `BrandingSource`
in runtime mode the page is rendered per request (static mode renders it once); the SPA shows that branding until a reload
(ADR 0033). `assets/brand/{kind}` always serves the current file of the kind
(immutable only when `v` is current, else `no-cache`).

| Method | Path | Response |
|---|---|---|
| GET | `/panel` | `PanelResponse` `{brand, user, navigation: [ResourceInfo], system, settings, pages, home?, permissions, uploads?}`: resources with `view_any`, in registration order (the SPA reorders by `sort`) (hidden ones too, `nav: false`; optional `sort`), settings pages with `view`, custom pages the user may open (optional `sort`), the home page's slug when the user may open it, the declared admin permissions the user holds, and `uploads` (`{maxBytes, maxConcurrent}`) when the media library is enabled and the user has `media.create` |
| GET | `/resources/{slug}/schema?operation=create\|edit\|view&key=` | `ResourceSchema` for this user |
| GET | `/resources/{slug}/records?…` | `{data: [Envelope], meta}` |
| POST | `/resources/{slug}/records` | 201 `{data: Envelope}` |
| GET / PATCH / DELETE | `/resources/{slug}/records/{key}` | `{data: Envelope}` / `{data: Envelope}` / 204 |
| POST | `/resources/{slug}/form/react` | `FormReactRequest` `{operation: "create" \| "edit", key?, changed, state, previous?, via?: {parent, record, relation}, seq}` → `FormReactResponse` `{seq, patches, failed?}`: the changes the reaction of input `changed` asks for (ADR 0043). Before the body is read: 403 unless the user may create or update records of the resource, and 120 requests per user per minute; then create: `create` (through `via`, as that relation creates), edit: `update` on the record, in scope; `view` is 422, as is an input without a reaction or a JSON repeater sent with more rows than it holds; 2 reactions at once per user and 8 in all (429 beyond) |
| GET | `/resources/{slug}/records/{key}/computed` | `ComputedValues` `{data: {name: value}, failed: [name]}`: the computed values the view page's infolist shows, for a user who may view the record (ADR 0039); 60 requests per user per minute and 8 pages computing at once (429 beyond) |
| POST | `/resources/{slug}/records/{key}/restore` | `{data: Envelope}` (soft deletes; trashed records only) |
| DELETE | `/resources/{slug}/records/{key}/force` | 204 (soft deletes; trashed records only) |
| GET | `/resources/{slug}/records/{key}/relations/{relation}?…` | `LinkPage` `{data: [Envelope], meta, pivot?}` of the target (list query string; `pivot`: readable pivot columns by target key, belongs-to-many) |
| POST | `/resources/{slug}/records/{key}/relations/{relation}` | 201 `{data: Envelope}` of the target, foreign key set by the server (has-many; belongs-to-many: 405) |
| POST | `…/relations/{relation}/attach` `{keys, pivot?}` | `LinkOutcome {processed, missing}`; admitted by `attach_{relation}` before the body (403), then 422 (1–100 keys, pivot by field rules), then parent 404/403 |
| POST | `…/relations/{relation}/detach` `{keys}` | `LinkOutcome`; `detach_{relation}`; links to targets not visible are kept and listed in `missing` |
| PATCH | `…/relations/{relation}/links/{target}` `{…pivot}` | `LinkOutcome`; `update_pivot_{relation}`; 404 when not linked or not visible |
| GET | `…/relations/{relation}/options?search=&keys=` | `{data: [LinkOption {value, label, attached}]}`; `attach_{relation}` admitted, then on the parent record |
| POST | `/resources/{slug}/actions/{action}` `{keys, data?}` | `{processed, denied, missing, replayed?, message?, open?}`; admitted before the body; `data` checked against the action's form (none: refused; `ActionSchema.submitLabel` names its button); one record hidden by the action's predicate: 403; `message` and `open` come from the handler and are checked before the commit: `open` on a run over several records is a 422 form issue `one_record_only`; an `open` that is not `https://…` or a path of the site, or a `message` over 500 characters, is a handler bug: 500, rolled back |
| GET | `/resources/{slug}/export?{list query}` | `text/csv` file (≤ 1 000 rows, else 422 `too_many_to_export`; ≤ 10 MiB, else 422 `too_large_to_export`); `export` + `view_any` admitted first; 404 when not exportable |
| POST | `/resources/{slug}/exports?{list query}` | 201 `ExportView`; background job (needs storage), ≤ 3 waiting per user (429) |
| GET | `/exports` | `[ExportView]` of the user (not expired, newest first) |
| POST | `/resources/{slug}/imports` (CSV body) | 201 `ImportView`; `import` + `create` (or `update` with an import key) admitted before the body; 422 `file`; ≤ 3 waiting (429) |
| GET | `/imports/{id}` | `ImportView` (owner only, else 404) |
| POST | `/imports/{id}/preview` `{mapping}` | `ImportPreview {rows, valid, keyed, report}`; no write, no hook |
| POST | `/imports/{id}/run` `{mapping}` | `ImportView` (running); once, then 409 `import_started` |
| GET | `/exports/{id}/file` | `text/csv`; owner only (else 404), 409 `export_not_ready` / `export_stale` (credentials, permission, fields or any record no longer visible) |
| GET | `/resources/{slug}/actions/{action}/fields/{field}/options?search=&keys=` | `{data: [SelectOption]}` of a form's `belongs_to` field (keys as text) |
| POST | `/media` (multipart: one `file` part, and an optional `collection` part naming a declared collection, in any order; 422 `collection` for another name) | 201 `{data: MediaItem}` (with `alt`, its alternative text, and `collection`; media library enabled; the media schema's `upload` ability says whether the user may); 429 with `Retry-After` when every upload slot of the process is busy (before the body is read) or beyond 60 uploads a minute per user |
| GET | `/media/{id}/file` | the file (authorized, safe headers) |
| GET | `/resources/{slug}/fields/{field}/options?search=&keys=a,b&record=` | `{data: [{value, label}]}` (≤ 50) |
| … | `/system/…` | users, roles and permissions, see [Users, roles & account](../guide/src/admin/users-roles.md) |
| GET | `/queue/history`, `/queue/jobs/{id}` | finished jobs (`JobRunPage`), one job and its runs (`QueueJobDetail`); `queue.view` (0.8.0) |
| GET / POST | `/schedules`, `/schedules/{name}/runs`, `/schedules/{name}/run` | the Schedules page (`ScheduleList`, `ScheduleRunPage`, 202 `ScheduleRunResponse`); `schedules.view`, Run now also `schedules.manage` (0.8.0) |
| GET | `/system/permission-presets` | `[PermissionPresetInfo]` `{id, label, description?, permissions}`; `roles.view_any` or `permissions.view_any` (ADR 0034). `/system/permissions` items carry `area`, `areaLabel`, `areaSort`, and `label`/`description` when declared |
| GET / PUT | `/settings/{group}` | `SettingsResponse`; PUT `{version, data}` (see below) |
| POST | `/system/users/invitations` | `{name, email, roleIds}` → 201 `SystemUser` (or 204); `users.create` + `users.invite` before the body; 409 `mail_disabled` |
| POST | `/system/users/{id}/invitation` | 200 `SystemUser` (or 204); `users.invite` + `users.view` before the id; 409 `invitation_accepted` / `user_inactive` |
| GET | `/dashboard` | `DashboardView` (widgets the user may see) |
| GET | `/dashboard/widgets/{name}` | `WidgetData` (`stat` / `chart` / `recent` / `too_many`); permission, `view_any` and field reads before any query; 60/min/user, 4 at once per process |
| POST | `/settings/mail/test` | `{to}` → `{ok, code?}` (feature `mail`; `settings.mail.update` before the body; 5/min/user) |
| GET | `/audit?…` | `AuditPage` (`audit.view_all`; auth families with `audit.view_auth`) |
| GET | `/resources/{slug}/records/{key}/history?cursor=&per_page=` | `AuditPage` of the record (`audit.view` + record `view`) |
| GET | `/pages/{slug}` | `CustomPageResponse` (`content`: tagged `html` / `script`); 403 for users with no page, else 404 unknown / 403 not allowed; `data` only when allowed |
| GET | `/pages/{slug}/{*path}` | the same for a sub-page: the raw path is split before decoding and checked after the page's permissions (422 on `path`: malformed or double encoding, `.`/`..`, empty segments, >16 segments or >512 bytes); `Ctx::page_path()` |
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
`form`; vocabulary 6: columns may name a custom cell, `ColumnSchema.cell` + `props`, ADR 0019; vocabulary 7: relations carry `kind`, `attach`, `detach`, `pivot`,
`attachForm`, `pivotForm`; vocabulary 8: encrypted fields are
`secret` and `secret_map` inputs, with masked views and explicit write
operations; vocabulary 9: `belongs_to_many` inputs, ADR 0035; vocabulary
10 (current): row action placement, see below). It contains only what the user may see: inputs for
fields they may write (create), readable fields with `disabled` set when not
writable (edit), and actions they may run. The reference payload is
[`fixtures/contract/posts.schema.json`](https://github.com/irisvn/RustCMS-Core/blob/main/fixtures/contract/posts.schema.json),
checked by the server tests and shared with the admin SPA. Each input carries
`valueType` (the wire kind), `nullable` (an emptied input sends `null`; for text
kinds that are not nullable, `""`) and client `rules` (`required`, `max_length`
in characters); the server re-validates everything. List pages request
`operation=view` without a key. Inputs marked `writeOnly` (writable but not readable, e.g. a
password) carry no current value; omit them from the payload to keep the stored value.
The media library's schema also carries `abilities.upload` (`media.create`:
`POST /media` is allowed); its `create` is always false.

Vocabulary 10: an action may be `primary` (shown with its label in list
rows; the first one a row offers); the other row actions go in the row's
menu, those with `color: "danger"` last after a separator. Whether an action
is offered on a record is the envelope's `abilities[action]`, which also
reflects the action's per-record predicate (`visible_when`); the server
decides it again when the action runs (one record: 403; several: `denied`). Older clients show every
offered action as a button. Columns may carry `hideBelow` (`sm`, `md`,
`lg`): hidden by CSS on narrower screens; older clients show them.
Vocabulary 15 adds count columns: `count: true` shows the number of items of
the field's JSON value (a list's length, an object's keys, `0` for null);
`counts: "<relation>"` shows the envelope's `counts[relation]` (its `name` is
not a field, nothing shown when absent). Both are drawn as a number whatever
their `type`, and never sortable.

Vocabulary 11 adds the editor layout: layouts `split` (children: `pane`s with
`id` `main` and `aside`, either left out when the user sees nothing in it;
`asideFirst` puts the aside first on narrow screens) and `pane`; sections
may carry `collapsed` (foldable, starting folded or not) and `compact`;
`text_input` inputs may carry `prefix` and `suffix` (fixed text, never part of
the value); `FormSchema.width` is `wide` or `full` (absent: the default
width). Older clients show splits and panes as plain groups, sections open,
inputs without their prefix, at the default width.

Vocabulary 13 adds conditions (ADR 0037): inputs and layouts may carry
`shownWhen`, a condition tagged by `op`: `eq` and `ne` (`name`, `value`),
`is_in` (`name`, `values`), `filled` and `is_true` (`name`), `all` and `any`
(`conditions`); inputs may carry `clearWhenHidden: true`. Values are wire
values; integers and decimals compare numerically. Clients evaluate
conditions on every change of the form's values (a layout's condition applies
to its children; a hidden input's value counts as `null` for the others),
do not render what is hidden, keep its draft, never send it, and skip its
client rules; view pages evaluate them on the record's values. The server
evaluates them again on every write, on the resulting values, drops values
sent for hidden inputs and requires required fields only when shown. Older
clients show every input; the server ignores what they send for hidden ones.

Vocabulary 15 adds repeaters (ADR 0040): inputs of type `repeater` carry
`repeater` (`components`: the row's inputs and layouts, each an
`InputSchema`; `minItems`, `maxItems`, `reorderable`, `collapsible`,
`collapsed`, `itemLabel` (`{input}` placeholders), `addActionLabel`,
`addable`, `deletable`, `defaultItems`, `cloneable`). A JSON repeater's value
is an array of objects keyed by the row's inputs; row inputs may carry
`integer: true`. Clients send rows without empty inputs, evaluate a row's
conditions on that row, and show problems in their row. Error paths below a
repeater: `name.index.input` (`faq.2.question`, nested `faq.2.links.0.url`),
`name.index` for a row, `name` for the list. Entries of type
`repeatable_entry` carry `components`: the entries of each row (kind
`json`). New issue code `min_items`. Older clients show repeaters read-only
and never send them.

Relationship repeaters have `valueType: "rows"` and `repeater.relationship`
(`resource`, `relation`, `create`, `update`, `delete`, `orderColumn`); their
row inputs are the child's. Clients read the rows from
`records/{key}/relations/{relation}?per_page=…` (each row's `abilities` say
whether it may be changed or removed) and send a change, never the list:
`{create?: [{ref, data}], update?: [{key, data}], delete?: [keys], order?:
[keys and refs]}` (`ref`: `[a-z0-9-]{1,16}`; at most 100 entries per part;
`order` lists every row the user sees, once). Error paths: `name.key.input`
and `name.ref.input`; `name.key` for a row; `name` for the change. Issue
codes `not_a_row` (a key that is not a visible child of this record) and
`row_order`. The parent's history has `rows_changed` entries whose `meta`
lists `relation`, `created`, `updated`, `deleted` (counts, never child keys)
and `reordered`. Vocabulary 16: in `order`, a stored row is its key and a new
row is tagged `{"new": ref}` (a reference never reads as a key; at most 1,000
rows); edit schemas add `repeater.createComponents`, the inputs of a new row
(the child's create form), `components` being those of a stored row.

Vocabulary 18 adds rich text document version 3 (ADR 0042): marks
`underline`, `superscript` and `subscript` (never both of the last two on one
text), and application nodes (`app.…`, atoms with `attrs` only). A
`rich_editor` input, and a `rich_text_entry`, carry `richNodes` when the
field accepts application nodes: `[{name, label, inline, attrs: [{name, label, kind,
required, default?, min?, max?, options?}]}]` (integers as JSON numbers, an
exception to the wire format: bounded to ±(2^53 − 1)), `kind` one of `text` (`min`,
`max` characters), `integer` (`min`, `max`, within ±(2^53 − 1)), `boolean`,
`one_of` (`options`), `uuid`. Clients write the lowest version a document
needs, keep only the declared nodes with attributes that fit, and show a
document holding anything else read-only (never rewritten unless the user
removes the elements the field no longer accepts); an unknown node
is shown as a placeholder, never as JSON. The server fills in defaults and lowers
`version` to the lowest the parts need before storing, and refuses undeclared nodes (`document_element`),
block elements inside a callout (`document_structure`), a document over 256 KiB
once its defaults are filled in (`max_kib`), version 3 parts in
older documents (`document_needs_version`) and more than 500 nodes
(`max_app_nodes`). Clients of vocabulary 17 and below show version 3 documents
read-only (they read versions 1 and 2 only). Extensions draw and edit nodes
with `registerRichTextNode` (host `ui` 4).

Vocabulary 23 adds media collections. `GET /panel` may carry
`mediaCollections` (`[{value, label}]`, declaration order, labels translated)
for users who may list or upload files. A `file_upload` or `rich_text` input
may carry `collection`: its uploads send it as a `collection` part, and its
picker opens on it. A select filter may carry `none`, the label of a last
choice for records without a value, sent as `~none` in its `in` list (matched
as `IS NULL`; `~none,a` matches either). Older clients never send the part
(files without a collection) and show no such choice.

Vocabulary 22 adds the image viewer's switch: a media entry and an input's
`media` may carry `preview: false` (sent only then): its images do not open
the viewer when clicked. Clients open images in a viewer otherwise (media
entries, file uploads, images of stored documents, the media library), going
through the images of the same place. The media entry's `gallery` (vocabulary
14) is no longer sent: older clients show such entries' images without a link.

Vocabulary 21 adds date formats: a column or an entry may carry `format`, a
pattern (`yyyy yy MMMM MMM MM M dd d EEEE EEE HH H hh h mm ss a z`, text
between single quotes, `''` a quote; at most 64 characters) the server checked
against the field's kind (no time on a date, no `z` on a local date-time).
Clients write the value with it: instants in the viewer's time zone, dates
and local date-times as they are, names in the panel's language. Without one,
the panel's configuration may carry `dateFormat` (date fields) and
`dateTimeFormat` (local date-times, instants, and the panel's own times); else
the client's default style. A client that cannot read a pattern uses its
default. Older clients ignore `format`.

Vocabulary 20 adds option labels to table columns: a text or badge column may
carry `options` (`[{value, label}]`, labels in the user's language, values in
canonical wire form like its `colors`), declared by the column or taken from
the form's select or checkbox list of the field. Clients show the label of the
option holding a cell's value (each item's, for a list; one badge per item in
a badge column) and the value itself when no option holds it; colors still
follow the values. Older clients show the values.

Vocabulary 19 adds form reactions (ADR 0043): inputs of create and edit
forms may carry `reacts: true` (never on view schemas or repeater rows). After such an input
changes and typing pauses (about 400 ms), clients post `form/react` with the
form's values in wire format (as a save sends them, without secrets, links or
relationship rows), the changed input's `previous` value (when the last
answer was applied, or the form opened), `via` on a relation's create form,
and an increasing `seq`, and apply the answer only if its
`seq` is the latest and the form has not changed since. The server evaluates
the reaction on what the user's form holds (ADR 0037: stored values only for
a record the user may view, typed values only for fields they may write). Patches, tagged by
`op`, apply in order (a row index refers to the rows as the previous patches
left them): `set` and `set_if_empty` (`field`, `value`; the latter only when
the field is `null`, `""` or `[]`), `add_row` (`field`, `row`, `index?`: the
end when absent), `remove_row` (`field`, `index`), `move_row` (`field`,
`from`, `to`), `flag_row` (`field`, `index`, `message`: a warning under the
row until the next answer or a change of that repeater), `notice` (`message`). Row patches name JSON
repeaters only. Values are canonical (as a save stores them); at most 200
patches, messages of at most 500 characters. Patches do not trigger further
reactions; network errors, 429 and `failed` are ignored. Saves never run
reactions. Older clients never call them.

Vocabulary 14 adds infolists (ADR 0039): the view schema of a record
(`operation=view&key=…`) carries `infolist`, declared or, for a resource
without one, made from the form; a view schema without a key
(lists, imports, relation sections) has none (`InfolistSchema`:
`columns`, `components`, `width`, and since vocabulary 17 `fromForm: true`
when made from the form), whose components are layouts and
entries: `{type: "text_entry" | "relation_entry" | "boolean_entry" |
"key_value_entry" | "media_entry" | "rich_text_entry" | "computed_entry" |
"custom_entry", name, label, valueType, columnSpan, help?, shownWhen?, …}`
with the options absent when unset (`badge`, `colors`, `copyable`, `limit`,
`url`, `decimals`, `list`, `multiline`, `password`, `prefix`, `suffix`,
`options`, `labels`, `size`, `gallery`, `outline`, `relationship`, `link`,
`computed`, `widget`, `props`; `computed: true`: the value comes from
`records/{key}/computed`, not from the record's data). Entries of fields the user may not read are left out.
Clients draw the infolist (the form stays in the schema: the dashboard reads
its option labels), evaluate its
conditions on the record's values (with `fromForm`, as the edit form reads
them: the values the form would hold, `null` where the form hides a field), read titles of related records from the
envelope's `labels` (`link`: the user may list the related resource), and
load computed values from `records/{key}/computed` (a name in `failed`, or
an error, shows as not available). An entry type whose name ends in `_entry`
but is unknown is drawn as text. Custom page content may be `{kind:
"infolist", infolist, data}`: entries over `data` by name. Clients of
vocabulary 13 and below ignore `infolist` and draw the form read-only. The
extension host's `ui` level is 3: `registerEntry` (v1 `window.rustcms.registerEntry`, v2
`v2.registerEntry`) and the UI kit's `Infolist`.

Vocabulary 12 adds number bounds: `number_input` inputs may carry `number`,
`{min?, max?, step?}` (JSON numbers). The server checks `min` and `max` on
every client write of the field (`min_value`, `max_value` issues); `step` is
a hint for the input's arrows and keyboard. Clients render `i32` and `f64`
values as a native number input with these attributes (and `step` 1 for
`i32` without one); `i64` and decimal values stay text, so they keep their
digits. Older clients show the input without its bounds.

Vocabulary 9 adds `belongs_to_many` inputs (ADR 0035): `name` is a
belongs-to-many relation, `relationship.resource` its target, `valueType`
`links`, and `links: {attach, detach}` what the user may change. Records
carry no value for it: edit forms read the current links from the relation's
list. The payload sends a change, `{attach?: [keys], detach?: [keys]}`, and
only when it changes something; create forms read options from
`GET /resources/{slug}/relations/{relation}/options`, edit forms from the
record's link options. Older clients render it read-only and never send it.

Settings pages may hold `file_upload` inputs (`SettingField::media`,
`media_list`) with the same `media` rules as resource inputs (`valueType`
`uuid` for one file, `json` for a list); see
[`fixtures/contract/settings.brand.json`](https://github.com/irisvn/RustCMS-Core/blob/main/fixtures/contract/settings.brand.json).

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
- `abilities` also holds one entry per action (`"publish": true`), decided by the policy and the action's `visible_when` on that record.
- `labels` holds titles of related records. A related record the user may not
  see is shown as `#<key>`.
- `counts` (the list route `GET resources/{slug}/records` only, not relation
  lists; vocabulary 15) holds, per relation a count column
  names, how many of its records the user may list (scope, policy, trash).
  Absent for a user who may not list the relation, and on a page whose
  related records are too many to check one by one.
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
carries a stable `code` (`write_conflict`, or one named by its endpoint:
`import_started`, `export_not_ready`, `mail_disabled`, …) and a `reason`;
`write_conflict` reasons include `busy`, `upload_expired`,
`receipt_expired`.

| Status | `code`              |
|--------|---------------------|
| 401    | `unauthenticated`   |
| 403    | `forbidden`         |
| 403    | `csrf_mismatch` (missing or stale `X-CSRF-Token`: fetch `/auth/csrf` again; do not replay blindly) |
| 404    | `not_found` (also for records outside the user's scope; a user without the ability gets 403 for any key) |
| 405    | `method_not_allowed` (e.g. creating through a belongs-to-many relation) |
| 409    | `write_conflict` or an endpoint's own code, with `reason` |
| 413    | `payload_too_large` |
| 422    | `validation_failed` |
| 429    | `too_many_requests`, with `Retry-After` (seconds) |
| 500    | `internal_error` (details are logged, never returned) |

## Idempotency keys

`POST …/records` (create), `POST …/relations/{relation}` (has-many create),
`POST …/actions/{action}`, `POST /media`, `POST …/exports` and
`POST …/imports` accept an `Idempotency-Key` header (1–128 of `A-Z a-z 0-9 _ -`, otherwise 422 on
`idempotency_key`). The key is claimed in the write's transaction, so the write
and its receipt commit together or not at all. Repeating the request with the
same key, by the same user, within 24 hours:

- same request (body compared canonically: key order does not matter) → the
  first result, not applied again: a create answers its record as the user may
  see it **now** (key only when no longer visible); an action answers the first
  run's outcome with `"replayed": true`. Admission is checked again (403 if the
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
