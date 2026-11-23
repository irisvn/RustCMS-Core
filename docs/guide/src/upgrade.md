# Upgrade guide

## Upgrading the command and an application

1. The command: `rustcms self-update` replaces it with the latest release
   (`--version v0.4.0` for a given one, older included). It works when the
   [install script](getting-started/installation.md#installing-the-rustcms-command)
   installed it; a command built with `cargo install` is updated the same way
   it was installed. `rustcms --version` shows the version and the platform.
2. An application: in its directory, `rustcms upgrade` moves it to the
   command's version:

   ```sh
   rustcms upgrade --dry-run   # what changes in Cargo.toml
   rustcms upgrade             # writes it, then updates Cargo.lock
   rustcms migrate
   cargo test
   ```

   It sets the tag of `rustcms` and `rustcms-testing` (always the same), the
   `sea-orm` pins of that version and `[package.metadata.rustcms] version`,
   keeping your comments and features. If Cargo cannot resolve the result,
   `Cargo.toml` and `Cargo.lock` are put back. It refuses to go back to an
   older version (unless `--force`), and leaves alone a dependency on a local
   `path`, inherited from a workspace, renamed, or from another repository:
   change those by hand. Then follow the section of that version below, and
   commit `Cargo.toml` and `Cargo.lock` together.

   To upgrade to another version than the command's, install that command
   first (`rustcms self-update --version vX.Y.Z`).

## From 0.13.1 to 0.14.0

Run `rustcms migrate`: framework migration `m20261202_000023_add_collection_to_media` adds a nullable
`media.collection` column and an index. **Rollback:** its `down` drops both; 0.13.1 also runs with the column
in place (it ignores it), so rolling back the binary alone is safe.

- **Table columns show option labels.** A text or badge column whose field has a select or checkbox list with
  options in the form shows the option's label instead of the stored value. Add `.raw()` to the column to keep
  the values. Search, sorting, filters and exports still use the values.
- **`.color(..)` values are compared as the API sends them.** On an `i64` or decimal column write
  `.color("2", ..)` (a number such as `.color(2, ..)` is read the same way). A value the column cannot hold (a
  string on an `i32` or boolean column, a number on a text column) now stops the boot with a message naming
  it: such colors never matched before.
- **Colors.** Inputs, checkboxes and outline buttons use the new `input` token instead of `border`: a theme
  that set `border` for them sets `input` too. Primary badges are solid, a row's primary action is an outline
  button, dark mode's main buttons are light grey, and the light `muted-foreground` is a little darker
  (`#63636c`) so muted text reaches 4.5:1 on selected rows. A theme or branding whose text colors fall under
  4.5:1 is named in a warning in the log; nothing is refused. See [theming](admin/theming.md).
- **Uploads.** An upload may carry a `collection` part. A part other than `file` and `collection` is still
  refused with code `file_part`, with a new message; an undeclared collection gets the new code
  `media_collection`.
- **Rust API.** `media::entity::Model` gains `collection`: code that builds a media `Model` or `ActiveModel`
  with a struct literal adds `collection: None` (or `Set(None)`). `MediaEntry::gallery()` is deprecated and
  does nothing (every image opens in the viewer; `.preview(false)` keeps a field's images still).
- **Schema (vocabularies 20 to 23).** `ColumnSchema` gains `options` and `format`, `EntrySchema` `format` and
  `preview` (its `gallery` is gone: code building an `EntrySchema` drops it), `MediaSchema` `preview`,
  `InputSchema` `collection`, `FilterSchema` `none`, `MediaItem` `collection`, the panel's
  configuration `dateFormat`/`dateTimeFormat` and `PanelResponse` `mediaCollections`. A client on an older
  vocabulary shows forms read-only.
- **Extensions.** The SDK's `Badge` takes `color` (`"gray"`, `"primary"`, `"info"`, `"success"`, `"warning"`,
  `"danger"`, any other string shows gray); copy the SDK again (`rustcms make:sdk`) to get the type.

Repeaters gain conditions, filtered relations, a table layout, rows on view pages and files in rows (see
[forms](admin/forms.md#repeaters) and [relationships](admin/relationships.md#filtered-relations)). They need no
migration. **Rollback:** application code using the new builders goes back with the binary; files written into
JSON rows stay in their columns. What changes for existing applications:

- **Relationship repeaters hold fewer rows by default.** The new setting
  [`[admin] repeater_rows`](getting-started/configuration.md#admin-configadmintoml) (default 50, 1 to 500)
  bounds the rows of one parent a relationship repeater reads and reorders, and the rows one save adds,
  changes or removes in it (422 `too_many_rows` beyond). 0.13 read up to 100 rows (or `max_items`), took up to
  100 entries per part of a change and reordered up to 1,000 rows. A parent with more rows than the setting
  shows its repeater read-only (edit the rows in the relation's section): applications with larger repeaters
  raise it. Relation lists (`GET …/relations/{relation}`) now take `per_page` up to that setting when it is
  above the resource's page size.
- **The default view page shows relationship repeaters.** A resource without `infolist()` now lists a
  relationship repeater's rows under the record (a `RepeatableEntry` of the relation, its rows' inputs as
  entries) instead of nothing. Declare an `infolist()` to choose otherwise.
- **Rows keep the child form's conditions.** Rows of a relationship repeater whose `.schema()` declares no
  condition are checked with the child form's conditions, as in 0.13. Declaring a condition in the rows
  (`shown_when`, `when_root`) makes the rows follow the repeater's conditions instead of the child form's.
- **Filtered relations** (`Relation::where_null`, `where_eq`) are new. `where_eq` takes a boolean, integer,
  UUID or date field only (others stop the boot). A user who may not read every filtered field does not get
  the relation (its routes answer 404, a repeater change for it 403); one who may not write them on create
  cannot create through it (403). A hook that removes or changes a filtered value of a record created or
  updated through the relation is a 500.
- **Hooks of repeater rows.** A row's `after_save` runs before the save's file check, which comes last: an
  unavailable file then rolls the whole save back. Do outside work through a job dispatched in the
  transaction (`ctx.dispatch(txn, …)` or `queue::dispatch(txn, …)`), never `queue::enqueue(db, …)` nor a
  direct call (see [hooks after a write](admin/resources.md#hooks-after-a-write)). A child's hook that adds a
  file to a row no longer fails with a 500: the file is checked with the parent's.
- **Files per save.** One save refers to at most 2,000 distinct files (fields, rich text and rows together;
  422 `too_many_media`, a new code).
- **Rust API.**
  - The leaves of `Condition` (`Eq`, `Ne`, `IsIn`, `Filled`, `IsTrue`) gain a `scope` member. The builders
    (`when`, `Condition::all`, `Condition::any`) are unchanged; code that builds a variant by hand sets
    `scope: Scope::Row`, and code matching a variant by all its fields adds `..`.
  - `ListQuery` gains a hidden `relation_filters` field, set by the engine: code that builds it with
    `..Default::default()` is unaffected, a struct literal listing every field adds it.
  - `DynResource` (implemented by the engine only) changes: `counted_by` takes the relation's filters
    before the parents (`&[]` for none), `field_locks_media_or_secret` is `field_is_encrypted`, and it gains
    methods. Only code that calls these is affected.
- **Schema (vocabularies 24 and 25).** Leaf conditions gain `scope` (`"root"`, `"parent"`; absent: the same
  level), relationship repeaters may carry `shownWhen`, `RepeaterSchema` gains `layout` (`"table"`),
  `RepeaterRelationship` `rowLimit`, `RelationSchema` `fixed`, `EntrySchema` `rows`, `InputSchema`
  `toolbar` (`"compact"`), and file uploads and rich editors may appear in rows. A client on an older
  vocabulary shows forms read-only.

Selects and checkbox lists may compute their options on the server
([`options_fn`](admin/forms.md#options-computed-on-the-server)), and reactions may read and change the rows
of relationship repeaters ([reactions on child records](admin/forms.md#reactions-on-child-records)). Both are
opt-in and need no migration; nothing changes for a form that does not use them. **Rollback:** code using
`options_fn`, `.reacts()` on a relationship repeater or the new patches goes back with the binary. Otherwise:

- **Idempotent update retries are answered earlier.** An update carrying an idempotency key
  (`Ctx::with_idempotency_key`, import rows) whose first run committed now answers with the record before its
  input is checked, as creates do: a retry of a committed write no longer fails on a rule that changed
  meanwhile, and runs no `options_fn`.
- **New answers on saves with `options_fn`.** A save may fail with the form error `options_failed` (the
  options could not be computed) or 409 `stale` (reason `options`: a dependency changed before the record was
  locked; send it again). Imports retry such rows.
- **Rust API.**
  - `FormPatch` gains `AddRelationRow`, `UpdateRelationRow`, `RemoveRelationRow` and `FlagRelationRow`. It is
    not `#[non_exhaustive]`: an exhaustive `match` on it needs arms for them (or `_`).
  - `SelectOption`, `RowId` and `StateRow` are exported at the crate root (`rustcms::admin::…`); a glob import
    that brings in another item of the same name needs a path.
  - `DynResource` (implemented by the engine only) gains methods (`form_options`, `computes_options`, and
    hidden ones for row reactions). Only code that implements it is affected.
- **Schema (vocabularies 26 and 27).** `InputSchema` gains `optionsFrom` (`{dependsOn}`), with a new route
  `form/options`; relationship repeaters may carry `reacts`, `form/react`'s state carries their rows, and its
  answer may hold the four row patches. A client on an older vocabulary shows forms read-only.

## From 0.13.0 to 0.13.1

No migration. To roll back, install 0.13.0 again; no data changes shape.

- **Default view page.** A resource without `infolist()` is shown by an
  infolist made from its form instead of the read-only form: values look as
  before, a `belongs_to` field's title links to the related record when the
  user may list it, and an encrypted field no longer shows whether it is set.
  A declared `infolist()` is unchanged.
- **Schema (vocabulary 19).** `InfolistSchema` gains `fromForm`; a view
  schema without `key` no longer carries `infolist` (a client drawing a
  record's view page passes `key`); `InputSchema` and rich text entries gain
  `richNodes`, inputs gain `reacts` (form reactions, a new route
  `form/react`).
- **Rich text version 3** (underline, superscript, subscript, application
  elements). Documents are stored in the lowest version they need, so 0.13.0
  still reads every document without a version 3 part. **Rollback:** 0.13.0
  refuses version 3 documents on save and shows them read-only.
- **Rust API.** `Mark` is `#[non_exhaustive]` and gains `Underline`,
  `Superscript`, `Subscript`; `Node` gains `App`: an exhaustive `match`
  needs an arm (or `_`). `rich_text::parse` refuses `app.*` nodes: read
  documents holding them with `Registry::rich_nodes()` (or
  `AppNodes::parse`).
- **Extensions: SDK `ui` 4.** Bundles built for `ui` 3 keep working. To draw
  elements, copy the new SDK (`rustcms make:sdk`), set `Requires { ui: 4 }`
  and rebuild.

## From 0.12.1 to 0.13.0

No migration. To roll back, install 0.12.1 again; no data changes shape.

- **SQLite 3.54 (security, RUSTSEC-2026-0459).** No sqlx release bundles it
  yet, and a `[patch.crates-io]` in RustCMS does not reach an application:
  add the section below to the `Cargo.toml` at your workspace's root (the
  application's own, or the parent workspace's for an application inside
  one), then run `cargo update -p libsqlite3-sys`. `rustcms new` writes it
  for new applications, except with `--in-workspace`: add it to the parent
  workspace's root. A later release says when to remove it.

  ```toml
  [patch.crates-io]
  sqlx = { git = "https://github.com/launchbadge/sqlx", rev = "10462fa4414c6c7f1ed1d39bb31678e3b726878c" }
  sqlx-core = { git = "https://github.com/launchbadge/sqlx", rev = "10462fa4414c6c7f1ed1d39bb31678e3b726878c" }
  sqlx-macros = { git = "https://github.com/launchbadge/sqlx", rev = "10462fa4414c6c7f1ed1d39bb31678e3b726878c" }
  sqlx-macros-core = { git = "https://github.com/launchbadge/sqlx", rev = "10462fa4414c6c7f1ed1d39bb31678e3b726878c" }
  sqlx-mysql = { git = "https://github.com/launchbadge/sqlx", rev = "10462fa4414c6c7f1ed1d39bb31678e3b726878c" }
  sqlx-postgres = { git = "https://github.com/launchbadge/sqlx", rev = "10462fa4414c6c7f1ed1d39bb31678e3b726878c" }
  sqlx-sqlite = { git = "https://github.com/launchbadge/sqlx", rev = "10462fa4414c6c7f1ed1d39bb31678e3b726878c" }
  ```

  Only a process that opens a database file (with its `-wal`) written by a
  less trusted party is exposed; PostgreSQL and MySQL applications are not.

- **New schema vocabulary.** The panel and the server move together (the
  admin panel is embedded): nothing to do, unless an application builds
  `InputSchema` or `LayoutSchema` by hand (new members `number`,
  `shownWhen`, `clearWhenHidden`).
- **Relationship select search.** Each word of a search must match one of the
  title's fields: with `title("name")`, a search of several words no longer
  matches only that exact phrase. Titles with an empty value are left out of
  `titles()`.
- **Extensions: SDK `ui` 3.** Bundles built with `ui: 2` keep working. To use
  `registerEntry` or the SDK's `Infolist`, copy the new SDK
  (`rustcms make:sdk`, which points at the registrations to move), set
  `Requires { ui: 3 }` and rebuild (`npm run build`).
- **Rust API.** New variants: `Component::Entry` (forms hold entries in
  infolists), `ComponentSchema::Entry`, `PageView::Infolist`. An exhaustive
  `match` on one of these enums needs an arm for it (or `_`). Code
  implementing `DynResource` itself (not through `Resource`) adds `computed`
  and `field_locks_media_or_secret`; struct literals of `RepeaterSchema` add
  `create_components`.
- **`lock_parent` cannot target the media library** (boot refuses it): media
  rows are locked after every other record of a write.
- **Name clash.** `rustcms_admin::Condition` (form conditions) can clash with
  `sea_orm::Condition` when an application glob-imports both
  (`use rustcms_admin::*; use sea_orm::*;`): import the one you use by name.

## From 0.12.0 to 0.12.1

No migration. To roll back, install 0.12.0 again.

- Only new applications change. To format an existing one as new ones are,
  copy [`rustfmt.toml`](https://github.com/irisvn/RustCMS-Core/blob/v0.12.1/rustfmt.toml)
  to its root and run `cargo fmt`; generators find their markers wherever
  they sit in the builder chains.

## From 0.11.0 to 0.12.0

No migration. To roll back, install 0.11.0 again; no data changes shape.

- **Behaviour change: the media and exports cleanups need the scheduler.**
  They are now framework schedules, `rustcms.media-prune.<disk>` (every
  minute) and `rustcms.exports-prune.<disk>` (every 10 minutes), run only by
  `serve` with `[schedule] enabled`, once per slot across instances. They no
  longer run in `queue:work` or console commands and take no Redis lease. If
  no instance runs `serve` with the scheduler, cron `media:prune` and
  `schedule:run rustcms.exports-prune.local` (or `.s3`).
- Temporary files an instance staged (uploads, exports, imports) are removed
  by that instance after an hour: every `serve` and `queue:work`, every 10
  minutes. `media::prune`, `exports::prune` and `imports::prune` no longer
  sweep them; call `rustcms::storage::sweep_staging` if your code relied on
  it.
- New `[app] mode` (`full` by default, nothing changes). In `api` mode
  (`serve --api-only`), `queue.enabled` and `schedule.enabled` left out are
  `false` ([API mode](digging-deeper/instances.md#api-mode)).
- New opt-in settings `auth.permission_generation_cache_ms` and
  `auth.jwt.session_cache_ms`, both `0` (unchanged behaviour).
- `MediaResource` now implements `after_trash` and `after_restore` (for
  media hooks); nothing changes for applications.

## From 0.10.3 to 0.11.0

No migration. Read the first points before upgrading: they can make a
request a 422 or stop the boot. To roll back, install 0.10.3 again; no data
changes shape.


- `rustcms docs` opens the guide of your version online
  (`https://irisvn.github.io/RustCMS/v/X.Y.Z/`); the command no longer
  carries an offline copy. The guide's sources are Markdown files in
  RustCMS-Core (`docs/guide/src`).
- The hourly auth cleanup (expired sessions, ended JWT sign-ins, reset and
  invitation links, old audit entries) and the idempotency cleanup are now
  framework schedules, `rustcms.auth-prune` and `rustcms.idempotency-prune`:
  only `serve` runs them, with `[schedule] enabled`, on the hour (UTC), once
  across instances. They no longer run in `queue:work`, console commands or
  tests, and take no Redis lease. If no instance runs `serve` with the
  scheduler, cron `schedule:run rustcms.auth-prune` (or `session:prune`,
  `audit:prune`) and `idempotency:prune`. They appear on System → Schedules
  and in `schedule:list`; holders of `schedules.manage` can Run them now.
  Application schedules still cannot use the `rustcms.` prefix. No migration.
- **Behaviour change: static options are validated.** A client write to a
  resource (create, update, a relation create, a CSV import row) whose value
  is not one of a `SelectInput`'s or `CheckboxList`'s `.options(…)` is now a
  422 (`not_option`; a checkbox list also `invalid` when not a list and
  `given_twice` for a repeated item); it was stored before. Records already
  holding other values keep them until the field is written. Clients and
  imports sending values outside the options must send an option, or the
  options must list the value. Hooks and action handlers are not affected.
- New boot failures (the application refuses to start): a select option that
  is not a value of its column (`12` for an `i64` column, write `"12"`) or that
  would be stored as another value (`"10.555"` on a column of scale 2, more
  digits than a decimal's precision, `0.1` on an `f32` column, a timestamp
  finer than microseconds); options on a `SecretInput` or `SecretMapInput`; a
  `CheckboxList` on a column that is not JSON; a field in several inputs that
  do not offer the same options (another set, a select and a checkbox list,
  or one of them without options). See
  [options](admin/forms.md#options-are-not-validated). The schema now shows
  select options in canonical form (`"0012"` → `"12"`), and a matched select
  value is stored as its option (`"10.505"` → `"10.50"`). No migration.
- A background export over its limits now fails with the reason
  `too_many_to_export` or `too_many_matches` instead of `revoked`. A client
  of `GET {admin}/api/exports` that maps reasons should add both (unknown
  reasons should already show as a generic failure).
- The panel's sockets refuse to send messages larger than `[realtime]
  max_message_bytes` (it was a fixed 64 KiB): with a lower value, pages that
  sent up to 64 KiB now get `false` from `send` for larger messages. Keep the
  value the same on every instance; open tabs keep the limit they loaded until
  reloaded.
- Export and import pruning only cleans its own disk (`[storage] disk`).
  After a change of disk, expired exports and imports of the old disk stay
  (rows and files) until a process configured with the old disk boots and
  prunes them, or you delete them yourself (they are no longer listed nor
  downloadable once expired). No migration.

## From 0.10.2 to 0.10.3

- Nothing to change. On phones a record page's Edit and Delete are in its
  ⋯ menu: end-to-end tests at phone widths open the menu first.

## From 0.10.1 to 0.10.2

- Vocabulary 11: layouts `split` and `pane`, `collapsed` and `compact` on
  sections, `prefix` and `suffix` on text inputs, `FormSchema.width`. An older
  admin SPA shows the banner and lays such forms out as plain groups;
  extensions that check the vocabulary version should accept 11. Exhaustive
  matches on `LayoutType` and clients that build `LayoutSchema`,
  `InputSchema` or `FormSchema` values by hand need the new members.
- The default theme's `success` and `warning` colors are green and amber
  (they were grays): set them with `Theme::color` to keep the old look.
- No migration.

## From 0.10.0 to 0.10.1

- **Run `rustcms migrate`** (`m20261201_000022_add_alt_to_media`, a nullable
  column): `serve` does not wait for it, and media queries fail until it
  has run. 0.10.0 binaries read the migrated database: to roll back, run
  them with `migrate --allow-newer` (or `[database] allow_newer_migrations`);
  leave the column in place (`migrate:rollback` would undo the application's
  own latest migrations first).
- **`media.update` now grants something**: renaming one's own files and
  editing their alternative text (before, updates were always refused); with
  `media.manage_all`, everyone's files. Review roles that hold `media.*` as a
  whole.
- The navigation and the dashboard's resource cards follow registration
  order, not slugs: use `ResourceMeta::sort` to keep an order.
- Every `.bulk()` action is also offered on rows where the record's
  abilities allow it; an action offered only for a selection is not
  supported (`visible_when` also applies when the action runs, so hiding
  it on every row would leave a bulk run nothing to process).
- Action dialogs no longer say "Run": their button is the action's label
  without "…" (or `submit_label`); empty values read "Not set". Update
  end-to-end selectors.
- Vocabulary 10: `ActionSchema.primary` and `submitLabel`,
  `ColumnSchema.hideBelow`; per-record `abilities` include `visible_when`.
  An older admin SPA shows the banner and every offered action as a button;
  extensions that check the vocabulary version should accept 10.
- Struct literals: `ActionOutcome` has `message` and `open` (add
  `..Default::default()`); `media::entity::Model` / `ActiveModel` have `alt`.
  Clients that build wire types by hand gain `MediaItem.alt`,
  `RecentColumn.valueType`, `options` and `column`, and `RecentRecord.data`.
- `Action::new` is unchanged; handlers that return an outcome use
  `Action::with_outcome`.

## From 0.9.0 to 0.10.0

- Rich text documents have a version 2 (tables, embeds, callouts, image
  captions and widths). Stored version 1 documents stay valid and are read
  as they are, and the panel keeps writing version 1 unless a document uses
  a version 2 part. Nothing to migrate. To rewrite stored documents anyway,
  map each value through `rustcms::admin::rich_text::upgrade_value` (only
  `version` changes).
- **Rolling back to 0.9, or a 0.9 reader of the same database** (another
  service, a replica during a rolling deploy): documents saved with a
  version 2 part (table, embed, callout, image caption or width) are refused
  by 0.9's `rich_text::parse`, and the 0.9 panel shows them read-only. Deploy
  every reader first; before rolling back, remove those parts or keep 0.10.
  A headless consumer that parses documents itself should accept
  `"version": 2` and the new nodes (see [Rich text](admin/media.md#rich-text)).
- `rich_text::Node` is `#[non_exhaustive]` and has new variants (`Table`,
  `Embed`, `Callout`); `Node::Image` now holds an `Image { media, alt,
  caption, width }`; `Document` has a `version` field. Exhaustive matches,
  patterns on `Node::Image { media, alt }` and struct literals need updating.
- `rich_text::to_html` keeps its signature but writes images as
  `<figure><img …></figure>` and headings with an `id`. Use
  `to_html_with(&doc, &HashMap<Uuid, ImageInfo>)` to write image sizes, and
  `outline` for a table of contents.
- `PanelResponse` has an optional `uploads` field (the upload limits);
  admin clients that build `PanelResponse` values by hand need it.

## From 0.8.2 to 0.9.0

- New permission `media.view_all` (run `permission:sync` to list it). Roles
  that held `media.manage_all` only to see other users' files can hold
  `media.view_all` instead.
- `InputType` has a `BelongsToMany` variant, `InputSchema` a `links` field and
  `DynResource` a `link_options_new` method (exhaustive matches, struct
  literals and hand-written implementations need them). Admin clients see
  `vocabularyVersion` 9: an older admin SPA shows these inputs read-only and
  never sends them, and an extension
  that checks the vocabulary version should accept 9.
- `Resource::after_relation_change` is new: a link write never updates the
  parent row, so touch `updated_at` there if your application needs it.
- Restores now call `after_restore`, whose default calls `after_save`:
  nothing changes unless you override it (then call `Self::after_save` from
  it if restores should still run your `after_save`).

## From 0.8.1 to 0.8.2

Nothing needs to change (no rebuild of React pages either). A workaround that
hid `session_changed` from reads may stay or go.

## From 0.8.0 to 0.8.1

Nothing needs to change. To survive automatic rollbacks, keep migrations
additive and start the image with `migrate --allow-newer`. Deploy that image
before adding migrations: a rollback only accepts newer migrations when the
image it returns to is 0.8.1 or later with the flag (a 0.8.0 image still
refuses). Run migrations once per deployment, not in every replica
([Deployment](getting-started/deployment.md#upgrading)).

## From 0.7.1 to 0.8.0

Run `rustcms migrate`: it adds `jobs.started_at`, `jobs.description` and the
`job_runs` and `schedule_runs` tables. Then:

- Grant `schedules.view` (and `schedules.manage` for Run now) to the roles
  that should see System → Schedules (`rustcms permission:sync` declares them).
- `.run` schedules keep working; switch to `.task` to show a summary.
- Job kinds starting with `rustcms.` are now refused (they are the
  framework's).
- The Queue's *Scheduled* status is labelled *Delayed* (the API value stays
  `scheduled`).

## From 0.7.0 to 0.7.1

Nothing needs to change. React pages' dialogs get the close button without a
rebuild; `rustcms make:sdk` adds `closeButton` and `closeDisabled` to the
SDK's types.

## From 0.6.1 to 0.7.0

Most applications need no change; update links of your own to theme files,
and `PermissionInfo` struct literals, if you have any.
- The Roles page now groups permissions by area: resource matrix cells under
  "Resources", the built-in users, roles and permissions permissions under
  "System", the rest under "Other" until you declare areas
  (`permission_area`, `permission_meta`).
- Theme files moved from `{admin}/assets/ext/` to `{admin}/assets/brand/`
  (`theme.css`, `logo`, `favicon`). Change any link of your own to the old
  paths; the panel's pages need nothing.
- The sign-in pages now show your `brand(…)` name and logo instead of
  "rustcms".
- `PermissionInfo` (from `GET {admin}/api/system/permissions`) has new fields;
  code that builds it with a struct literal must add them.

## From 0.6.0 to 0.6.1

Nothing needs to change. Users can now pick Light, Dark or System; your
`Theme::color_scheme` is the default until they do. To use `useColorScheme()`
in a React page, run `rustcms make:sdk`, then `npm run build` in
`resources/admin`.

## From 0.5.1 to 0.6.0

Nothing needs to change: React pages built with `ui: 1` keep working.

To use `props.resources`, `props.toast` and the new components:
1. Run `rustcms make:sdk`: it updates `resources/admin/sdk` (keeping files
   you edited) and lists the registrations to change.
2. Set `ui: 2` in those `Requires { … }`. A bundle built with the new SDK
   is refused while its provider still declares `ui: 1`.
3. Run `npm install && npm run build` in `resources/admin`, then commit.

## From 0.5.0 to 0.5.1

Nothing needs to change. `PanelResponse` (`home`), `ResourceInfo` (`sort`,
`nav`) and `PageLink` (`sort`) have new fields: code that builds these
structs with a literal must add them (they are normally only read).

## From 0.4.1 to 0.5.0

Nothing needs to change: v1 scripts (`extension_script`,
`window.rustcms.registerPage` with `apiVersion: 1`) work as before.

- New, optional: [React pages](admin/react-pages.md)
  (`rustcms make:page <Name> --react`, `extension_script_v2`). They need Node
  20.19+ or 22.12+ to build, not to compile or deploy the app.
- `GET {admin}/api/panel` has a new `permissions` list; a test that compares
  the whole response should expect it, and Rust code building a
  `PanelResponse` literal must add `permissions`.
- `{admin}/api/pages/{slug}/{*path}` is new: a page now answers its
  sub-paths too (with the page's permissions; read `ctx.page_path()` in
  `data` to tell them apart).

## From 0.4.0 to 0.4.1

- `rustcms migrate`: two new tables, `schedule_states` and `schedule_slots`.
- Periodic work can move from your own `tokio::spawn` loops to
  [schedules](digging-deeper/scheduling.md), which run once per slot across
  instances and only in `serve`.
- `rustcms::settings::{read, load, write}` take the keyring
  (`state.keyring()`) instead of `&state.config().app.key`.
  `settings::get(&state)` is unchanged.
- New, optional: `APP_PREVIOUS_KEYS` (see
  [Rotating APP_KEY](operations/app-key.md)), and
  [encrypted columns](digging-deeper/encryption.md).
- The admin vocabulary is 8 (`secret_map` inputs). An extension that checks
  the vocabulary version should accept 8.

## From 0.3.2 to 0.4.0

`rustcms self-update`, then `rustcms upgrade` in each application. Nothing
needs to change in your code.

- New `[realtime]` keys, all optional: `socket_revalidate_secs`,
  `max_message_bytes`, `send_timeout_secs`
  ([Configuration](getting-started/configuration.md)). The other keys already
  existed.
- The admin page's CSP now also lists `ws(s)://` for the panel's origins in
  `connect-src`. A test that compares the header with
  `rustcms::admin::spa::CONTENT_SECURITY_POLICY` should use
  `content_security_policy(&origins)` instead.
- `{admin}/api/x` belongs to your `AdminRoutes`: nothing built in lives
  there.
- Behind a proxy, if you add WebSocket routes, see the
  [production checklist](security/production.md).

## From 0.3.1 to 0.3.2

Only the command changed (new `doctor`, `upgrade`, `self-update`, `docs`).
Install it again with the install script (0.3.1 has no `self-update`), then
`rustcms upgrade` in each application.

## Applications made before 0.3.1

They depend on `rustcms = "0.3.0"` (not on crates.io) or on a local `path`.
`rustcms upgrade` moves a version dependency to the Git tag; a `path` is
changed by hand, keeping your features:

```toml
[dependencies]
rustcms = { git = "https://github.com/irisvn/RustCMS-Core", tag = "v0.3.1" }

[dev-dependencies]
rustcms-testing = { git = "https://github.com/irisvn/RustCMS-Core", tag = "v0.3.1" }
```

## From 0.1.0-alpha.0 (or a 0.2 build) to 0.3.0

1. Update the `rustcms` dependency, then run `rustcms migrate` (new tables for
   media, JWT sign-ins, idempotency keys, locks, the audit log, settings,
   jobs, and `users.locale`). `serve` now also runs queue workers (set
   `queue.enabled = false` to keep them out of web instances).
2. Rust API changes:
   - API routes: move them to `ApiRoutes` with an explicit `Access` per route
     ([Routing](basics/routing.md)); register them with
     `AuthProvider::new().api_permissions(&api)` and `api.register(&mut app)`.
   - `RateLimiter::reserve`, `release` and `clear` are async (`.await` them).
   - `Principal` and `rustcms::models::user::Model` have new fields
     (`locale`, `credential`): struct literals need them, or `..Default::default()`.
   - `AppError::Conflict` has a new `reason` field.
   - `SettingsAbilities` has a new `test` field; `SettingsPage::validate_in`
     (with the application) runs `validate` by default.
   - `rustcms::models::user::Model` has `invitation_pending`, `invited_at`,
     `invited_by` (migrations `…000018`, `…000019`): struct literals need them
     (`..Default::default()` on `ActiveModel`s is unaffected). `rustcms-auth`
     now enables the core `mail` feature.
   - Mail: register `rustcms::mail::MailProvider` (and
     `AdminProvider::mail_settings()` for the page); in production with
     `driver = "smtp"` and `source = "settings"`, set `[mail] allowed_hosts`.
   - Functions that change credentials record an audit entry and take one more
     argument: `credentials::change_password` and `credentials::logout_everywhere`
     an `&AuditContext`; `credentials::issue_token` and
     `jwt_session::{issue, rotate, logout}` a `&RequestMeta` (both in
     `rustcms::audit`; `RequestMeta` is also an extractor).
   - `InputType` has a `Secret` variant and `AdminPanel` a `settings` field
     (exhaustive matches and struct literals need them); admin clients see
     `vocabularyVersion` 5.
   - Resource slugs are limited to 64 bytes and action names to 57, and
     `user`, `role`, `permission`, `settings`, `session` (and `job`, `queue`,
     see below) are reserved (boot fails); `Ctx::new` records writes as its principal.
   - `ActorKind` has a `Job` variant (exhaustive matches need it).
   - `AdminPanel` has a `custom` field (`CustomPanel::empty(base)` in hand-built
     panels), `PanelResponse` a `pages` field and `ColumnSchema` `cell` /
     `props`; admin clients see `vocabularyVersion` 6.
   - `SystemLinks` has `audit` and `queue` fields; resource schemas gain the
     `history` ability.
   - Resource slugs `job` and `queue` are reserved too (subjects of the
     Queue page's audit entries).
   - Actions: `data` sent to an action without a form is refused (422), and
     the request body may only hold `keys` and `data`. Declare what an action
     needs with `Action::form` ([Actions](admin/actions.md#actions-with-a-form)).
     Actions are admitted before the body is read: a policy deciding per record
     should override `Policy::admits` for abilities that need a permission.
     Actions the policy does not admit are no longer listed in the schema.
   - Run `rustcms migrate` (new `exports` and `imports` tables). `ResourceSchema.abilities`
     may hold `export`; `DynResource` has new methods (implementations outside
     the crate are not expected).
   - `Relation` has new fields and `RelationSchema` a `kind`; `ActionSchema`
     a `form`; `ListQuery` a hidden `through` field (struct literals need
     `..Default::default()`). Admin clients see `schemaVersion` 2 and
     `vocabularyVersion` 7: an older admin SPA turns read-only.
   - Tests that build tables from entities instead of running migrations also
     need the `audit_logs` table ([Testing](basics/testing.md)).
3. Optional: translate your labels. Add `lang/vi.toml`, register it with
   `AdminProvider::new().translations("vi", include_str!("../lang/vi.toml"))`,
   and fill in what `rustcms lang:missing vi` lists.
4. API clients: error bodies gain `issues` and `formIssues` (and `reason` on
   409); existing fields are unchanged.
5. Several instances: see [Several instances](digging-deeper/instances.md).

## Application layout (0.3)

Applications are laid out like Laravel applications, inside `src/`
([Directory structure](getting-started/directory-structure.md)). Generators
refuse an application in the old layout; move it once:

| Before | Now |
|---|---|
| `src/app.rs` (`build`) | `src/bootstrap/mod.rs` (`build`) + `src/bootstrap/providers.rs` |
| providers set up in `build` | `src/providers/{auth_service_provider,admin_panel_provider,app_service_provider}.rs` |
| `src/migrations/` | `src/database/migrations/` (keep the file names: they are recorded in `seaql_migrations`) |
| `src/admin/post.rs` | `src/admin/resources/post_resource.rs` |
| `src/http/api/mod.rs` (`routes`) | `src/routes/api.rs` |
| `src/http/api/posts/handlers.rs` | `src/http/controllers/post_controller.rs` |
| `src/http/api/posts/requests.rs` | `src/http/requests/post_request.rs` |
| `src/http/api/posts/json.rs` | `src/http/resources/post_resource.rs` |
| a settings struct with its page | `src/settings/<x>_settings.rs` + `src/admin/pages/<x>_settings_page.rs` |
| a `<app>:seed` command | `src/database/seeders/` + `rustcms db:seed` |
| `tests/*.rs` | `tests/feature/*.rs` listed in `tests/feature/main.rs`, helpers in `tests/support/mod.rs` |
| `assets/admin/<vendor>/*.js`, `*.css` | `resources/admin/js/<vendor>/`, `resources/admin/css/<vendor>/` (update the `include_str!` paths) |

Then:

1. `src/main.rs`: `Console::<my_app::database::migrations::Migrator>::new(my_app::bootstrap::build)`.
2. Add the generator anchors where generated lines go: `// rustcms:mods`
   (`src/lib.rs`), `// rustcms:resources` and `// rustcms:pages`
   (admin panel provider), `// rustcms:seeders`, `// rustcms:jobs`,
   `// rustcms:commands` (app service provider), `// rustcms:api-routes`
   (`src/routes/api.rs`), `// rustcms:tests` (`tests/feature/main.rs`),
   `// rustcms:extensions` (admin panel provider, for `make:page --script`). A new
   application from `rustcms new` shows every one.
3. `rustcms migrate:status` lists every migration as applied: nothing to run.

