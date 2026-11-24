# Release notes

RustCMS is in pre-release. The current version is **0.14.1**; 0.1.0-alpha.0
was the first published one.

## 0.14.1

A fix and the guide's pictures; nothing to change in an application
([upgrade guide](upgrade.md#from-0140-to-0141)).

- **Image viewer**: the counter ("1 / 6") no longer covers the image's title.
- **Guide**: screenshots from the demo for every 0.14.0 feature, and the
  demo's other screenshots taken again with 0.14.0's colors.

## 0.14.0

Read the [upgrade guide](upgrade.md#from-0131-to-0140) first: a framework
migration (`rustcms migrate`), schema vocabularies 20 to 27, a few Rust API
changes, and `[admin] repeater_rows` (50 by default) for relationship
repeaters with more rows. Most features of this release were contributed by
@giangittb112000 and reworked after review. It also takes security fixes of
`lettre` and `url`.

- **Option labels and date formats in tables and view pages**: columns show a
  select's labels; `.date_format("dd/MM/yyyy HH:mm")` per column or entry, or
  for the whole panel ([Tables](admin/tables.md#option-labels)).
- **Image viewer**: a click on an image opens it over the page, with zoom and
  the place's other images ([Media](admin/media.md#the-image-viewer)).
- **Colors and dark mode**: every text pair reaches WCAG AA; new theme tokens;
  themes and brandings under 4.5:1 are named in a warning
  ([Theming](admin/theming.md#light-and-dark)).
- **Media collections**: sort the library into declared collections, upload
  into one from a field ([Media](admin/media.md#collections)).
- **Repeaters, second round**: conditions reading the record's form
  (`when_root`), filtered relations (`where_eq`, `where_null`), a table
  layout, relationship rows on view pages, files, rich text and record
  references in rows; a hidden relationship repeater keeps its rows
  ([Forms](admin/forms.md#repeaters),
  [Relationships](admin/relationships.md#filtered-relations)).
- **Options computed on the server**: `options_fn` for selects whose choices
  depend on other inputs or data, checked again on save
  ([Forms](admin/forms.md#options-computed-on-the-server)).
- **Reactions on child records**: a form's reaction can add, change, remove
  or flag a relationship repeater's rows
  ([Forms](admin/forms.md#reactions-on-child-records)).

## 0.13.1

Read the [upgrade guide](upgrade.md#from-0130-to-0131) first: new schema
vocabulary (19), SDK `ui` 4, a few Rust API additions. No migration. The
three features of this release were contributed by @giangittb112000 and
reworked after review.

- **Default view page**: a resource without `infolist()` shows its records
  with entries made from its form (relation titles as links, encrypted fields
  never shown); conditions read fields as the edit form does
  ([Infolists](admin/infolists.md#the-default-view-page)).
- **Rich text version 3**: underline, superscript and subscript, and your
  own elements in documents (`RichTextNode` atoms with typed attributes,
  accepted per field, drawn by an extension or a placeholder); documents are
  stored in the lowest version they need, so older readers keep working
  ([Media and rich text](admin/media.md#application-elements)).
- **Form reactions**: `.reacts(…)` runs on the server as the user types and
  answers with changes to the form (a slug that follows the name, totals of
  rows), reading only what the user's form holds
  ([Forms](admin/forms.md#reacting-to-changes)).

## 0.13.0

Read the [upgrade guide](upgrade.md#from-0121-to-0130) first: new schema
vocabulary, a few Rust API additions, and `lock_parent` may not target the
media library. No migration. Most of this release was contributed by
@giangittb112000.

- **Number bounds**: `NumberInput::make(…).min(n).max(n).step(n)` (and on
  integer settings), checked by the server on every write
  ([Forms](admin/forms.md#number-bounds)).
- **Inputs shown under conditions**: `.shown_when(when("x").eq(v))` on
  inputs, layouts, tabs, settings and action forms, evaluated live in the
  panel and again by the server on what the user's form holds;
  `.clear_when_hidden()` ([Forms](admin/forms.md#showing-inputs-under-conditions)).
- **Titles from several fields**: `ResourceMeta::title_template("{name}
  (grade {grade})")`, searched word by word ([Resources](admin/resources.md)).
- **Infolists**: `infolist()` shows a record's view page with entries (text,
  badges, relations, media, rich text, custom entries), `computed()` values
  computed when the page opens, `PageView::Infolist` for panel pages; SDK
  `ui` 3 ([Infolists](admin/infolists.md)).
- **Repeaters**: `Repeater::make("faq").schema(…)` stores rows in a JSON
  column; with `.relationship()`, the rows are the child records of a
  has-many relation, edited in the parent's form (the child locks its
  parent); count columns `count()` / `counts("posts")`; row-level history
  ([Forms](admin/forms.md#repeaters), [Tables](admin/tables.md#counts)).
- **`rustcms serve --watch`** (macOS, Linux): rebuilds on changes and
  replaces the server once the build succeeds; a failed build keeps the
  running server
  ([Installation](getting-started/installation.md#rebuilding-on-changes)).
- A create leaving out a column that may not be null answers `is required`
  (422) instead of a server error; actions that move records under
  `lock_parent` lock the destination parent.
- **Security**: bundled SQLite 3.54.1 (RUSTSEC-2026-0459) through a patched
  sqlx; existing applications add the patch
  ([upgrade guide](upgrade.md#from-0121-to-0130)). ammonia 4.2.5.

## 0.12.1

- A new application (`rustcms new`) passes the `cargo fmt --check` of its own
  CI: it gets a `rustfmt.toml`, whatever its name
  ([Directory structure](getting-started/directory-structure.md)).

## 0.12.0

Read the [upgrade guide](upgrade.md#from-0110-to-0120) first: the media and
exports cleanups now need the scheduler. No migration.

- **API mode**: `rustcms serve --api-only` (or `[app] mode = "api"`) serves
  the application API alone, without the admin panel, to scale it out behind
  a load balancer; the same build runs the panel instance
  ([API mode](digging-deeper/instances.md#api-mode)).
- **Faster API tokens**: a personal access token is checked with one
  statement, and its last use is written at most once a minute. Two opt-in
  settings let API instances reuse the permission counter and JWT sign-ins
  for a short time ([Credential revocation](security/revocation.md#several-instances)).
- **Media hooks**: `AdminProvider::media_hook` runs your code in the
  transaction of every upload, edit, trash, restore and delete of a media
  file ([Media hooks](admin/media.md#media-hooks)).
- **Cleanups are schedules**: the media and exports cleanups run as
  `rustcms.media-prune.<disk>` and `rustcms.exports-prune.<disk>`; every
  `serve` and `queue:work` removes the temporary files it left
  ([Data retention](operations/retention.md)).
- Abandoned export and import files on an S3 disk's staging directory are
  removed too.

## 0.11.0

Read the [upgrade guide](upgrade.md#from-0103-to-0110) first: static select and
checkbox options are now enforced, and some form declarations stop the boot.
No migration.

- **A new guide**: rewritten from the source, with screenshots and samples
  from compiled examples, on VitePress (search, dark mode, phones). `rustcms
  docs` opens the online guide of your version.
- **Options are validated**: a client write outside a `SelectInput`'s or
  `CheckboxList`'s options is a 422; a matched value is stored as its option
  ([Options are validated](admin/forms.md#options-are-not-validated)).
- **Framework cleanups are schedules**: the auth and idempotency cleanups run
  in `serve` as `rustcms.auth-prune` and `rustcms.idempotency-prune`, shown on
  System → Schedules ([Framework schedules](digging-deeper/scheduling.md#framework-schedules)).
- A panic in a handler answers the standard 500 instead of dropping the
  connection ([Errors](basics/errors.md)).
- A background export over its limits says why (`too_many_to_export`,
  `too_many_matches`); the export list no longer fails on MySQL with large
  exports; export and import cleanups only touch their own disk
  ([Import and export](admin/import-export.md)).
- The panel's sockets send up to `[realtime] max_message_bytes`
  ([Realtime](admin/realtime.md)).

## 0.10.3

- Record pages on a phone keep the primary action and ⋯ in the header (the
  other actions, Edit and Delete under ⋯); a viewed text input shows its
  prefix and suffix ([Actions](admin/actions.md#placement-in-lists)).

## 0.10.2

- **An editor layout**: `Form::width(FormWidth::Full)`, a main column with a
  sticky aside (`Split`), sections that fold or are compact, and text inputs
  with a fixed prefix or suffix (`/posts/` before a slug)
  ([An editor layout](admin/forms.md#an-editor-layout)).
- Badge columns show all five colors; the default theme's `success` and
  `warning` are green and amber. The media picker keeps a page chosen right
  after opening.

## 0.10.1

Run `rustcms migrate`; `media.update` now grants renaming and alt text (see
the [upgrade guide](upgrade.md#from-0100-to-0101)).

- **Media page**: a grid of thumbnails or the table (remembered per user)
  and an upload button for several files ([The Media page](admin/media.md#the-media-page));
  files can be renamed and given an alternative text, which the editor and
  the picker pre-fill ([Names and alternative text](admin/media.md#names-and-alternative-text)).
- **Media settings**: `SettingField::media` and `media_list`
  ([Media settings](admin/settings.md#media-settings)).
- **Actions per record**: `Action::visible_when`, `primary` and
  `destructive`; list rows show one labelled action and a ⋯ menu, and record
  pages and relation tables show their actions too
  ([Per-record visibility](admin/actions.md#per-record-visibility)).
- **Action outcomes**: `Action::with_outcome` returns a message and a page
  to open in a new tab ([Outcomes](admin/actions.md#outcomes-a-message-a-page-to-open)).
- **Narrow screens**: `TableColumn::hide_below(Breakpoint::Md)`
  ([Narrow screens](admin/tables.md#narrow-screens)).
- Fixes: bulk actions are offered on rows too; action dialogs are titled
  without "…" and their button is the action's label (`submit_label`);
  record pages format dates and times; the recent-records widget shows
  labels and dates; files that cannot be loaded show "File unavailable";
  empty values read "Not set"; navigation keeps the registration order.

## 0.10.0

- **Rich text version 2**: tables, YouTube/Vimeo embeds, callouts, image
  captions and widths; version 1 documents are read as they are
  ([Rich text](admin/media.md#rich-text)).
- **Editor**: several images uploaded at once (choose, drop, paste) with
  per-file status and retry; a media library picker shared with
  `FileUpload` fields ([The media library picker](admin/media.md#the-media-library-picker)).
- **Headless HTML**: images as figures with their size
  (`rich_text::to_html_with`), stable heading ids with Vietnamese diacritics
  folded, and `rich_text::outline` for a table of contents
  ([Headless rendering](admin/media.md#headless-rendering)).

## 0.9.0

- **`Resource::after_trash` and `after_restore`** run in the transaction of a
  trashing and of a restore, with the record; the default `after_restore`
  calls `after_save`, as restores did ([Resources](admin/resources.md#hooks)).
- **`ActionField::datetime`**: a date and time in an action's form, entered
  in UTC ([Actions](admin/actions.md)).
- **`SettingsPage::after_save`** runs in the save's transaction with the old
  and new values ([Settings](admin/settings.md#reacting-to-a-save)).
- **`media.view_all`**: see and use every file of the media library without
  the right to remove other users' files ([Media](admin/media.md#whose-files-a-user-sees)).
- **`Resource::after_relation_change`** runs when links of a belongs-to-many
  relation change, in the same transaction
  ([Relationships](admin/relationships.md#reacting-to-link-changes)).
- **Links in the record's form**: `BelongsToManySelect::make("tags")` lets
  users choose belongs-to-many links while creating or editing a record,
  with the relation manager's permissions
  ([Relationships](admin/relationships.md#choosing-links-in-the-records-form)).
- Rich text links are edited in a dialog; record forms ask before leaving
  with unsaved changes ([Forms](admin/forms.md#leaving-with-unsaved-changes)).
- Security: `tokio` 1.53.3 and `ctutils` 0.4.4 (RUSTSEC-2026-0386, 0385).
  Applications pinning their own `Cargo.lock` should run
  `cargo update -p tokio -p ctutils`.

## 0.8.2

- React pages: a read still in flight when a sub-path navigation remounts
  the view is delivered to the new view instead of a "session changed"
  error ([React pages](admin/react-pages.md)).
- Security: updated TLS certificate verification crates (`rustls-webpki`,
  `webpki-roots`) for RUSTSEC-2026-0381, -0382 and -0383.

## 0.8.1

- **Rollbacks after migrations**: `migrate --allow-newer` (or
  `[database] allow_newer_migrations = true`) lets an older binary start on a
  database a newer one migrated, when migrations are additive; `migrate:status`
  shows them as `Newer`; `rustcms::db::unknown_migrations` lists them
  ([Deployment](getting-started/deployment.md#upgrading)).

## 0.8.0

- **Job history on the Queue page**: finished jobs (done, failed, cancelled)
  with their times and duration, a details dialog, filters by time, auto
  refresh; `Job::describe()` names a job ("deploy demo/web #12"); the
  *Scheduled* status is now *Delayed* ([Queues](digging-deeper/queues.md#the-queue-page)).
- **System → Schedules**: every schedule, its next and last run and its
  history, and **Run now** for job and closure schedules
  ([Scheduling](digging-deeper/scheduling.md#the-schedules-page)).
- **`Resource::after_delete`** runs in the transaction of a hard delete, for
  example to dispatch a job that removes what the record configured
  ([Resources](admin/resources.md)).
- **`.task(…)` schedules** report a summary or a safe error; `.timeout(…)`
  bounds a closure's run; `Job::AT_MOST_ONCE` for jobs that must never run
  twice.

## 0.7.1

- **Dialogs have a close button** (×) in their top right corner, also the
  `Dialog` of React pages; forms in dialogs cannot be closed while saving.

## 0.7.0

- **Roles page**: permissions by area (collapsible, with counts), a search
  box, labels and descriptions, and presets ("Viewer", "Developer") as
  starting selections, declared on the `AuthProvider` with
  `permission_area`, `permission_meta` and `permission_preset`
  ([Roles & permissions](security/authorization.md#the-roles-page-areas-labels-and-presets)).
- **Runtime branding**: `AdminProvider::branding(source)` sets the brand
  name, logos (light and dark), favicon and theme tokens while the app runs,
  cached per instance and invalidated at once; `[admin] branding = "static"`
  recovers ([Runtime branding](admin/customizing.md#runtime-branding)).
- The sign-in, password reset and invitation pages show the brand name and
  logo; the brand names the browser tab.

## 0.6.1

- **Light, Dark or System per user**: a switch at the bottom of the sidebar,
  kept in the browser; the app's color scheme stays the default
  ([Theme](admin/customizing.md#theme)).
- React pages: `useColorScheme()` to redraw canvas charts in the current
  scheme ([React pages](admin/react-pages.md#calling-your-routes)).

## 0.6.0

- **Resources from React pages**: `props.resources` and the hooks
  `useResource` and `useResourceMutation` read and write your resources
  through the panel's routes. Policies, validation, audit and encrypted
  fields apply; a lost answer pauses writes safely, as in the panel
  ([React pages](admin/react-pages.md#records-of-your-resources)).
- **Toasts**: `props.toast.success` and `props.toast.error`.
- **Page titles**: `props.title(text)` names a sub-page in the panel's
  heading and breadcrumb (`null` hides the heading).
- **More components**: Tabs, DropdownMenu, Tooltip, Table, Skeleton,
  Separator and CodeBlock, CSP-safe and usable inside dialogs.
- **More icons** for resources and pages.
- **Any-of page permissions**: `PanelPageMeta::permission_any([…])`.
- `ui: 1` React pages keep working; set `ui: 2` to use the new parts.

## 0.5.1

- **Home page**: `AdminProvider::home(slug)` opens one of your pages at
  `/admin` ([Navigation](admin/overview.md#home-page)).
- **Order**: `.sort(n)` on resources and pages orders the sidebar.
- **Hidden resources**: `ResourceMeta::navigation(false)` keeps a resource's
  routes and API without a sidebar entry.

## 0.5.0

- **React pages**: a page, slot or cell can be a React component rendered
  inside the panel, on its React, React Query, UI kit, theme and router
  (`extension_script_v2`, `rustcms make:page <Name> --react`).
  - Props carry the page data, the sub-path, `can(permission)`, an API
    client with the session and CSRF token, and realtime streams; hooks
    `useAdminQuery`, `useAdminMutation`, `useEvents`, `useSocket`.
  - A bundle needing another React or React Query major, a newer minor
    than the panel's, or a higher UI level is refused before it runs; a view that throws shows Retry; the CSP is unchanged.
  - Built files are committed, so building the app needs no Node
    ([React pages](admin/react-pages.md)).
- **Sub-pages**: `/admin/p/<slug>/<sub/path>` opens the same page, with
  `ctx.page_path()` in `data`.
- Plain script extensions (v1) work as before.

## 0.4.1

- **Encrypted columns**: `rustcms::crypto::Encrypted<T>` keeps secrets of your
  own tables (tokens, environment variables, credentials) sealed with
  `APP_KEY`, bound to their table and column.
  - Values are never serialized, logged or deserialized from JSON; you
    `seal` and `open` them explicitly
    ([Encrypted columns](digging-deeper/encryption.md)).
- **Key rotation**: `APP_PREVIOUS_KEYS` keeps old keys for opening only, and
  `rustcms key:reencrypt` moves every encrypted column and settings secret
  to the new key, safely and resumably
  ([Rotating APP_KEY](operations/app-key.md)).
- **Encrypted fields in the panel**: `Field::encrypted()` (a secret string)
  and `Field::encrypted_map()` (named secrets).
  - Write-only: the panel shows names and states, never values.
  - Each value is kept, replaced or removed; a save over someone else's
    change is refused.
  - Always redacted in the audit log
    ([Forms](admin/forms.md#encrypted-fields)).
- **Scheduling**: `AppBuilder::schedule` runs periodic work every N seconds
  or on a cron (UTC), as queue jobs or in the process.
  - Each slot runs once across all instances, with or without Redis, and
    `.without_overlapping()` is available.
  - Only `serve` runs schedules; `schedule:list` and `schedule:run`
    ([Scheduling](digging-deeper/scheduling.md)).

## 0.4.0

- **Admin routes**: JSON routes for your own panel pages under
  `{admin}/api/x`, on the admin session, each with a declared
  `AdminAccess` checked before extractors (`AdminRoutes`,
  `AuthProvider::admin_permissions`); `assert_access` tests every route
  against a table of actors ([Admin routes](admin/admin-routes.md)).
- **Server-sent events**: `AdminRoutes::events` streams live data to a
  page.
  - It is rechecked while open: the session, the user, the permissions and a
    per-target check.
  - Limits are per user and per instance, slow clients are dropped, and
    streams end cleanly on shutdown.
  - A process-local `Hub` shares one producer between viewers, with `resync`
    when a viewer falls behind.
  - Script pages read streams with `ctx.realtime.events`
    ([Realtime](admin/realtime.md)).
- **WebSocket**: `AdminRoutes::socket` for terminals and consoles.
  - Admission: the exact `Origin` and the session's CSRF token as a
    subprotocol.
  - A supervised, rechecked socket with bounded queues both ways.
  - `AdminSocket::authorize()` checks a command when it runs.
  - Script pages use `ctx.realtime.socket`
    ([Realtime](admin/realtime.md#websocket)).
- **Content Security Policy**: unchanged, except that `connect-src` also
  names the panel's own `ws(s)://` origins.
- **Secrets from files**: every variable also reads `<VAR>_FILE` (Docker
  and Kubernetes secrets) ([Configuration](getting-started/configuration.md)).
- **Testing**: `TestApp::new` takes a closure, so tests can bind doubles.
  `RUSTCMS_TEST_BACKENDS` picks the databases, and `TestClient::socket`
  opens WebSocket connections ([Testing](basics/testing.md)).
- **Tooling**: the test servers run in memory (the matrix is about twice as
  fast), and `just test-fast` runs every test on SQLite only.

## 0.3.2

- The command checks and updates itself and applications: `rustcms doctor`,
  `rustcms upgrade [--dry-run]` (Core tag, SeaORM pins; comments and
  features kept), `rustcms self-update`, and `rustcms docs` (the guide,
  offline in release builds) ([Upgrading](upgrade.md#upgrading-the-command-and-an-application)).
- `make:command` refuses the names of the command's own commands.

## 0.3.1

- **Distribution**: each tag publishes the `rustcms` command (macOS on Apple
  silicon, Linux x86_64 and arm64), the guide and `install.sh`
  ([Installation](getting-started/installation.md)).
- `rustcms new` depends on the RustCMS-Core Git tag of its version, writes a
  CI workflow (`.github/workflows/ci.yml`) and resolves dependencies
  (`--no-fetch` to skip).
- Fixes: local storage on Linux (every upload, import and export failed with
  `Bad file descriptor`); migrations run on one connection; a refused TLS
  handshake is reported as `smtp_tls`; security updates of ammonia and
  rustls.

## 0.3.0

- **Admin panel**: soft deletes with a trash; tabs and key/value inputs;
  media library, file uploads and rich text documents; has-many relation
  managers; custom field widgets from extension scripts; idempotent creates,
  actions and uploads ("retry safely").
- **Languages**: the panel in English and Vietnamese, chosen per user; your
  labels translated from `lang/<locale>.toml` (`AdminProvider::translations`,
  `rustcms lang:missing`); validation errors carry stable codes
  ([i18n](admin/i18n.md), [error codes](reference/error-codes.md)).
- **Audit log**: every admin write, upload, role and user change and API
  token recorded with its actor, credential and request, in the write's
  transaction; sign-ins, sign-outs and JWT events recorded best effort;
  redacted by declaration; `audit:prune` and hourly retention; shown to
  auditors in the panel, and as each record's history
  ([Audit log](digging-deeper/audit-log.md)).
- **Settings**: groups of settings as Rust structs, edited in the panel
  (`settings.<group>.view` / `.update`), read with `rustcms::settings::get`;
  secrets encrypted and never sent to the browser
  ([Settings](admin/settings.md)).
- **Queues**: jobs stored in the database, dispatched in the transaction of
  the change, run by workers in `serve` or `queue:work` with leases,
  retries, timeouts and graceful shutdown; `queue:failed` / `retry` /
  `forget`; a **Queue page** in the panel (System → Queue: jobs by status,
  retry, forget, cancel; `queue.view` / `queue.manage`)
  ([Queues](digging-deeper/queues.md)).
- **Application layout**: Laravel's folders inside `src/` (bootstrap,
  providers, routes, http controllers/requests/resources, services, jobs,
  settings, admin resources/pages, database migrations/seeders/factories);
  seeders and `db:seed`, factories, generators for each
  ([Directory structure](getting-started/directory-structure.md); moving an
  existing app: [upgrade guide](upgrade.md#application-layout-03)).
- **Parent locks**: `ResourceMeta::lock_parent` for rules over a parent's
  children ([Resources](admin/resources.md#parent-locks)).
- **Customizing the panel**: a theme (color tokens, radius, fonts, logo,
  favicon, light/dark), custom pages and slots beside records from server
  templates (escaped, then sanitized) or extension scripts, custom table
  cells, `make:page`; `view!("file.html")` embeds a template and reads it
  again at each request in local development ([Customizing](admin/customizing.md)).
- **Belongs-to-many relations**: `Relation::belongs_to_many` through a pivot
  table, with pivot columns; attach, detach and edit links on a record's page,
  each with its own permission ([Relationships](admin/relationships.md#belongs-to-many-pivot-tables)).
- **Actions with a form**: `Action::form(vec![ActionField::…])` asks for values
  in a dialog, checked on the server ([Actions](admin/actions.md#actions-with-a-form)).
- **CSV import and export**: `ResourceMeta::exportable()` (the list as shown,
  readable fields only, protected against spreadsheet formulas; large lists
  in the background, checked again at download) and `importable()` /
  `import_key()` (upload, check, import through the usual write paths, a
  receipt per row) ([Import and export](admin/import-export.md)).
- **Mail**: SMTP (the server set in Settings → Mail or the configuration,
  within the operator's `allowed_hosts`/`allowed_ports`, TLS required),
  templates per language, mails queued with the change (`send_later`) and
  retried on temporary failures, a test-mail button, `log` and `memory`
  drivers; failed queue jobs can fail at once (`Failure::permanent`)
  ([Mail](digging-deeper/mail.md)).
- **Password resets, invitations, remember me**: "Forgot password?" mails a
  one-time link (the same answer whether the account exists); administrators
  invite users who choose their own password; sign-in can be remembered for
  `remember_days` ([Authentication](security/authentication.md)).
- **Dashboard widgets**: counts (compared with the period before), charts per
  day, week or month, and the latest records of a resource, each with its own
  permission and computed as the list shows records ([Dashboard](admin/dashboard.md)).
- **Example**: `examples/lms`, a small LMS back office: seats under a lock,
  instructor scopes, a job per enrollment, news (categories, tags, scheduled
  articles), editor and viewer roles, lessons moved up and down, a dashboard,
  welcome mails and instructor invitations ([An LMS back office](examples/lms.md)).
- **Operations**: running the queue, retention, backup and restore,
  rotating `APP_KEY`, running mail ([Operations](operations/queue.md));
  several instances with Redis (shared rate limits, periodic jobs mostly on
  one instance); S3-compatible storage tested against a real server,
  Cloudflare R2 setup, bounded bucket timeouts
  ([File storage](digging-deeper/storage.md)).
- **Permission matrix**: the role page lays resource permissions out as
  resource × ability ([Users and roles](admin/users-roles.md)).
- **Databases**: MySQL 8.4 alongside SQLite and PostgreSQL.
- **APIs**: `ApiRoutes` with declared access, `Validated<T>`, `Pagination`,
  `Throttle<N>`, `make:api`; JWT access tokens with rotating refresh tokens.

## 0.1.0-alpha.0

- Resources rendered as an admin panel: forms, tables, filters, actions,
  `belongs_to` relationships, field and record authorization, hooks.
- Admin sessions with CSRF and origin checks; API tokens with abilities;
  roles and permissions per guard; users, roles and account pages.
- Application layer: configuration, providers, container, HTTP stack,
  migrations, console and generators, testing kit.

See the [upgrade guide](upgrade.md) when moving between versions.
