# Docs v2 inventory — batches 6–8 (admin pages)

Baseline: `git rev-parse --short HEAD` = **d4e87e3**. Paths are relative to the repository root. Snippet syntax
already in use: `<<< @/../../../examples/demo/src/bootstrap/providers.rs#register` (only one `// #region` exists
today, `examples/demo/src/bootstrap/providers.rs:7`). Every "Examples available" entry below needs a new region.

---

### admin/overview.md
- **Public API covered**: `AdminProvider::new/brand/media/resource/translations/home`, `AuthProvider::new`,
  `rustcms::storage::StorageProvider`, `ResourceMeta::{group,icon,sort,navigation}`, `PanelPageMeta::{permission,sort}`,
  config `auth.admin_path` (default `/admin`), `ColorScheme`, `rustcms permission:sync`, icon names.
- **Wrong or outdated**
  - The CSP is shortened. The real policy also has `img-src 'self' data:; font-src 'self'; connect-src 'self'
    [ws(s)://origin…]; object-src 'none'; base-uri 'self'; form-action 'self'` (crates/rustcms-admin/src/panel/spa.rs:31-33,
    and `content_security_policy` at :43-58 adds `ws://`/`wss://` origins).
  - The path table is missing panel routes: `/admin/forgot-password`, `/admin/reset-password`, `/admin/invitation`
    (admin-ui/src/app/router.tsx:41-59), `/admin/system/users/create`, `/system/users/<id>`, `/system/roles/create`,
    `/system/roles/<id>` (router.tsx:143-160), and `/admin/p/<slug>/…` sub-paths for React pages (router.tsx:177).
    `/admin/r/<slug>/import` exists only for `.importable()` resources.
  - The code sample registers providers inline. Apps use `src/bootstrap/providers.rs` and
    `providers::admin_panel_provider::provider()` (examples/demo/src/bootstrap/providers.rs:8-15; template
    crates/rustcms-cli/templates/app/src/providers/admin_panel_provider.rs.tmpl).
  - Not wrong, but not stated: `reset-password` and `invitation` pages send `Referrer-Policy: no-referrer`, which
    overrides `same-origin` (spa.rs:231-234).
- **Missing**
  - Default brand `"rustcms"` (crates/rustcms-admin/src/panel/provider.rs:52).
  - `AdminProvider::extension_timeout` (1–60 s, default 10; provider.rs:108) and `extension_script_v2` (provider.rs:96).
    They probably belong in custom-fields/react-pages; link them from here.
  - Commands the panel adds: `idempotency:prune`, `media:prune` (only with `.media()`), `lang:missing`,
    `admin:branding` (provider.rs:473-533, :618).
  - `PANEL_LOCALES = ["en","vi"]` (provider.rs:43).
  - Boot panics: AuthProvider not registered first (provider.rs:247); `.media()` without StorageProvider (provider.rs:299-306).
  - `GET {admin}/api/panel` (http/mod.rs:105).
  - Icons used by the LMS (`book-open`, `list`, `newspaper`, `clipboard-check`) are not in the icon set
    (admin-ui/src/components/icons.tsx:43). They fall back to the generic icon, which matches the text, but the
    examples show it happening.
- **Examples available**: `examples/demo/src/bootstrap/providers.rs#register` (already a region);
  `examples/demo/src/providers/admin_panel_provider.rs` fn `provider`; `examples/lms/src/providers/admin_panel_provider.rs`
  fn `provider` (has `.theme`); template `admin_panel_provider.rs.tmpl`.
- **Screenshots needed**: the shell with sidebar groups and icons, light and dark (examples/demo: Content group +
  Settings; RDockPanel shows a richer icon set and a custom home page via `.home()`); the phone layout with the
  collapsed sidebar; the login page.
- **Linked anchors**: `#navigation`, `#order` (from admin/resources.md, admin/customizing.md), `#hiding-a-resource`
  (resources.md), `#home-page` (customizing.md, release-notes.md).

### admin/dashboard.md
- **Public API covered**: `rustcms::admin::dashboard::{Dashboard, Widget, Bucket}`;
  `Dashboard::{new, widget, resource_links}`; `Widget::{stat, chart, recent, label, span, permission, filter,
  compare_last_days, per, last, columns, limit, sort}`; `FilterOp`; `AdminProvider::dashboard`; permission
  `dashboard.<name>`; `GET {admin}/api/dashboard`, `GET {admin}/api/dashboard/widgets/{name}`.
- **Wrong or outdated**
  - The code sample mixes the LMS's resources (`learners`, `enrollments`, `courses`) with a `posts` resource. No real
    app has both. Import the LMS dashboard instead.
  - `use rustcms::admin::resource::FilterOp` works, but `rustcms::admin::FilterOp` is the canonical re-export
    (crates/rustcms-admin/src/lib.rs:43).
  - Everything else matches: limits at features/dashboard/mod.rs:35-43, response shapes at :323-430.
- **Missing**
  - Defaults:
    - `chart` defaults to `Bucket::Day` and the last 14 buckets (mod.rs:99).
    - `recent` defaults to 5 records, and its sort defaults to the table's `default_sort` (mod.rs:102-104).
    - `span` defaults to 1 (mod.rs:84).
  - `compare_last_days` accepts 1–3660 days (mod.rs:292-295).
  - Widget names must be 1–64 characters of `a-z0-9_` (mod.rs:276-278).
  - `Widget::required_permission()` and the public consts `MAX_BUCKETS`, `MAX_RECENT` (mod.rs:35-37, :183).
  - 429 responses:
    - the rate limit, keyed `dashboard:<user>` (mod.rs:571);
    - waiting more than 10 s for a computing slot returns 429 with `Retry-After: 5` (mod.rs:573-576).
  - `recent` records carry `{key, title?, values, data}` and columns `{name, label, valueType?, options?, column?}`
    (mod.rs:366-405).
  - Chart bucket `start` is `YYYY-MM-DD` UTC (mod.rs:357).
  - Unknown widget names return 404 before any permission check (mod.rs:567).
- **Examples available**: `examples/lms/src/providers/admin_panel_provider.rs` (the `.dashboard(Dashboard::new()…)`
  block: stat, compared stat, chart, recent); `examples/demo/src/providers/admin_panel_provider.rs` (stat, weekly chart,
  recent posts). Suggested regions: `dashboard`.
- **Screenshots needed**: the dashboard with widgets above the resource cards (LMS: compared stat with trend, weekly
  chart span 2, recent courses; demo: posts stat, published-per-week chart, latest posts). RBlog's home would show it
  on real data. Also the "too many records" state, if it can be staged.
- **Linked anchors**: `#api` (from reference/admin-api.md).

### admin/authorization.md
- **Public API covered**: `Policy::{allows, admits, view_depends_on_record}`, `Resource::{policy, scope}`, `Ctx::{can,
  conn, principal}`, `Field` rules `readable/creatable/updatable/guarded`, `rustcms make:policy`,
  `AuthProvider::permission(ADMIN_GUARD, name, group)`, `rustcms::async_trait`.
- **Wrong or outdated**
  - The `PostPolicy` sample decides per record but does not override `admits`. Users without the permission then get
    404/403 by key, which is the existence probing the page warns about. The demo's real policy overrides `admits`
    (examples/demo/src/policies/post_policy.rs:25-28). Import that instead.
  - The sample compares `r.get("author_id") == Some(&json!(ctx.principal.id))`. That works only because `author_id` is
    an i64, which the wire format sends as a string (ADR 0003). An i32 owner column is a JSON number and never matches.
    Say that `record` is every column in wire format, unfiltered by field rules (resources/policy.rs:30-33).
  - The `query_one_raw(/* … */)` sample does not compile as written. `ctx.conn()` returns
    `sea_orm::DatabaseExecutor` (engine/context.rs:183-188); show a SeaORM query on it.
- **Missing**
  - `PermissionPolicy::new(slug)` is public and can be wrapped or delegated to. It admits by permission and has
    `view_depends_on_record = false` (policy.rs:60-84).
  - Ability constants `rustcms::admin::policy::ability::{VIEW_ANY, VIEW, CREATE, UPDATE, DELETE, RESTORE,
    FORCE_DELETE, EXPORT, IMPORT}` (policy.rs:9-28).
  - Which `{slug}.{ability}` permissions are declared:
    - always: the 5 CRUD abilities;
    - with soft deletes: `restore`, `force_delete`;
    - with `.exportable()` / `.importable()`: `export` / `import`;
    - for actions: each action's ability, plus its `requires_permission`;
    - for belongs-to-many relations: `attach_<rel>`, `detach_<rel>`, and `update_pivot_<rel>` when the relation has
      pivot fields.

    Source: engine/adapter/operations.rs:103-130.
  - The list scan limit is `MAX_LIST_SCAN = 10_000`, then 422 "Too many matching records…"
    (engine/adapter/mod.rs:630; operations.rs:342-349). The page states the limit but not where it comes from.
  - Actions are hidden from the schema when `Policy::admits` refuses them, not only by `requires_permission`
    (engine/adapter/schema.rs:386).
  - Other `Ctx` helpers usable in policies and hooks: `t`, `t_in`, `keyring`, `dispatch`, `audit`, `records`, `record`
    (context.rs:89-140).
  - The `make:policy` output and where it goes (`src/policies/<name>.rs`, crates/rustcms-cli/src/resource.rs:353-367).
- **Examples available**:
  - `examples/demo/src/policies/post_policy.rs`: `PostPolicy` with `allows` + `admits` and the `OWN` list.
  - `examples/lms/src/policies/enrollment_policy.rs`: `EnrollmentPolicy`, record-independent with
    `view_depends_on_record = false`.
  - `examples/demo/src/admin/resources/post_resource.rs`: `PostResource::scope`.
  - `examples/lms/src/admin/resources/course_resource.rs` (`scope`, plus field rules with `creatable`/`updatable`
    at :41-42).
  - `examples/demo/src/providers/auth_service_provider.rs:10`: the `.permission(…)` call.
- **Screenshots needed**:
  - One schema seen by two users: an editor with only `posts.update_own` sees their own posts and fewer actions.
    The demo's editor user works for this.
  - An "Access denied" page.
  - A disabled field in a form.
  - Mostly conceptual; local demo is enough.
- **Linked anchors**: none with an anchor (other pages link to `authorization.md` without one; the
  `#the-roles-page-areas-labels-and-presets` links go to security/authorization.md).

### admin/resources.md
- **Public API covered**:
  - `Resource` (`Entity`, `SLUG`, `meta`, `fields`, `form`, `table`, `policy`, `scope`, `actions`, `relations`, hooks
    `mutate_before_create/update`, `after_save/delete/trash/restore/force_delete/relation_change`);
  - `Field::column` and its builders (`required, max_length, searchable, sortable, filterable, belongs_to, guarded,
    readable, creatable, updatable, label, media, media_list, accept, max_size, rich_text, key_value, max_items,
    encrypted, encrypted_map, audit_redact`);
  - `ResourceMeta` (`new, plural, title, group, icon, sort, navigation, key, unique, soft_deletes,
    soft_deletes_column, lock_parent, exportable, importable, import_key, audit`);
  - `KeyStrategy::{DbGenerated, AppGenerated, Natural}`;
  - `rustcms make:resource Post --generate`; `permission:sync`.
- **Wrong or outdated**
  - The `make:resource` flag `--belongs-to FIELD=SLUG` is not mentioned. `--generate` without it turns `*_id` integer
    columns into TextInputs with a TODO (crates/rustcms-cli/src/main.rs:68-70; resource.rs:225-230).
  - "plus action abilities": the declared permission set is conditional (operations.rs:103-130); see authorization.
  - Hooks use `#[async_trait]` via `use rustcms::async_trait;`. The demo writes `#[async_trait::async_trait]`
    (post_resource.rs:17). Pick one for the snippets.
  - The intro sample uses `KeyStrategy::AppGenerated` with `Value`/`Uuid` but no imports. No example app uses
    AppGenerated keys (tests do: crates/rustcms-admin/tests/support/mod.rs:322).
- **Missing**
  - Boot rules:
    - the primary key must be `i32`, `i64`, `String` or `Uuid`; composite keys panic
      (engine/adapter/mod.rs:116-124);
    - the slug is at most 64 bytes and may not be `user, role, permission, settings, session, job, queue`
      (mod.rs:138-143);
    - a field is declared once (mod.rs:135);
    - the title field must be a field (mod.rs:150);
    - the PK may not be `.audit_redact()` while auditing (mod.rs:144-147);
    - `import_key` needs a single-column `unique` (mod.rs:167-172);
    - `unique` columns must exist (mod.rs:176);
    - media and rich text fields: a JSON column for rich text, UUID for `media`, JSON for `media_list`, and no column
      default (mod.rs:280-299).
  - `Field::make(name)` (by name), `Field::can_read`, `Field::can_write`, `Operation::{Create, Update}`
    (resources/field.rs:14-19, :59, :240-256).
  - `ResourceMeta::unique` semantics: `name` is the PostgreSQL constraint name, `columns` identify it on SQLite;
    composite constraints go to form errors (resources/resource.rs:151-164).
  - `make:resource --generate` rules (crates/rustcms-cli/src/resource.rs:100-106, 147-258):
    - `created_at`, `updated_at`, `deleted_at` are skipped;
    - secret-looking names (`password, secret, token, hash, api_key, salt`) are skipped;
    - owner columns (`author_id, user_id, owner_id, created_by, updated_by`) become `.guarded().readable(|_| true)`;
    - `String` keys become `KeyStrategy::Natural`;
    - long-text names become a `Textarea`.
  - Hook helpers: `ctx.dispatch(txn, &job)` (context.rs:123), which is what "Dispatch a job there (outbox)" means;
    `ValidationErrors::field/add/add_form` as hook errors (used in examples/lms/src/admin/resources/enrollment_resource.rs:116,
    course_resource.rs:79-85).
  - `Ctx::records(slug, &ListQuery)` and `Ctx::record(slug, key)` (context.rs:98-113).
  - Wire types `Envelope`, `Page`, `PageMeta`, `ListQuery`, `Filter`, `Limits`, `Trashed` are exported (lib.rs:42-45).
    The guide probably only needs them for custom pages.
- **Examples available**:
  - `examples/demo/src/admin/resources/post_resource.rs`: `PostResource` (`meta`, `fields`, `mutate_before_create`,
    `scope`). Needs regions `meta`, `fields`, `hooks`.
  - `examples/demo/src/admin/resources/tag_resource.rs`: `TagResource`, a minimal resource with `unique` +
    `import_key`.
  - `examples/demo/src/admin/resources/category_resource.rs`: `CategoryResource::meta`, `unique`.
  - Parent locks: `examples/lms/src/admin/resources/enrollment_resource.rs` (`EnrollmentResource::meta` with
    `lock_parent`, `mutate_before_create/update` counting seats) and
    `examples/lms/src/admin/resources/lesson_resource.rs` (`meta`).
  - `examples/demo/src/providers/admin_panel_provider.rs`: registration.
  - No example uses `after_save` / `after_trash` / `after_restore` / `after_delete` / `after_force_delete`,
    `KeyStrategy`, `soft_deletes_column` or `audit(false)`. Keep those hand-written or add them to the demo.
- **Screenshots needed**:
  - the posts list with the trash toggle (demo: posts has soft deletes);
  - a record page (demo post);
  - a unique-constraint error under a field (demo: a duplicate category slug).

  RBlog would show a real posts resource.
- **Linked anchors**: `#fields` (from forms.md, relationships.md), `#hooks` (release-notes.md), `#parent-locks`
  (examples/lms.md, release-notes.md). Internal anchors: `#primary-keys`, `#soft-deletes`.

### admin/forms.md
- **Public API covered**:
  - `Form::{new, columns, schema, width}`, `FormWidth::{Default, Wide, Full}`, `components!`;
  - inputs `TextInput (.email/.password/.url/.prefix/.suffix)`, `Textarea`, `NumberInput`, `SelectInput (.options)`,
    `Toggle`, `Checkbox`, `CheckboxList`, `DatePicker`, `DateTimePicker`, `Hidden`, `KeyValue`, `FileUpload`,
    `RichEditor`, `CustomInput::make(field, widget)`, `SecretInput`, `SecretMapInput`, `BelongsToManySelect`;
  - common options `.label/.placeholder/.help/.column_span`;
  - layouts `Section::make` (`.collapsible`, `.compact`), `Grid::make`, `Tabs::make`/`Tab::make`,
    `Split::make().main().aside().aside_first_on_mobile()`;
  - `Field::key_value/max_items/encrypted/encrypted_map`, `Encrypted<T>`, `ctx.keyring()`.
- **Wrong or outdated**
  - **`SelectInput::options` and `CheckboxList` options are not checked on resource writes.** `accept_input` builds
    `InputRule { options: None }` (crates/rustcms-admin/src/engine/adapter/input.rs:39). Only action forms and
    BelongsToMany check options (adapter/action_form.rs:42,70; adapter/many.rs:571). The page implies the options
    bound the value. Say that they are presentation only and must be validated in a hook (the LMS does this in
    `enrollment_service::check`).
  - The "Misconfiguration … panics" paragraph appears twice in a row after "An editor layout" (forms.md ~line 125).
    The second one is about filters and sorts, which belong in tables.md.
  - "Without `form()`, one input per field, chosen by column type" is incomplete:
    - guarded fields get no input;
    - a `belongs_to` field gets a Select;
    - integer, float and decimal columns get a NumberInput;
    - bool gets a Toggle;
    - date gets a DatePicker;
    - local date-time and instant get a DateTimePicker;
    - **JSON gets a raw Textarea**;
    - string and UUID get a TextInput;
    - encrypted fields get a Secret or SecretMap input;
    - `Form::width` is kept on an auto form.

    Source: engine/adapter/mod.rs:216-221, 569-594.
- **Missing**
  - `Layout::columns(n)` on Section, Grid and Tab (forms/layouts.rs:46; the demo uses `Tab::make("Publishing").columns(2)`,
    post_resource.rs:48). `Form::columns(n)` is used in the sample but never explained (forms/form.rs:72).
  - `CustomInput::props(json)`, at most 4 KiB, and the `vendor/name` widget-name rule (forms/inputs.rs:166-191;
    adapter/mod.rs:273-278). Link to custom-fields.md.
  - Boot rules:
    - Secret and SecretMap inputs only for encrypted fields, and those fields only with those inputs (mod.rs:222-231);
    - `BelongsToManySelect` must name a belongs-to-many relation, not share a field's name, appear once, take no
      options and not target media (mod.rs:236-266);
    - a form input must be a field (mod.rs:266-271);
    - prefix and suffix only on TextInput (mod.rs:527-534);
    - layout rules (mod.rs:536-566).
  - `KeyValue` constants: keys up to 255 characters (engine/validate.rs:10); defaults 50 items and 1,000 characters
    (mod.rs:59-61). These are documented; the source lines are listed for reference.
  - Encrypted fields: no column default allowed, text column only (mod.rs:187, 205). Rotation with `key:reencrypt`
    (provider.rs:252-255).
  - Settings pages share the layout checks (`check_form_layout`, mod.rs:525).
- **Examples available**:
  - `examples/demo/src/admin/resources/category_resource.rs`: `CategoryResource::form`, the editor layout with
    `FormWidth::Full`, `Split`, `.prefix`, a compact collapsible Section and `CustomInput.props`. This could replace
    the hand-written "An editor layout" sample.
  - `examples/demo/src/admin/resources/post_resource.rs`: `PostResource::form`, with Tabs, Tab columns, FileUpload,
    RichEditor, BelongsToManySelect and KeyValue with help.
  - `examples/lms/src/admin/resources/course_resource.rs`: `CourseResource::form`, with `Form::columns(2)`,
    NumberInput help and `column_span(2)`.
  - `examples/lms/src/admin/resources/enrollment_resource.rs`: `EnrollmentResource::form`, a SelectInput with static
    `.options(STATUSES)`.
  - `examples/lms/src/admin/resources/learner_resource.rs`: `form` with `.email()`.
  - `examples/demo/src/admin/resources/integration_resource.rs`: `IntegrationResource::fields`, with `encrypted` and
    `encrypted_map().max_items(20)`.
- **Screenshots needed**:
  - the demo post form tabs;
  - the demo category editor layout at 1440px and 390px (E2E `editor-layout.spec.ts` already drives both);
  - the KeyValue input;
  - the secret and secret-map inputs (demo integrations);
  - the unsaved-changes dialog;
  - a validation error under a field with the tab opened.

  RBlog's post editor (Split + SEO collapsible section) is the real-world editor layout the 0.10.2 work was made for.
- **Linked anchors**: `#encrypted-fields` (resources.md, digging-deeper/encryption.md, release-notes.md), `#key-value`
  (resources.md), `#inputs` (admin/i18n.md), `#an-editor-layout`, `#leaving-with-unsaved-changes`
  (release-notes.md).

### admin/tables.md
- **Public API covered**:
  - `Table::{new, columns, filters, default_sort}`;
  - columns `TextColumn`, `BadgeColumn (.color)`, `BooleanColumn`, `DateColumn`, with `.label/.cell/.props/.hide_below`;
  - `Breakpoint::{Sm, Md, Lg}`;
  - filters `SelectFilter (.options)`, `TernaryFilter`, `DateRangeFilter`, with `.label`;
  - field flags `searchable/sortable/filterable`.
- **Wrong or outdated**
  - "Without it, the first fields are columns": it is the first **6** fields that are not guarded, not encrypted and
    not JSON. Bool fields get a BooleanColumn, date and time fields a DateColumn, the rest a TextColumn
    (engine/adapter/mod.rs:598-610).
  - The filters table says `SelectFilter` picks "one of the options". The UI picks one, but it is sent as
    `filter[field][in]` (admin-ui/src/renderers/table.tsx:146; tables/filters.rs:12), so API clients may send several.
    State that.
  - The sample's `BadgeColumn` and `TernaryFilter::make("featured")` match no example resource. Nothing is wrong,
    but it cannot be imported.
- **Missing**
  - Boot rules:
    - a column must be a field, and not an encrypted one;
    - a filter needs a `.filterable()` field;
    - `default_sort` needs a `.sortable()` field;
    - cell names must be `vendor/name`, and props at most 4 KiB.

    Source: mod.rs:314-351. The forms page currently says part of this.
  - The page-size options are 10/25/50/100 (mod.rs:617).
  - `Limits` default: `per_page` at most 100, search at most 100 characters, at most 10 filters, at most 100 items in
    an `in` filter, at most 500 keys (resources/query.rs:107-110).
  - Query string `filter[field][op]` with `FilterOp` eq/in/gt/gte/lt/lte (query.rs:13-26; table.tsx:77).
  - TernaryFilter sends `eq`, DateRangeFilter `gte`/`lte` (filters.rs:11-18).
  - `default_sort` is dropped when the user cannot read the field, and search is offered only if the user can read a
    searchable field (engine/adapter/schema.rs:404-406).
  - Labels are translated per resource (`t_in(slug, …)`, schema.rs:349, 375).
  - The trash toggle (`trashed=with|only`) and the export button (link resources.md#soft-deletes, import-export.md).
  - Columns of `belongs_to` fields show the related title; a SelectFilter on a `belongs_to` field searches the target
    (table.tsx:148-161).
- **Examples available**:
  - `examples/demo/src/admin/resources/post_resource.rs`: `PostResource::table`, with
    `hide_below(Breakpoint::Md)`, Ternary/Select/DateRange filters and `default_sort`. Use it as the main snippet.
  - `examples/lms/src/admin/resources/enrollment_resource.rs`: `EnrollmentResource::table`, with the custom cell
    `.cell("lms/progress")` and `SelectFilter.options(STATUSES)`.
  - `examples/lms/src/admin/resources/course_resource.rs`: `CourseResource::table`.
  - No example uses `BadgeColumn`. Adding a status badge to the demo or LMS would let the page drop its hand-written
    sample.
- **Screenshots needed**:
  - the demo posts list with filters open;
  - the same list at phone width (category and date columns hidden);
  - the bulk selection bar;
  - the LMS enrollments list with the progress cell.

  RBlog's post list with status badges would show BadgeColumn.
- **Linked anchors**: `#narrow-screens` (release-notes.md).

### admin/actions.md
- **Public API covered**:
  - `Action::<E>::new`, `Action::with_outcome`;
  - `.label, .bulk, .requires_confirmation, .ability, .requires_permission, .visible_when, .primary, .destructive,
    .color, .icon, .form, .submit_label`;
  - `ActionOutcome::{message, open, with_message, with_open}`;
  - `ActionField::{text, textarea, integer, boolean, date, datetime, select, belongs_to}` with
    `.required/.default/.max_length/.label`;
  - `GET …/actions/{action}/fields/{field}/options`; `Idempotency-Key`; `ValidationErrors`.
- **Wrong or outdated**
  - "`.requires_permission(…)` … hidden from the schema" is true, but the action is also hidden when
    `Policy::admits(ctx, ability)` refuses (engine/adapter/schema.rs:386). Say so.
  - The keys list says `processed`, `denied`, `missing`. `ActionOutcome` also has `replayed` (resources/action.rs:230-232).
  - Otherwise accurate against resources/action.rs:118-213 and :249-368 and CHANGELOG 0.10.1/0.10.3. The phone rule is
    described ("Edit and Delete go under ⋯ too, Delete last").
- **Missing**
  - Boot rules:
    - an action name is declared once and is at most 57 bytes (adapter/mod.rs:354-355);
    - it may not be a CRUD ability (`view_any, view, create, update, delete, restore, force_delete, export, import`;
      mod.rs:356-371);
    - form field names are unique snake_case (adapter/action_form.rs:189-196);
    - **a `belongs_to` ActionField cannot have a `.default`** (action_form.rs:199-203).
  - `ActionField::help`, `ActionField::placeholder` (action.rs:472-483).
  - The declared permission is `{slug}.{ability}`, where the ability is the action name unless `.ability(…)` is
    given; `requires_permission` names are declared for `permission:sync` too (operations.rs:117, 129).
  - Request shape `POST {admin}/api/resources/{slug}/actions/{action}` with `{keys, data?}` (http/actions.rs:22).
    At most 500 keys (`Limits::keys`, query.rs:109).
  - `MAX_OUTCOME_MESSAGE` and `MAX_OUTCOME_URL` are public consts (action.rs:249-253).
  - `BoxFuture` is re-exported (lib.rs:33).
- **Examples available**:
  - `examples/demo/src/admin/resources/post_resource.rs`, `PostResource::actions`. Split it into regions:
    - `publish`: bulk + confirmation;
    - `move`: a form with `ActionField::belongs_to`;
    - `unpublish`: `visible_when` + `primary` + `destructive` + `.ability("update")`;
    - `preview`: `with_outcome` + `ActionOutcome::open().with_message()`.

    These cover almost every hand-written sample except the multi-field form (`select/integer/text`) and
    `submit_label`.
  - `examples/lms/src/admin/resources/course_resource.rs`: `CourseResource::actions` (`enroll`), with
    `requires_permission`, a form, and `ValidationErrors` on a field or the form that roll the action back.
  - `examples/lms/src/admin/resources/lesson_resource.rs`: fn `moving` (move up/down actions).
- **Screenshots needed**:
  - a demo posts row with the primary "Unpublish" and the ⋯ menu with the destructive item last;
  - the bulk bar with Publish / Move;
  - the "Move to category" dialog;
  - the confirmation dialog;
  - the outcome toast with its "open" link (demo Preview);
  - a record page header on a phone with ⋯ (0.10.3).

  RDockPanel would show real `with_outcome` uses (opening a deploy log or console).
- **Linked anchors**: `#placement-in-lists` (overview.md, tables.md, relationships.md, release-notes.md),
  `#actions-with-a-form` (reference/admin-api.md, release-notes.md, upgrade.md), `#per-record-visibility`,
  `#outcomes-a-message-a-page-to-open` (release-notes.md).

### admin/relationships.md
- **Public API covered**:
  - `Relation::has_many(name, target, field)`, `Relation::belongs_to_many(name, target, parent_col, related_col)`,
    `.pivot(Vec<Field>)`, `.label`;
  - `Field::belongs_to`;
  - `Resource::relations`, `Resource::after_relation_change`, `RelationChange {relation, attached, detached,
    pivot_updated}`;
  - abilities `attach_<rel>` / `detach_<rel>` / `update_pivot_<rel>`; `BelongsToManySelect`;
  - routes `…/relations/{relation}/options`; `Idempotency-Key`.
- **Wrong or outdated**
  - The pivot migration sample uses raw `Table::create()` without the `rustcms::migration::create_table(manager, …)`
    helper and without foreign-key names. CLAUDE.md and ADR 0010 require the helpers. Import the demo migration
    instead (examples/demo/src/database/migrations/m20261022_000001_create_tags.rs, `Migration::up`).
  - The comment "src/models/post_tag.rs: post_id, tag_id (primary key over both), position" is fine, and the model
    itself could be imported (examples/demo/src/models/post_tag.rs).
  - The text otherwise matches the registry checks (engine/registry.rs:262-330, 395-462).
- **Missing**
  - Pivot field restrictions:
    - not a `belongs_to`, media, rich text or encrypted field;
    - not a key column;
    - not declared twice;
    - a column that is not declared must be nullable, have a default, or be a `.required()` pivot field.

    Source: registry.rs:432-460.
  - Relation names are unique snake_case (registry.rs:290-296).
  - The full route list: `GET/POST …/records/{key}/relations/{relation}`, `POST …/attach`, `POST …/detach`,
    `PATCH …/links/{target}`, `GET …/records/{key}/relations/{relation}/options`, and
    `GET …/relations/{relation}/options` (http/mod.rs:114-120). The last one is linked from reference/admin-api.md,
    but the page itself names only it.
  - `MAX_LINK_KEYS = 100` (engine/adapter/many.rs:17) and the 500-per-save bound from `Limits::keys` (many.rs:463).
    The numbers are documented; the source lines are listed for reference.
  - Public link types `LinkOp`, `LinkRequest`, `LinkOutcome`, `LinkOption` (resources/links.rs; lib.rs:38).
    `LinkOp::ability(relation)` builds the ability name (links.rs:21).
  - The default relation label is the name in words (resource.rs:429).
- **Examples available**:
  - Has-many:
    - `examples/demo/src/admin/resources/category_resource.rs`: `CategoryResource::relations`, the exact
      `has_many("posts","posts","category_id")` from the page;
    - `examples/lms/src/admin/resources/course_resource.rs`: `CourseResource::relations` (two has-many);
    - `examples/lms/src/admin/resources/learner_resource.rs`: `relations`.
  - Belongs-to-many:
    - `examples/demo/src/admin/resources/post_resource.rs`: `PostResource::relations`, with a pivot `position`;
    - `PostResource::form`: `BelongsToManySelect::make("tags")`;
    - `examples/lms/src/admin/resources/news_resource.rs`: `NewsResource::relations`, without pivot fields.
  - Policy: `examples/demo/src/policies/post_policy.rs`, the `OWN` list including `attach_tags/detach_tags/
    update_pivot_tags`.
  - No example implements `after_relation_change`. Add one to the demo (touch `updated_at`) or keep it hand-written.
- **Screenshots needed**:
  - a demo category page with the Posts has-many table and its New button;
  - a demo post page with the Tags relation manager (Attach dialog with the position pivot, Detach, Edit link);
  - the post form's tag select;
  - an LMS course page with the Lessons and Enrollments sections.

  RBlog shows post ↔ tag on real data.
- **Linked anchors**: `#belongs-to-many-pivot-tables` (reference/admin-api.md, release-notes.md),
  `#choosing-links-in-the-records-form` (forms.md, resources.md, reference/admin-api.md, release-notes.md),
  `#reacting-to-link-changes` (resources.md, release-notes.md).

---

## Features in this area with no guide page (or only a passing mention)

- **Record (view) page**: layout, history tab (`abilities.history`, operations.rs:142), slots, the actions header and
  its phone layout (CHANGELOG 0.10.3). It is spread over actions, customizing and audit-log; there is no page about
  the view page as such.
- **Trash UI** (`trashed=with|only`, restore and force delete buttons): only in resources.md#soft-deletes, nothing
  about the list toggle.
- **List query API** (`filter[field][op]`, `sort`, `search`, `page`, `per_page`, limits): not in tables.md. Check
  reference/admin-api.md.
- **`make:resource --generate` / `--belongs-to` generator rules**: not described anywhere except the CLI reference.
- **`Ctx` as an API** (`can, conn, t, t_in, keyring, dispatch, audit, records, record, page_path`;
  engine/context.rs): pieces appear in queues, audit-log, i18n and customizing, but there is no single reference.
- **Panel commands** `idempotency:prune`, `admin:branding`: CLI reference only.
- **Form layouts** (Split, Section collapsible/compact, FormWidth): already covered in forms.md#an-editor-layout.
  `Layout::columns` is not.
- **Queue and Schedules pages**: covered in digging-deeper/queues.md#the-queue-page and
  scheduling.md#the-schedules-page. Overview links them; no admin page needed.
- **Static select options are presentation only** (not checked on resource writes): not stated anywhere. This is a
  security-relevant gap given "deny by default".
