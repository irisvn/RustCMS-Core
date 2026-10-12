// Generated from the Rust schema DTOs by crates/rustcms-admin/tests/typescript.rs. Do not edit.

export type JsonValue = number | string | boolean | Array<JsonValue> | { [key in string]: JsonValue } | null;

export type InputType = "text_input" | "textarea" | "number_input" | "select" | "toggle" | "checkbox" | "checkbox_list" | "date_picker" | "date_time_picker" | "hidden";

export type LayoutType = "section" | "grid";

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
titleField?: string, };

export type SchemaOperation = "create" | "edit" | "view";

export type Rule = { "rule": "required" } | { "rule": "max_length", 
/**
 * Limit.
 */
value: number, };

export type Relationship = { 
/**
 * Slug of the related resource (options come from the field's options endpoint).
 */
resource: string, };

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
relationship?: Relationship, };

export type LayoutSchema = { 
/**
 * Layout type.
 */
type: LayoutType, 
/**
 * Title (sections).
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
relationship?: Relationship, };

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
icon?: string, };

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
actions: Array<ActionSchema>, };

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
table: TableSchema, };

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
missing: Array<string>, };

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
 * May create roles (`roles.create`).
 */
createRoles: boolean, };

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
 * The current user holds it, so may grant or remove it.
 */
grantable: boolean, };

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
 * Resources the user may list (`view_any`), in registration order.
 */
navigation: Array<ResourceInfo>, 
/**
 * System pages the user may open.
 */
system: SystemLinks, };

export type DataResponse<T> = { 
/**
 * Payload.
 */
data: T, };
