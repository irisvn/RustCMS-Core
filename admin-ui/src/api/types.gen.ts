// Generated from the Rust schema DTOs by crates/rustcms-admin/tests/typescript.rs. Do not edit.

export type JsonValue = number | string | boolean | Array<JsonValue> | { [key in string]: JsonValue } | null;

export type InputType = "text_input" | "textarea" | "number_input" | "select" | "toggle" | "checkbox" | "checkbox_list" | "date_picker" | "date_time_picker" | "hidden" | "key_value" | "file_upload" | "rich_editor" | "custom" | "secret" | "secret_map" | "belongs_to_many" | "repeater";

export type LayoutType = "section" | "grid" | "tabs" | "tab" | "split" | "pane";

export type EntryType = "text_entry" | "relation_entry" | "boolean_entry" | "key_value_entry" | "media_entry" | "rich_text_entry" | "computed_entry" | "custom_entry" | "repeatable_entry";

export type ColumnType = "text" | "badge" | "boolean" | "date";

export type Breakpoint = "sm" | "md" | "lg";

export type FormWidth = "default" | "wide" | "full";

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

export type NumberBounds = { 
/**
 * Smallest value a client may write.
 */
min?: number, 
/**
 * Largest value a client may write.
 */
max?: number, 
/**
 * Increment of the input's arrows and keyboard (a hint, not checked).
 */
step?: number, };

export type Condition = { "op": "eq", 
/**
 * Input name.
 */
name: string, 
/**
 * Compared value.
 */
value: JsonValue, } | { "op": "ne", 
/**
 * Input name.
 */
name: string, 
/**
 * Compared value.
 */
value: JsonValue, } | { "op": "is_in", 
/**
 * Input name.
 */
name: string, 
/**
 * Allowed values.
 */
values: Array<JsonValue>, } | { "op": "filled", 
/**
 * Input name.
 */
name: string, } | { "op": "is_true", 
/**
 * Input name.
 */
name: string, } | { "op": "all", 
/**
 * Combined conditions.
 */
conditions: Array<Condition>, } | { "op": "any", 
/**
 * Combined conditions.
 */
conditions: Array<Condition>, };

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
 * Field used as record title (a title of one field; see `title_fields`).
 */
titleField?: string, 
/**
 * Fields of a record title of several fields, in template order (ADR 0038).
 */
titleFields?: Array<string>, 
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
maxSize?: number, 
/**
 * Images open in the panel's viewer when clicked; sent only when `false`
 * (vocabulary 22).
 */
preview?: boolean, };

export type LinksSchema = { 
/**
 * Link more records (`attach_{relation}`).
 */
attach: boolean, 
/**
 * Unlink linked records (`detach_{relation}`; edit forms only).
 */
detach: boolean, };

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
inline: boolean, 
/**
 * Alternative text, when given (edited on the media record).
 */
alt?: string, 
/**
 * Collection, when it is in one.
 */
collection?: string, };

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
media?: MediaSchema, 
/**
 * Media collection of the files uploaded through the input, and the
 * collection its picker opens on (`file_upload` and `rich_text` inputs,
 * vocabulary 23).
 */
collection?: string, 
/**
 * What the user may change (`belongs_to_many` inputs, vocabulary 9).
 */
links?: LinksSchema, 
/**
 * Fixed text shown inside the input before the value (text inputs, vocabulary 11).
 */
prefix?: string, 
/**
 * Fixed text shown inside the input after the value (text inputs, vocabulary 11).
 */
suffix?: string, 
/**
 * Bounds and step of a number input (vocabulary 12); `min` and `max`
 * are checked again by the server on every client write.
 */
number?: NumberBounds, 
/**
 * Shown only when this holds (vocabulary 13, ADR 0037): evaluated on
 * every change; the server drops values sent for hidden inputs.
 */
shownWhen?: Condition, 
/**
 * Hidden after a write, the field is cleared by the server (vocabulary 13).
 */
clearWhenHidden?: boolean, 
/**
 * JSON repeater rows: a whole number (vocabulary 15).
 */
integer?: boolean, 
/**
 * Repeaters: their rows (vocabulary 15, ADR 0040).
 */
repeater?: RepeaterSchema, 
/**
 * A change asks the server's reaction (`POST form/react`) for changes to
 * the form (vocabulary 19, ADR 0043). Create and edit forms only.
 */
reacts?: boolean, 
/**
 * Rich text: the application nodes the document may hold (vocabulary
 * 18, ADR 0042); absent when none.
 */
richNodes?: Array<RichNodeSchema>, };

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
components: Array<ComponentSchema>, 
/**
 * Sections the user may fold: whether it starts folded (vocabulary 11).
 */
collapsed?: boolean, 
/**
 * A tighter section (vocabulary 11).
 */
compact?: boolean, 
/**
 * Splits: the aside first on narrow screens (vocabulary 11).
 */
asideFirst?: boolean, 
/**
 * Shown, with everything in it, only when this holds (vocabulary 13).
 */
shownWhen?: Condition, };

export type EntrySchema = { 
/**
 * Entry type.
 */
type: EntryType, 
/**
 * Field (or computed value, or key of a panel page's data).
 */
name: string, 
/**
 * Label (translated).
 */
label: string, 
/**
 * Wire kind of the value (`json` for computed values and panel pages).
 */
valueType: string, 
/**
 * Columns spanned.
 */
columnSpan: number, 
/**
 * Help text (translated).
 */
help?: string, 
/**
 * Shown only when this holds for the stored values.
 */
shownWhen?: Condition, 
/**
 * Shown as a badge.
 */
badge?: boolean, 
/**
 * Colors by value (badges, booleans).
 */
colors?: Array<BadgeColor>, 
/**
 * A button copies the value.
 */
copyable?: boolean, 
/**
 * Most characters shown.
 */
limit?: number, 
/**
 * The value is a link (`http`, `https` only).
 */
url?: boolean, 
/**
 * A number with this many decimals.
 */
decimals?: number, 
/**
 * One item per line.
 */
list?: boolean, 
/**
 * Line breaks kept.
 */
multiline?: boolean, 
/**
 * Masked.
 */
password?: boolean, 
/**
 * Fixed text before the value.
 */
prefix?: string, 
/**
 * Fixed text after the value.
 */
suffix?: string, 
/**
 * Labels by value (translated).
 */
options?: Array<SelectOption>, 
/**
 * Labels of a boolean entry, `[yes, no]` (translated).
 */
labels?: [string, string], 
/**
 * Thumbnail size in pixels.
 */
size?: number, 
/**
 * Media entries: images open in the panel's viewer when clicked; sent
 * only when `false` (vocabulary 22, which dropped `gallery`).
 */
preview?: boolean, 
/**
 * How dates or times are shown (a pattern, as `ColumnSchema::format`;
 * vocabulary 21). Absent: the panel's.
 */
format?: string, 
/**
 * The document's headings as an outline.
 */
outline?: boolean, 
/**
 * Rich text entries: the application nodes the document may hold
 * (vocabulary 18).
 */
richNodes?: Array<RichNodeSchema>, 
/**
 * Related resource (relation entries; the title is the record's `labels[name]`).
 */
relationship?: Relationship, 
/**
 * The value is computed on the server (`records/{key}/computed`), not in the record's data.
 */
computed?: boolean, 
/**
 * Relation entries: the title links to the related record (the user may list its resource).
 */
link?: boolean, 
/**
 * Custom entries: the script drawing the value (`vendor/name`).
 */
widget?: string, 
/**
 * Settings of the custom entry.
 */
props?: JsonValue, 
/**
 * Repeatable entries: the entries of each row (vocabulary 15).
 */
components?: Array<ComponentSchema>, };

export type RepeaterSchema = { 
/**
 * The row's inputs and layouts.
 */
components: Array<ComponentSchema>, 
/**
 * Relationship repeaters on an edit page: the inputs of a new row (the
 * child's create form); `components` are those of a stored row
 * (vocabulary 16).
 */
createComponents?: Array<ComponentSchema>, 
/**
 * Fewest rows.
 */
minItems?: number, 
/**
 * Most rows.
 */
maxItems?: number, 
/**
 * The user may reorder the rows.
 */
reorderable?: boolean, 
/**
 * Rows fold.
 */
collapsible?: boolean, 
/**
 * Rows start folded.
 */
collapsed?: boolean, 
/**
 * Each row's heading: `{input}` placeholders.
 */
itemLabel?: string, 
/**
 * The label of the button adding a row (translated).
 */
addActionLabel?: string, 
/**
 * Rows can be added.
 */
addable: boolean, 
/**
 * Rows can be removed.
 */
deletable: boolean, 
/**
 * Rows a create form starts with.
 */
defaultItems?: number, 
/**
 * A row can be copied.
 */
cloneable?: boolean, 
/**
 * Relationship repeaters: the child records and what the user may do.
 */
relationship?: RepeaterRelationship, };

export type RepeaterRelationship = { 
/**
 * The child resource.
 */
resource: string, 
/**
 * The has-many relation (its list reads the rows).
 */
relation: string, 
/**
 * The user may add rows.
 */
create: boolean, 
/**
 * The user may change rows (each row's envelope decides too).
 */
update: boolean, 
/**
 * The user may remove rows (each row's envelope decides too).
 */
delete: boolean, 
/**
 * The child field holding each row's position (rows are shown in its order).
 */
orderColumn?: string, };

export type InfolistSchema = { 
/**
 * Columns.
 */
columns: number, 
/**
 * Entries and layouts the user may see.
 */
components: Array<ComponentSchema>, 
/**
 * How wide the view page is (absent: the default).
 */
width?: FormWidth, 
/**
 * Made from the record's form (the resource declares no `infolist()`): its
 * conditions read fields as the edit form does, a declared infolist's the
 * stored values (vocabulary 17).
 */
fromForm?: boolean, };

export type ComputedValues = { 
/**
 * Values by name.
 */
data: Record<string, JsonValue>, 
/**
 * Names that failed (error, timeout, too large): shown as not available.
 */
failed: Array<string>, };

export type RichNodeSchema = { 
/**
 * Its name (`app.…`).
 */
name: string, 
/**
 * Label (translated).
 */
label: string, 
/**
 * Inside text, or a block.
 */
inline: boolean, 
/**
 * Its attributes, in declaration order.
 */
attrs: Array<RichAttrSchema>, };

export type RichAttrSchema = { 
/**
 * Its name.
 */
name: string, 
/**
 * The label of its input (translated; its name when none is declared).
 */
label: string, 
/**
 * `text`, `integer`, `boolean`, `one_of` or `uuid`.
 */
kind: string, 
/**
 * Without a default: the document must give it.
 */
required: boolean, 
/**
 * The value when left out.
 */
default?: JsonValue, 
/**
 * Text: fewest characters; integer: smallest value.
 */
min?: number, 
/**
 * Text: most characters; integer: largest value.
 */
max?: number, 
/**
 * `one_of`: the values allowed.
 */
options?: Array<string>, };

export type FormPatch = { "op": "set", 
/**
 * The field.
 */
field: string, 
/**
 * Its new value, in wire format.
 */
value: JsonValue, } | { "op": "set_if_empty", 
/**
 * The field.
 */
field: string, 
/**
 * Its new value, in wire format.
 */
value: JsonValue, } | { "op": "add_row", 
/**
 * The repeater.
 */
field: string, 
/**
 * Where the row goes.
 */
index?: number, 
/**
 * The row's values.
 */
row: JsonValue, } | { "op": "remove_row", 
/**
 * The repeater.
 */
field: string, 
/**
 * The row.
 */
index: number, } | { "op": "move_row", 
/**
 * The repeater.
 */
field: string, 
/**
 * The row.
 */
from: number, 
/**
 * Its new place.
 */
to: number, } | { "op": "flag_row", 
/**
 * The repeater.
 */
field: string, 
/**
 * The row.
 */
index: number, 
/**
 * The warning (translated).
 */
message: string, } | { "op": "notice", 
/**
 * The message (translated).
 */
message: string, };

export type FormReactRequest = { 
/**
 * `create` or `edit` (`view` is refused).
 */
operation: SchemaOperation, 
/**
 * The record's key (`edit` only).
 */
key?: string, 
/**
 * The input that changed (one with `reacts`).
 */
changed: string, 
/**
 * The form's values, in wire format (as a save sends them).
 */
state: Record<string, JsonValue>, 
/**
 * The changed input's value when the panel last applied an answer (or
 * opened the form).
 */
previous?: JsonValue, 
/**
 * A create form opened through a relation of another record: the
 * reaction sees the key that relation binds, as the save does.
 */
via?: FormReactVia, 
/**
 * Sent back unchanged: the panel applies only its latest request's answer.
 */
seq: number, };

export type FormReactVia = { 
/**
 * The parent resource.
 */
parent: string, 
/**
 * The parent record's key.
 */
record: string, 
/**
 * The relation's name.
 */
relation: string, };

export type FormReactResponse = { 
/**
 * The request's `seq`.
 */
seq: number, 
/**
 * The changes to apply, in order (messages translated).
 */
patches: Array<FormPatch>, 
/**
 * The reaction failed (error, timeout): nothing to apply.
 */
failed?: boolean, };

export type ComponentSchema = InputSchema | LayoutSchema | EntrySchema;

export type FormSchema = { 
/**
 * Columns.
 */
columns: number, 
/**
 * Components the user may see for the operation.
 */
components: Array<ComponentSchema>, 
/**
 * How wide the record's page is (absent: the default; vocabulary 11).
 */
width?: FormWidth, };

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
 * Labels shown for values of a text or badge column, in the user's
 * language (the column's own, or its form select's; vocabulary 20).
 */
options?: Array<SelectOption>, 
/**
 * How dates or times are shown, a pattern such as `HH:mm dd/MM/yyyy`
 * (`yyyy yy MMMM MMM MM M dd d EEEE EEE HH H hh h mm ss a z`, text in
 * single quotes; vocabulary 21). Absent: the panel's.
 */
format?: string, 
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
props?: JsonValue, 
/**
 * Hidden on screens narrower than this breakpoint (`sm`: 640 px, `md`:
 * 768 px, `lg`: 1024 px); presentation only (vocabulary 10).
 */
hideBelow?: Breakpoint, 
/**
 * Shows the number of items of the field's JSON list (vocabulary 15).
 */
count?: boolean, 
/**
 * Shows `counts[relation]` of each record: how many records of this
 * has-many relation it has that the user may list (vocabulary 15). The
 * column's `name` is not a field.
 */
counts?: string, };

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
relationship?: Relationship, 
/**
 * Label of the option for records without a value (select filters),
 * sent as `~none` in the `in` list (vocabulary 23).
 */
none?: string, };

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
form?: FormSchema, 
/**
 * Shown with its label in list rows; other row actions go in the row's
 * menu, those with color `danger` last (vocabulary 10).
 */
primary?: boolean, 
/**
 * Label of the button that runs it from its form dialog (default: the
 * action's label without a trailing ellipsis).
 */
submitLabel?: string, };

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
 * The view page's entries, for `operation=view` of a resource that
 * declares them (vocabulary 14); absent: the form is shown read-only.
 */
infolist?: InfolistSchema, 
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
labels?: { [key in string]: string }, 
/**
 * In the list route's pages: how many records of each has-many relation a count column
 * shows the record has, among those the user may list (vocabulary 15).
 * Absent for a relation the user may not list or too many to check.
 */
counts?: { [key in string]: number }, };

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
 * Keys the policy denied, or the action is hidden on (several records).
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
replayed?: boolean, 
/**
 * A message for the user, shown by the panel (plain text, at most
 * [`MAX_OUTCOME_MESSAGE`] characters).
 */
message?: string, 
/**
 * A page the panel opens in a new tab (`noopener`, `noreferrer`) once
 * the action committed: an `https://` URL, or a path of this site
 * (`/admin/…`, `/blog/…`). Only for a run on one record; any other
 * value fails the action (rolled back).
 */
open?: string, };

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
 * `too_many_to_export` (more records than a background export holds),
 * `too_many_matches` (more candidates than a per-record policy checks),
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

export type PanelUploads = { 
/**
 * Largest accepted file, in bytes (`max_upload_bytes`).
 */
maxBytes: number, 
/**
 * Uploads the server processes at once (`max_concurrent_uploads`; more
 * answer 429): the panel sends at most this many at a time.
 */
maxConcurrent: number, };

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
queue: boolean, 
/**
 * The Schedules page (`schedules.view`).
 */
schedules: boolean, };

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
label: string, 
/**
 * Wire kind of the value (`instant`, `date`, …).
 */
valueType?: string, 
/**
 * Labels of the field's static options (its form input's), in the user's language.
 */
options?: Array<SelectOption>, 
/**
 * The resource table's column of the field (type, badge colors, custom cell), when it has one.
 */
column?: ColumnSchema, };

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
values: { [key in string]: string }, 
/**
 * The value of each column, in wire format (readable fields only), for
 * option labels, custom cells and dates.
 */
data: Record<string, JsonValue>, };

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
canManage: boolean, 
/**
 * Jobs finished in the last 24 hours (from the history).
 */
lastDay: QueueLastDay, };

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
 * `pending`, `scheduled` (shown as Delayed), `running`, `stalled` or `failed`.
 */
status: string, 
/**
 * `Job::describe`.
 */
description?: string, 
/**
 * Its current run started at (running jobs).
 */
startedAt?: string, 
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

export type QueueLastDay = { 
/**
 * Done.
 */
done: number, 
/**
 * Failed for good.
 */
failed: number, };

export type JobRun = { 
/**
 * This record's id (UUID).
 */
id: string, 
/**
 * The job's id (UUID).
 */
jobId: string, 
/**
 * Queue name.
 */
queue: string, 
/**
 * `Job::KIND`.
 */
kind: string, 
/**
 * `Job::describe`.
 */
description?: string, 
/**
 * `done`, `failed` or `cancelled`.
 */
status: string, 
/**
 * Runs used.
 */
attempts: number, 
/**
 * Dispatched at (RFC 3339).
 */
createdAt: string, 
/**
 * Its last run started at (RFC 3339).
 */
startedAt?: string, 
/**
 * Finished at (RFC 3339).
 */
finishedAt: string, 
/**
 * How long its last run took.
 */
durationMs?: number, 
/**
 * Why it failed (a safe message).
 */
lastError?: string, 
/**
 * The user who dispatched it.
 */
initiatedBy?: QueueUser, };

export type JobRunPage = { 
/**
 * Finished jobs.
 */
data: Array<JobRun>, 
/**
 * Continuation.
 */
meta: QueueJobPageMeta, };

export type QueueJobDetail = { 
/**
 * The job, while it exists.
 */
job?: QueueJob, 
/**
 * Its finished runs, newest first (at most 20).
 */
runs: Array<JobRun>, };

export type ScheduleActionInfo = { "kind": "job", 
/**
 * Its kind.
 */
jobKind: string, } | { "kind": "closure" };

export type ScheduleRunInfo = { 
/**
 * Run id (UUID).
 */
id: string, 
/**
 * The slot it ran for (scheduled runs).
 */
slotAt?: string, 
/**
 * `schedule` or `manual`.
 */
trigger: string, 
/**
 * Started (or was dispatched) at.
 */
startedAt: string, 
/**
 * Finished at.
 */
finishedAt?: string, 
/**
 * `dispatched`, `running`, `succeeded`, `failed`, `skipped`, `cancelled` or
 * `unknown` (also a `running` run past its time: its end was not recorded).
 */
status: string, 
/**
 * How long it ran.
 */
durationMs?: number, 
/**
 * A task's summary.
 */
summary?: string, 
/**
 * Why it failed (a safe message).
 */
error?: string, 
/**
 * The job that ran it.
 */
jobId?: string, 
/**
 * The user who ran it (id).
 */
initiatedBy?: string, };

export type ScheduleInfo = { 
/**
 * Name.
 */
name: string, 
/**
 * When it fires, in words.
 */
rule: string, 
/**
 * What it does.
 */
action: ScheduleActionInfo, 
/**
 * Slots wait for the previous job.
 */
withoutOverlapping: boolean, 
/**
 * Next slot (RFC 3339).
 */
nextAt?: string, 
/**
 * Last claimed slot.
 */
lastSlotAt?: string, 
/**
 * Its latest run.
 */
last?: ScheduleRunInfo, };

export type SchedulerInfo = { 
/**
 * `schedule.enabled`.
 */
enabled: boolean, 
/**
 * `schedule.grace_secs`.
 */
graceSecs: number, };

export type ScheduleList = { 
/**
 * Every schedule, as declared.
 */
data: Array<ScheduleInfo>, 
/**
 * The scheduler on this instance.
 */
scheduler: SchedulerInfo, 
/**
 * Workers run jobs on this instance (`queue.enabled`).
 */
workers: boolean, 
/**
 * May run schedules now (`schedules.manage`).
 */
canManage: boolean, };

export type ScheduleRunPage = { 
/**
 * Runs.
 */
data: Array<ScheduleRunInfo>, 
/**
 * Continuation.
 */
meta: QueueJobPageMeta, };

export type ScheduleRunResponse = { 
/**
 * The run, dispatched.
 */
run: ScheduleRunInfo, };

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
data: JsonValue, } | { "kind": "infolist", 
/**
 * The entries.
 */
infolist: InfolistSchema, 
/**
 * The values, by entry name.
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
permissions: Array<string>, 
/**
 * Upload limits, when the media library is enabled and the user may
 * upload (`media.create`).
 */
uploads?: PanelUploads, 
/**
 * The media library's collections, in declaration order, labels
 * translated: when the panel declares some and the user may list or
 * upload files (vocabulary 23).
 */
mediaCollections?: Array<SelectOption>, };

export type DataResponse<T> = { 
/**
 * Payload.
 */
data: T, };
