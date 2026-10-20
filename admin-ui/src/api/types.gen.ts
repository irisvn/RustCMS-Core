// Generated from the Rust schema DTOs by crates/rustcms-admin/tests/typescript.rs. Do not edit.

export type JsonValue = number | string | boolean | Array<JsonValue> | { [key in string]: JsonValue } | null;

export type InputType = "text_input" | "textarea" | "number_input" | "select" | "toggle" | "checkbox" | "checkbox_list" | "date_picker" | "date_time_picker" | "hidden" | "key_value" | "file_upload" | "rich_editor" | "custom" | "secret" | "secret_map";

export type LayoutType = "section" | "grid" | "tabs" | "tab";

export type ColumnType = "text" | "badge" | "boolean" | "date";

export type FilterType = "select" | "ternary" | "date_range";

export type SelectOption = { 
/**
 * Value in wire format.
 */
value: JsonValue, 
/**
 * Displayed label.
 */
label: string, };

export type ResourceInfo = { 
/**
 * URL slug.
 */
slug: string, 
/**
 * Singular label.
 */
label: string, 
/**
 * Plural label.
 */
pluralLabel: string, 
/**
 * Icon name (client icon set).
 */
icon?: string, 
/**
 * Navigation group.
 */
group?: string, 
/**
 * Field used as record title.
 */
titleField?: string, 
/**
 * Navigation position (`ResourceMeta::sort`), absent when 0.
 */
sort?: number, 
/**
 * `false` for a resource with no navigation entry
 * (`ResourceMeta::navigation(false)`), absent otherwise.
 */
nav?: boolean, };

export type SchemaOperation = "create" | "edit" | "view";

export type Rule = { "rule": "required" } | { "rule": "max_length", 
/**
 * Limit.
 */
value: number, } | { "rule": "max_items", 
/**
 * Limit.
 */
value: number, };

export type Relationship = { 
/**
 * Slug of the related resource (options come from the field's options endpoint).
 */
resource: string, };

export type MediaSchema = { 
/**
 * Several media, in order (a list of ids).
 */
multiple: boolean, 
/**
 * Most items.
 */
maxItems: number, 
/**
 * Accepted MIME types (`image/*`); empty accepts every uploadable type.
 */
accept: Array<string>, 
/**
 * Largest accepted file, in bytes.
 */
maxSize?: number, };

export type RelationKind = "hasMany" | "belongsToMany";

export type RelationSchema = { 
/**
 * URL segment.
 */
name: string, 
/**
 * Has-many or belongs-to-many.
 */
kind: RelationKind, 
/**
 * Section title.
 */
label: string, 
/**
 * Slug of the related resource.
 */
resource: string, 
/**
 * Has-many: field of the related resource holding this record's key (set
 * by the server). Belongs-to-many: the pivot column (informational).
 */
field: string, 
/**
 * Whether the user may create related records from here (has-many).
 */
create: boolean, 
/**
 * Whether the user may attach targets to this record (belongs-to-many).
 */
attach?: boolean, 
/**
 * Whether the user may detach targets from this record.
 */
detach?: boolean, 
/**
 * Pivot columns the user may read, shown with each link.
 */
pivot?: Array<InputSchema>, 
/**
 * Pivot columns set when attaching (with `attach`).
 */
attachForm?: FormSchema, 
/**
 * Pivot columns the user may change on a link.
 */
pivotForm?: FormSchema, };

export type MediaItem = { 
/**
 * Id (UUID).
 */
id: string, 
/**
 * Original file name.
 */
name: string, 
/**
 * MIME type.
 */
mime: string, 
/**
 * Size in bytes.
 */
size: number, 
/**
 * Image width.
 */
width?: number, 
/**
 * Image height.
 */
height?: number, 
/**
 * Shown inline (an image) rather than downloaded.
 */
inline: boolean, };

export type InputSchema = { 
/**
 * Input type.
 */
type: InputType, 
/**
 * Field name.
 */
name: string, 
/**
 * Label.
 */
label: string, 
/**
 * Wire kind of the value (`string`, `i64`, `decimal`, `instant`, …).
 */
valueType: string, 
/**
 * The column accepts `null` (an emptied input sends `null`; otherwise an
 * empty string for text kinds).
 */
nullable: boolean, 
/**
 * Create forms: the column's declared default (entity `default_value`), in
 * wire format. Inputs without one start empty, and untouched ones are left
 * out of the payload so the database default applies.
 */
default?: JsonValue, 
/**
 * Shown but not editable.
 */
disabled: boolean, 
/**
 * Writable but not readable (e.g. a password): no current value is sent;
 * leave it out of the payload to keep the stored value.
 */
writeOnly?: boolean, 
/**
 * Client-side rules.
 */
rules: Array<Rule>, 
/**
 * Grid span.
 */
columnSpan: number, 
/**
 * Placeholder.
 */
placeholder?: string, 
/**
 * Help text.
 */
help?: string, 
/**
 * Input mode (`email`, `password`, `url`).
 */
mode?: string, 
/**
 * Static options.
 */
options?: Array<SelectOption>, 
/**
 * Related resource for `belongs_to` fields.
 */
relationship?: Relationship, 
/**
 * Widget of a `custom` input (`vendor/name`).
 */
widget?: string, 
/**
 * Settings of a `custom` input's widget.
 */
props?: JsonValue, 
/**
 * Media references (`file_upload` inputs).
 */
media?: MediaSchema, };

export type LayoutSchema = { 
/**
 * Layout type.
 */
type: LayoutType, 
/**
 * Stable identity (tabs).
 */
id?: string, 
/**
 * Title (sections, tabs).
 */
label?: string, 
/**
 * Columns.
 */
columns: number, 
/**
 * Children.
 */
components: Array<ComponentSchema>, };

export type ComponentSchema = InputSchema | LayoutSchema;

export type FormSchema = { 
/**
 * Columns.
 */
columns: number, 
/**
 * Components the user may see for the operation.
 */
components: Array<ComponentSchema>, };

export type ColumnSchema = { 
/**
 * Column type.
 */
type: ColumnType, 
/**
 * Field name.
 */
name: string, 
/**
 * Header.
 */
label: string, 
/**
 * Wire kind of the value.
 */
valueType: string, 
/**
 * May be used in `sort`.
 */
sortable: boolean, 
/**
 * Included in `search`.
 */
searchable: boolean, 
/**
 * Badge colors.
 */
colors?: Array<BadgeColor>, 
/**
 * Related resource (display `labels[name]`).
 */
relationship?: Relationship, 
/**
 * Custom cell (`vendor/name`) drawing the value, if registered.
 */
cell?: string, 
/**
 * Settings of the custom cell.
 */
props?: JsonValue, };

export type BadgeColor = { 
/**
 * Value.
 */
value: JsonValue, 
/**
 * Color name.
 */
color: string, };

export type FilterSchema = { 
/**
 * Filter type.
 */
type: FilterType, 
/**
 * Field name.
 */
name: string, 
/**
 * Label.
 */
label: string, 
/**
 * Wire kind of the value.
 */
valueType: string, 
/**
 * Options (select filters).
 */
options?: Array<SelectOption>, 
/**
 * Related resource (options endpoint).
 */
relationship?: Relationship, };

export type ActionSchema = { 
/**
 * Name (URL segment).
 */
name: string, 
/**
 * Label.
 */
label: string, 
/**
 * Offered for selections of records.
 */
bulk: boolean, 
/**
 * Confirmation question, if any.
 */
confirm?: string, 
/**
 * Color (`primary`, `danger`, …).
 */
color?: string, 
/**
 * Icon name.
 */
icon?: string, 
/**
 * Values asked for before running (in a dialog).
 */
form?: FormSchema, };

export type TableSchema = { 
/**
 * Columns.
 */
columns: Array<ColumnSchema>, 
/**
 * Filters.
 */
filters: Array<FilterSchema>, 
/**
 * Default sort (`-field` for descending).
 */
defaultSort?: string, 
/**
 * Allowed page sizes.
 */
perPageOptions: Array<number>, 
/**
 * Whether `search` matches anything.
 */
searchable: boolean, 
/**
 * Actions.
 */
actions: Array<ActionSchema>, 
/**
 * Deleting moves records to the trash (`trashed` list parameter, restore, force delete).
 */
softDeletes?: boolean, };

export type ResourceSchema = { 
/**
 * [`SCHEMA_VERSION`].
 */
schemaVersion: number, 
/**
 * [`VOCABULARY_VERSION`].
 */
vocabularyVersion: number, 
/**
 * Identity.
 */
resource: ResourceInfo, 
/**
 * Operation the form describes.
 */
operation: SchemaOperation, 
/**
 * `viewAny`, `create`, and for a given record `view`, `update`, `delete`.
 */
abilities: { [key in string]: boolean }, 
/**
 * Form.
 */
form: FormSchema, 
/**
 * Table.
 */
table: TableSchema, 
/**
 * Has-many relations of the viewed record the user may list.
 */
relations?: Array<RelationSchema>, };

export type Envelope = { 
/**
 * Encoded primary key.
 */
key: string, 
/**
 * Record title, when the title field is readable.
 */
title?: string, 
/**
 * Record-level abilities computed by the policy.
 */
abilities: { [key in string]: boolean }, 
/**
 * Readable fields only.
 */
data: { [key in string]: JsonValue }, 
/**
 * The record is in the trash (soft deletes); then only `view`, `restore`
 * and `force_delete` apply.
 */
trashed?: boolean, 
/**
 * Titles of related records for `belongs_to` fields (`#key` when not visible).
 */
labels?: { [key in string]: string }, };

export type PageMeta = { 
/**
 * Current page (1-based).
 */
page: number, 
/**
 * Page size.
 */
perPage: number, 
/**
 * Total records in scope matching the query.
 */
total: number, 
/**
 * Last page number.
 */
lastPage: number, };

export type Page = { 
/**
 * Records.
 */
data: Array<Envelope>, 
/**
 * Pagination.
 */
meta: PageMeta, };

export type ActionOutcome = { 
/**
 * Keys the handler processed.
 */
processed: Array<string>, 
/**
 * Keys the policy denied.
 */
denied: Array<string>, 
/**
 * Keys not found in the user's scope.
 */
missing: Array<string>, 
/**
 * This is the outcome of an earlier request with the same `Idempotency-Key`:
 * the action did not run again.
 */
replayed?: boolean, };

export type LinkOutcome = { 
/**
 * Targets linked (attach: already or now), unlinked (detach) or updated.
 */
processed: Array<string>, 
/**
 * Targets not found, or not visible to the user: never touched.
 */
missing: Array<string>, };

export type LinkOption = { 
/**
 * Target key.
 */
value: string, 
/**
 * Target title.
 */
label: string, 
/**
 * Already linked to the record.
 */
attached: boolean, };

export type LinkPage = { 
/**
 * Pivot columns by target key (belongs-to-many).
 */
pivot?: { [key in string]: { [key in string]: JsonValue } }, 
/**
 * Records.
 */
data: Array<Envelope>, 
/**
 * Pagination.
 */
meta: PageMeta, };

export type ExportView = { 
/**
 * Id (UUID).
 */
id: string, 
/**
 * Resource slug.
 */
resource: string, 
/**
 * `pending`, `running`, `done` or `failed`.
 */
status: string, 
/**
 * Rows written (when done).
 */
rows: number, 
/**
 * Why it failed: `revoked`, `account_changed`, `fields_changed`,
 * `too_large`, `unfinished` or `failed`.
 */
error?: string, 
/**
 * RFC 3339.
 */
createdAt: string, 
/**
 * When the file is deleted (RFC 3339).
 */
expiresAt: string, };

export type ImportField = { 
/**
 * Field name (also what a file's column is matched with).
 */
name: string, 
/**
 * Label in the user's language.
 */
label: string, 
/**
 * Required when creating.
 */
required: boolean, 
/**
 * The import key (finds the record to update).
 */
key: boolean, };

export type RowIssue = { 
/**
 * Line in the file (the header is line 1).
 */
row: number, 
/**
 * Field (absent: the whole row).
 */
field?: string, 
/**
 * Message (English source).
 */
message: string, 
/**
 * Its code and placeholder values, for the panel to translate.
 */
code: string, 
/**
 * Placeholder values of the code.
 */
params?: { [key in string]: string }, };

export type ImportView = { 
/**
 * Id (UUID).
 */
id: string, 
/**
 * Resource slug.
 */
resource: string, 
/**
 * `uploaded`, `running`, `done` or `failed`.
 */
status: string, 
/**
 * Column names of the file.
 */
header: Array<string>, 
/**
 * Rows of the file.
 */
rows: number, 
/**
 * Fields the user may set, with the import key marked.
 */
fields: Array<ImportField>, 
/**
 * Column → field suggested (by name or label) or fixed (once it runs).
 */
mapping: { [key in string]: string }, 
/**
 * Records created.
 */
created: number, 
/**
 * Records updated.
 */
updated: number, 
/**
 * Rows that failed.
 */
failed: number, 
/**
 * Problems of the rows that failed (at most 1,000).
 */
report: Array<RowIssue>, 
/**
 * Why it stopped: `revoked`, `account_changed`, `unfinished` or `failed`.
 */
error?: string, };

export type ImportPreview = { 
/**
 * Rows checked.
 */
rows: number, 
/**
 * Rows with no problem.
 */
valid: number, 
/**
 * Rows with an import key: created or updated, decided when it runs.
 */
keyed: number, 
/**
 * Problems found (at most 1,000).
 */
report: Array<RowIssue>, };

export type PanelUser = { 
/**
 * User id (string).
 */
id: string, 
/**
 * Display name.
 */
name: string, 
/**
 * Email.
 */
email: string, };

export type SystemLinks = { 
/**
 * Users page (`users.view_any`).
 */
users: boolean, 
/**
 * Roles page (`roles.view_any`).
 */
roles: boolean, 
/**
 * May create users (`users.create`).
 */
createUsers: boolean, 
/**
 * May invite users (`users.create` and `users.invite`).
 */
inviteUsers: boolean, 
/**
 * May create roles (`roles.create`).
 */
createRoles: boolean, 
/**
 * The audit log (`audit.view_all`).
 */
audit: boolean, 
/**
 * The Queue page (`queue.view`).
 */
queue: boolean, };

export type RoleRef = { 
/**
 * Role id.
 */
id: string, 
/**
 * Role name.
 */
name: string, };

export type SystemUser = { 
/**
 * User id.
 */
id: string, 
/**
 * Display name.
 */
name: string, 
/**
 * Sign-in email.
 */
email: string, 
/**
 * May sign in.
 */
isActive: boolean, 
/**
 * Invited, the invitation not yet accepted.
 */
invitationPending: boolean, 
/**
 * When they were invited (RFC 3339).
 */
invitedAt?: string, 
/**
 * Roles held in the admin guard.
 */
roles: Array<RoleRef>, 
/**
 * Holds the admin `super_admin` role.
 */
superAdmin: boolean, 
/**
 * The signed-in user themselves (managed on `/account`).
 */
isSelf: boolean, 
/**
 * What the current user may do with this user (the server re-checks).
 */
abilities: { [key in string]: boolean }, };

export type UserPage = { 
/**
 * Users.
 */
data: Array<SystemUser>, 
/**
 * Pagination.
 */
meta: PageMeta, };

export type SystemRole = { 
/**
 * Role id.
 */
id: string, 
/**
 * Role name.
 */
name: string, 
/**
 * Created by migrations (immutable, never deleted).
 */
isSystem: boolean, 
/**
 * Managed by super admins only.
 */
isPrivileged: boolean, 
/**
 * Permission names.
 */
permissions: Array<string>, 
/**
 * Users holding it.
 */
users: number, 
/**
 * What the current user may do with this role (the server re-checks).
 */
abilities: { [key in string]: boolean }, };

export type PermissionInfo = { 
/**
 * Name (`posts.update`).
 */
name: string, 
/**
 * Group (usually the resource slug).
 */
group?: string, 
/**
 * The group as shown: a resource's plural label, else the group, in the
 * request's language. The group itself stays the identifier.
 */
groupLabel?: string, 
/**
 * The current user holds it, so may grant or remove it.
 */
grantable: boolean, 
/**
 * The resource whose ability it is (`posts`), when it is exactly one
 * resource's `{slug}.{ability}`: a cell of the permission matrix.
 */
resource?: string, 
/**
 * That ability (`update`, `attach_tags`, an action's).
 */
ability?: string, 
/**
 * Label (declared with `permission_meta`), translated; else show `name`.
 */
label?: string, 
/**
 * What it allows, translated.
 */
description?: string, 
/**
 * Section of the Roles page (`resources`, `system`, `other` or declared).
 */
area: string, 
/**
 * The area as shown, translated.
 */
areaLabel: string, 
/**
 * The area's position (lower first).
 */
areaSort: number, };

export type PermissionPresetInfo = { 
/**
 * Id (`viewer`).
 */
id: string, 
/**
 * Label, translated.
 */
label: string, 
/**
 * Description, translated.
 */
description?: string, 
/**
 * Admin permissions it selects (declared, not orphaned; also those the
 * current user may not grant, which the page leaves unchanged).
 */
permissions: Array<string>, };

export type SettingsLink = { 
/**
 * The group (URL segment).
 */
group: string, 
/**
 * Title.
 */
label: string, 
/**
 * Icon name.
 */
icon?: string, };

export type SecretStatus = "set" | "unset" | "unreadable";

export type SettingsAbilities = { 
/**
 * Save changes.
 */
update: boolean, 
/**
 * Send a test mail with the saved settings (the Mail page).
 */
test: boolean, };

export type SettingsResponse = { 
/**
 * See [`SCHEMA_VERSION`].
 */
schemaVersion: number, 
/**
 * See [`VOCABULARY_VERSION`].
 */
vocabularyVersion: number, 
/**
 * The group.
 */
group: string, 
/**
 * Title.
 */
label: string, 
/**
 * The form (inputs disabled without `update`).
 */
form: FormSchema, 
/**
 * Values of the settings other than secrets (defaults for unsaved ones).
 */
data: Record<string, JsonValue>, 
/**
 * State of each secret; their values are never sent.
 */
secrets: { [key in string]: SecretStatus }, 
/**
 * Version to send back when saving (`"0"`: never saved).
 */
version: string, 
/**
 * What the user may do.
 */
abilities: SettingsAbilities, };

export type MailTestResult = { 
/**
 * Sent.
 */
ok: boolean, 
/**
 * Why not (`smtp_auth`, `mail_not_configured`, …).
 */
code?: string, };

export type Bucket = "day" | "week" | "month";

export type WidgetInfo = { 
/**
 * Name (`GET dashboard/widgets/{name}`).
 */
name: string, 
/**
 * `stat`, `chart` or `recent`.
 */
kind: string, 
/**
 * Title in the user's language.
 */
label: string, 
/**
 * Grid columns taken (1 to 4).
 */
span: number, 
/**
 * The resource it counts or lists.
 */
resource: string, 
/**
 * Charts: their buckets.
 */
bucket?: Bucket, };

export type DashboardView = { 
/**
 * The widgets the user may see, in order.
 */
widgets: Array<WidgetInfo>, 
/**
 * Show the resource cards.
 */
resourceLinks: boolean, };

export type ChartBucket = { 
/**
 * First day (`YYYY-MM-DD`, UTC).
 */
start: string, 
/**
 * Records (an integer string, ADR 0003).
 */
count: string, };

export type RecentColumn = { 
/**
 * Field.
 */
name: string, 
/**
 * Label in the user's language.
 */
label: string, };

export type RecentRecord = { 
/**
 * Key (`/r/{resource}/records/{key}`).
 */
key: string, 
/**
 * Title.
 */
title?: string, 
/**
 * Text of each column (readable fields only; related records by title).
 */
values: { [key in string]: string }, };

export type WidgetData = { "kind": "stat", 
/**
 * Records (an integer string).
 */
value: string, 
/**
 * The period before (compared stats).
 */
previous?: string, } | { "kind": "chart", 
/**
 * Buckets.
 */
buckets: Array<ChartBucket>, } | { "kind": "recent", 
/**
 * Columns shown.
 */
columns: Array<RecentColumn>, 
/**
 * Records.
 */
records: Array<RecentRecord>, } | { "kind": "too_many" };

export type AuditActorView = { 
/**
 * `user`, `system` or `guest`.
 */
kind: string, 
/**
 * User id (string).
 */
id?: string, 
/**
 * Name, for viewers who may list users.
 */
name?: string, 
/**
 * How they were signed in: `session`, `token`, `jwt` or `console`.
 */
credential?: string, };

export type AuditEntryView = { 
/**
 * Entry id.
 */
id: string, 
/**
 * When (RFC 3339).
 */
at: string, 
/**
 * What happened.
 */
event: string, 
/**
 * What it is about (resource slug, `user`, `role`, `settings`, …).
 */
subjectType: string, 
/**
 * Which one.
 */
subjectKey?: string, 
/**
 * Who.
 */
actor: AuditActorView, 
/**
 * Client address (`audit.view_auth`).
 */
ip?: string, 
/**
 * Client user agent (`audit.view_auth`).
 */
userAgent?: string, 
/**
 * Request id.
 */
requestId?: string, 
/**
 * `{field: [old, new]}`.
 */
changes?: Record<string, JsonValue>, 
/**
 * Facts of the event.
 */
meta?: Record<string, JsonValue>, };

export type AuditPageMeta = { 
/**
 * Pass as `cursor` for older entries; absent on the last page.
 */
nextCursor?: string, };

export type AuditPage = { 
/**
 * Entries.
 */
data: Array<AuditEntryView>, 
/**
 * Continuation.
 */
meta: AuditPageMeta, };

export type QueueStats = { 
/**
 * Queue name.
 */
name: string, 
/**
 * Run as soon as a worker is free.
 */
pending: number, 
/**
 * Wait for their time (a backoff, a delayed dispatch).
 */
scheduled: number, 
/**
 * Owned by a worker.
 */
running: number, 
/**
 * Last attempt's lease ended without an answer: failed at the next worker pass.
 */
stalled: number, 
/**
 * Will not run again unless retried.
 */
failed: number, };

export type QueueWorkers = { 
/**
 * `queue.enabled`: `serve` runs workers.
 */
enabled: boolean, 
/**
 * Workers per process.
 */
workers: number, 
/**
 * Queues they take jobs from, in order.
 */
queues: Array<string>, };

export type QueueOverview = { 
/**
 * Queues with jobs, by name.
 */
queues: Array<QueueStats>, 
/**
 * This instance's worker settings.
 */
workers: QueueWorkers, 
/**
 * May retry, forget and cancel (`queue.manage`).
 */
canManage: boolean, };

export type QueueUser = { 
/**
 * User id (string).
 */
id: string, 
/**
 * Name, for viewers who may list users.
 */
name?: string, };

export type QueueJob = { 
/**
 * Job id (UUID).
 */
id: string, 
/**
 * Queue name.
 */
queue: string, 
/**
 * `Job::KIND`.
 */
kind: string, 
/**
 * `Job::VERSION` of its payload.
 */
version: number, 
/**
 * `pending`, `scheduled`, `running`, `stalled` or `failed`.
 */
status: string, 
/**
 * Runs so far.
 */
attempts: number, 
/**
 * Most runs.
 */
maxAttempts: number, 
/**
 * Not run before (RFC 3339).
 */
availableAt: string, 
/**
 * Its worker owns it until (running jobs).
 */
reservedUntil?: string, 
/**
 * Why the last run failed (a safe message).
 */
lastError?: string, 
/**
 * Dispatched at (RFC 3339).
 */
createdAt: string, 
/**
 * Failed at (RFC 3339).
 */
failedAt?: string, 
/**
 * The user who dispatched it.
 */
initiatedBy?: QueueUser, };

export type QueueJobPageMeta = { 
/**
 * Pass as `cursor` for older jobs; absent on the last page.
 */
nextCursor?: string, };

export type QueueJobPage = { 
/**
 * Jobs.
 */
data: Array<QueueJob>, 
/**
 * Continuation.
 */
meta: QueueJobPageMeta, };

export type PageLink = { 
/**
 * URL segment (`/p/{slug}`).
 */
slug: string, 
/**
 * Title.
 */
label: string, 
/**
 * Icon name.
 */
icon?: string, 
/**
 * Navigation group.
 */
group?: string, 
/**
 * Navigation position (`PanelPageMeta::sort`), absent when 0.
 */
sort?: number, };

export type CustomContent = { "kind": "html", 
/**
 * The markup.
 */
html: string, } | { "kind": "script", 
/**
 * `vendor/name`.
 */
script: string, 
/**
 * What the script gets as `ctx.data`.
 */
data: JsonValue, };

export type CustomPageResponse = { 
/**
 * URL segment.
 */
slug: string, 
/**
 * Title (translated).
 */
title: string, 
/**
 * Content.
 */
content: CustomContent, };

export type SlotContent = { 
/**
 * Slot name.
 */
name: string, 
/**
 * Heading (translated).
 */
label?: string, 
/**
 * Content.
 */
content: CustomContent, };

export type SlotsResponse = { 
/**
 * The slots the user may see, in registration order.
 */
data: Array<SlotContent>, };

export type PanelResponse = { 
/**
 * Name shown in the panel.
 */
brand: string, 
/**
 * Signed-in user.
 */
user: PanelUser, 
/**
 * Resources the user may list (`view_any`), by slug (hidden ones too:
 * `nav: false`).
 */
navigation: Array<ResourceInfo>, 
/**
 * System pages the user may open.
 */
system: SystemLinks, 
/**
 * Settings pages the user may open.
 */
settings: Array<SettingsLink>, 
/**
 * Custom pages the user may open, in registration order.
 */
pages: Array<PageLink>, 
/**
 * The custom page `/admin` opens (`AdminProvider::home`), only when the
 * user may open it.
 */
home?: string, 
/**
 * The declared admin-guard permissions the user holds, sorted (every
 * declared one for a super admin): for showing or hiding controls in
 * extensions (`can`); the server checks every request.
 */
permissions: Array<string>, };

export type DataResponse<T> = { 
/**
 * Payload.
 */
data: T, };
