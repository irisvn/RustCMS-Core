# Changelog

All notable changes are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.10.1] — 2026-10-24

### Added (0.10.1)
- The Media page shows the library as a grid of thumbnails (the
  authorized file URL in an `<img>`, scaled by the browser; other files as
  an icon) or as the usual table, a choice remembered per user in the
  browser; the grid shows at most 24 thumbnails a page. Its **Upload
  files** button sends several files through the panel's upload queue
  (`useUploads`: at most 2 at a time, a 429 sent again after `Retry-After`,
  an idempotency key per file, the same on a retry). The media schema has
  an `upload` ability (`media.create`); `create` stays refused. Images
  inserted from the library take the file's alternative text; a trashed or
  missing file shows "File unavailable" on its page and in the editor.
- Media names and alternative text: a nullable `media.alt` column
  (framework migration `m20261201_000022_add_alt_to_media`, re-runnable on
  MySQL), and the media record's edit form changes exactly `name` (a rename,
  cleaned like uploaded names, never empty) and `alt` (at most 500
  characters). `MediaPolicy` allows `update` with `media.update` on one's own
  files, and on others' only with `media.manage_all` (like trashing); every
  other column stays refused. `alt` is read with the record and returned in
  `MediaItem` (`alt?`). The record page shows the file and what was detected.
- `SettingField::media(name)` and `SettingField::media_list(name, max)`
  (with `.accept(…)`, `.max_size(…)`): settings holding media ids
  (`Option<Uuid>`, `Vec<Uuid>`), edited with the media input. A save checks
  only the files it adds, as resource media fields are checked (exists, not
  trashed, visible to the saver now, accepted type and size; rows locked
  in the save's transaction); files the setting already held are kept
  unchecked, whatever the order or removals. Boot refuses them without
  `AdminProvider::media()` or with a non-empty default. Contract fixture
  `settings.brand.json`.
- `Action::visible_when(|ctx, record| …)`: whether an action is offered on
  a record, decided by the server for each record (envelope `abilities`) and
  again on the locked record when the action runs: a run on one record it is
  hidden on is refused (403); a run on several skips those records (reported
  in `denied`, never handed to the handler). Oracle case.
  `Action::primary()` (schema `ActionSchema.primary`) and
  `Action::destructive()` (color `danger`): a list row shows at most one
  labelled action (the first primary one it offers) and a ⋯ menu with the
  others, destructive ones last after a separator. Vocabulary 10.
- Action outcomes: `Action::with_outcome(name, handler)` (a new
  constructor; `Action::new` is unchanged) takes a handler returning
  `AppResult<ActionOutcome>`; `ActionOutcome` gains `message` and `open`
  (`ActionOutcome::message`, `ActionOutcome::open`, `with_message`,
  `with_open`). `open` must be `https://` (no credentials) or an absolute
  path of this site (no `//`, dot segments, backslashes; at most 2,048
  bytes), or the action fails (500) and rolls back; it is refused on runs on
  several records (422 `one_record_only`). The message is at most 500
  characters. The panel shows the message and, after the commit, opens the
  page in a new tab (`noopener,noreferrer`), with a link as fallback.
- `TableColumn::hide_below(Breakpoint::Sm | Md | Lg)` (a closed enum,
  `rustcms::admin::Breakpoint`; schema `ColumnSchema.hideBelow`,
  vocabulary 10): the column is hidden on narrower screens (640, 768,
  1024 px) by static CSS classes (lists and relation tables); records are
  unchanged.
- `Action::submit_label(label)` (schema `ActionSchema.submitLabel`): the
  button of the action's form dialog; by default the action's label without
  a trailing ellipsis (it was always "Run"). Action dialog titles drop a
  trailing ellipsis too.

### Fixed (0.10.1)
- Bulk actions were never offered on list rows, although `.bulk()` means
  "also offered for selections": every action is now a row action where the
  row's abilities allow it.
- Record pages and relation tables showed no actions: a record's page shows
  the actions it offers (primary labelled, the others under ⋯), and the rows
  of relation tables those of the related resource.
- Record pages showed dates and times as wire text
  (`2026-10-24T02:00:45.857073Z`): they are formatted in the user's
  language, instants in the browser's time zone with its name (the exact
  value stays in the `<time datetime>` attribute).
- `Widget::recent` showed raw values (`draft`, ISO instants): each column
  now carries its `valueType`, its form input's `options` and its table
  `column`, and each record its wire `data`, so the panel shows option
  labels, custom cells, badges and formatted dates (the `values` text stays).
- A rich text image whose file is missing or trashed rendered as a broken
  image: it shows the "File unavailable" chip, as file fields now do.
- Generated text: a `null` value on a record's page reads "Not set" (no
  "(null)"); relation search boxes have a placeholder.
- The media input reported its pending upload again on every render of its
  form (a new `onBusy` callback each time), re-rendering the form without
  end while a file was being uploaded.

### Changed (0.10.1)
- `ActionOutcome` has two new fields, `message` and `open` (code building
  it with a struct literal adds `..Default::default()`).
- The navigation (and the dashboard's resource cards) lists resources in
  registration order instead of by slug; `ResourceMeta::sort` still
  reorders them.
- `media::entity::Model` has a new field `alt: Option<String>` (code that
  builds `media::ActiveModel` literals sets it).

## [0.10.0] — 2026-10-24

### Added (0.10.0)
- Rich text document version 2 (ADR 0008 amendment): `table` (a header
  row, rows of cells holding paragraphs, rectangular, at most 50 × 20),
  `embed` (`{provider, id}` for YouTube and Vimeo, strict per-provider ids,
  never a URL or HTML; `Embed::from_url`), `callout` (`info`/`warn`,
  paragraphs and lists, no table or callout inside), and image `caption` and `width`
  (`normal | wide | full`). `rich_text::parse` reads versions 1 and 2 (v2
  parts refused in v1); `rich_text::upgrade_value(&Value) -> Value`; the
  editor writes the lowest version a document needs (v1 without v2 parts,
  so 0.9 still reads it);
  `Document::version`; the limits `MAX_NODES`, `MAX_DEPTH`, `MAX_IMAGES`,
  `MAX_EMBEDS`, `MAX_ALT`, `MAX_CAPTION`, `MAX_TABLE_ROWS`,
  `MAX_TABLE_COLUMNS`, `MAX_HREF` are public; issue codes
  `document_needs_version`, `document_caption`, `document_table_rows`,
  `document_table_columns`, `document_embed`, `max_embeds`.
- `rich_text::to_html_with(&doc, &HashMap<Uuid, ImageInfo>)`: images as
  `<figure><img src width height alt><figcaption>`, sizes from the
  map (`ImageInfo { url, width, height }`, `ImageInfo::new(url).size(w, h)`).
- Stable heading ids in `to_html` (diacritics folded to ASCII, Vietnamese included;
  `-2`, `-3` for repeats) and `rich_text::outline(&doc) -> Vec<Heading { level, text, id }>`,
  `rich_text::slug`.
- Editor: multi-image upload (picker multi-select, drop, paste of files) with
  per-file status, error and retry (same idempotency key), at most 2 at a
  time per list (never more than `max_concurrent_uploads`), a 429 retried
  by itself after `Retry-After` (up to 3 times); image details (alt text, caption,
  width); tables; embeds from YouTube/Vimeo addresses (a text card, no
  iframe, no thumbnail); callouts; Heading 4 and horizontal rule buttons; saves the lowest version
  the document needs.
- Media library picker, shared by the editor and `FileUpload` /
  `media_list` inputs: thumbnails, search, pages, upload tab; the media
  scope, `accept` and `max_size` apply; English and Vietnamese.
- `PanelResponse.uploads` (`PanelUploads { maxBytes, maxConcurrent }`) for
  users who may upload.
- E2E `rich-text.spec.ts` (the editor's v2 nodes, multi-image upload, the
  media library picker; CSP watcher).

### Changed (0.10.0)
- `rich_text::to_html` writes images as figures and headings with ids (same
  signature). `rich_text::Node` is `#[non_exhaustive]`; `Node::Image` holds
  an `Image` struct; new variants `Table`, `Embed`, `Callout`.
- Pasted HTML images other than the panel's own media are dropped when
  pasted (they were dropped on save).

## [0.9.0] — 2026-10-24

### Added (0.9.0)
- `Resource::after_trash` and `Resource::after_restore`: run in the
  transaction of a trashing and of a restore (soft deletes), after the write,
  with the record as stored; an error rolls the write back. Restores call
  `after_restore`, whose default calls `after_save` (so restores behave as
  before; an override replaces that call). `after_force_delete` (unchanged)
  is documented with them. Core has no bulk trash, restore or force delete:
  bulk actions write through their handlers.
- `ActionField::datetime`: an instant in an action's form, edited and
  labelled in UTC like the form's `DateTimePicker` of an instant column;
  strict RFC 3339 on the wire (ADR 0003), canonical UTC
  (`2026-10-12T02:30:00Z`) in the handler's `data`.
- `SettingsPage::after_save(state, ctx, txn, before, after)`: runs in the
  save's transaction after the row and its audit entry are written, with the
  group as it was and as saved (typed); an error rolls the save back, and
  the secrets of either version are redacted from its message (validation,
  bad request or internal). Not called when a save changes nothing.
- `media.view_all` (`rustcms::admin::media::VIEW_ALL`): every file is
  listed, downloaded and accepted as a reference (media fields, rich text),
  but trashing, restoring and deleting another user's file still needs
  `media.manage_all` (`MediaPolicy` decides per file; 403). Oracle actors
  `uploader` and `librarian`.
- `Resource::after_relation_change(ctx, txn, record, &RelationChange)`:
  runs in the transaction of a belongs-to-many write that changed links
  (attach, detach, pivot update), once per relation and write, with the
  parent record and the changed target keys (`attached`, `detached`,
  `pivot_updated`); an error rolls the write back. The parent row is still
  not updated by link writes (`updated_at` included).
- `BelongsToManySelect::make(relation)` (ADR 0035): belongs-to-many links
  chosen in the record's own create and edit form. The payload sends a
  change (`{attach?, detach?}` of target keys), never a set, so others' and
  hidden links are kept; `attach_{rel}` / `detach_{rel}` are admitted before
  the keys are read and decided on the record as stored by the write; the
  targets are locked through `lock_visible`; pivot rows,
  `after_relation_change`, then `after_save` in the record's transaction;
  audit `attached` / `detached` after `created` / `updated`. Targets are
  locked before the record's row is written (its foreign keys never upgrade
  a shared lock), per target resource in the registry's lock order
  (`Registry::lock_rank`, deterministic), together with the rows of those
  resources the row's own `belongs_to` keys reference
  (`DynResource::lock_visible_with`, with a default; a `belongs_to` and links
  into one resource never cross); at most 100 keys per `attach` or
  `detach` and 500 per target resource and save; refused at boot for the
  media library and for relations with required pivot fields. Route
  `GET resources/{slug}/relations/{relation}/options` (create forms; only
  relations the form links, 404 otherwise), oracle cases. Vocabulary 9: `InputType::BelongsToMany`, `InputSchema.links`
  (`LinksSchema {attach, detach}`); `DynResource::link_options_new`. The SPA
  shows the current links as chips (from the relation's list, at most 100),
  a search to add more (`RelationSelect` gains `exclude` and `onPick`), and
  invalidates the record's relation lists after a save; without the search
  the input itself carries the field's id, and focus never falls to the page
  when a chip is removed.

- Record forms ask before leaving with unsaved changes: the panel's dialog
  for in-app navigation (TanStack Router `useBlocker`), the browser's prompt
  for closing or reloading; not after a save, while one is in flight or
  uncertain, or when the session sends the tab elsewhere
  (`lib/leave.ts`).

### Changed (0.9.0)
- Rich text: the link button opens a dialog (the panel's `Dialog`,
  translated; checked like the server; edits or removes the link at the
  cursor; inserts the address as linked text when nothing is selected)
  instead of `window.prompt`. The client's link check refuses C1 controls
  (U+0080–U+009F) like the server's.
- LMS example: editors and viewers hold `media.view_all` instead of
  `media.manage_all` (they only view the news' images).
- Demo: posts choose their tags in the form too (`BelongsToManySelect`);
  E2E `form-links.spec.ts` (form links, the link dialog, the leave dialog).

### Fixed (0.9.0)
- The panel's `Dialog` passed a new ref callback on every render: each
  commit detached and attached it again, setting state, so a dialog that
  stays mounted (closed) in a busy form rendered until React gave up
  (error #185). Its ref is stable now.

### Security (0.9.0)
- `ctutils` 0.4.4 (RUSTSEC-2026-0385: non-constant-time mask generation and
  `u128` comparisons) and `tokio` 1.53.3 (RUSTSEC-2026-0386: `AsyncFd::new`
  violates I/O safety), the smallest fixed releases; younger than the usual
  two weeks, by exception.

## [0.8.2] — 2026-10-21

### Fixed (0.8.2)
- React pages (ADR 0032): a read (`props.api.get`, `props.resources` reads)
  started by a live mount and answered after a same-session remount (a
  sub-path navigation) resolves instead of rejecting with `session_changed`,
  so the query cache shared with the new view no longer shows that error.
  Reads started after the mount ended, and reads across a session change,
  still reject; writes are unchanged.

### Security (0.8.2)
- `rustls-webpki` 0.103.16, `webpki-roots` and `webpki-root-certs` 1.0.10
  (RUSTSEC-2026-0381, -0382, -0383: distrusted intermediates). An exception
  to the two-week rule for dependency updates, approved for these patches.

## [0.8.1] — 2026-10-20

### Added (0.8.1)
- `migrate --allow-newer` and `[database] allow_newer_migrations`: migrations
  applied by a newer binary (after a rollback) are logged and left as they
  are, and this binary's pending ones still run (through a tolerant migrator
  with a stand-in per unknown migration). Without it, `migrate` refuses with a
  message naming them. `migrate:status` lists them as `Newer`.
- `rustcms::db::{unknown_migrations, migrate_up, migration_status,
  MigrationSettings}`.

## [0.8.0] — 2026-10-20

### Added (0.8.0)
- Job history (ADR 0016): `job_runs`, one row per terminal outcome (done,
  failed, cancelled), written in the transaction of the transition, only
  when it changed the job (settle, reap and cancel are transactional);
  `jobs.started_at`, `jobs.description` (`Job::describe`, ≤ 200 characters),
  `[queue] history_retention_days` (7; 0 = none), pruned with failed jobs.
- `Job::AT_MOST_ONCE`: a run stopped by a shutdown fails instead of being
  given back.
- Queue page: Jobs and History tabs, a details dialog, time filters, the last
  24 hours' counts, auto refresh (paused after "Load more"); routes
  `GET queue/history`, `GET queue/jobs/{id}`, `since`/`until` on
  `GET queue/jobs`. The *Scheduled* status reads *Delayed*.
- Schedules (ADR 0031): `schedule_runs` (scheduled, skipped and manual runs,
  their outcome, a task's summary, a safe error), `Schedule::task`,
  `Schedule::timeout`, `TaskCtx`, public read API (`describe`, `next_after`,
  `action`, `states`, `runs`), `schedule::run_now` (in the caller's
  transaction, on the schedule's state row lock), the internal job
  `rustcms.schedule.run` (at most once), `[schedule] history_retention_days`.
- `Resource::after_delete`: runs in the transaction of a hard delete (resources
  without soft deletes), after the delete; an error rolls it back.
- System → Schedules: `GET schedules`, `GET schedules/{name}/runs`,
  `POST schedules/{name}/run` (audited `schedule.run`); permissions
  `schedules.view`, `schedules.manage`; `SystemLinks.schedules`.

### Changed (0.8.0)
- A slot skipped by `without_overlapping` is committed (with a `skipped`
  run): no other instance runs it later.
- `.run` errors are logged as before and recorded as "failed (see the logs)".
- Application job kinds may not start with `rustcms.`.

## [0.7.1] — 2026-10-20

### Added (0.7.1)
- `Dialog` (panel and the SDK kit): a close button (×, `action.close`) after
  the content, so opening still focuses the first field; `closeButton` and
  `closeDisabled` (which also blocks Escape). Dialogs are labelled by their
  title (`aria-labelledby`), so `title` may be any node.
- Form dialogs cannot be closed or cancelled while submitting.

## [0.7.0] — 2026-10-20

### Added (0.7.0)
- Roles page (P7, ADR 0034): permissions by area (collapsible, selected/total
  counts), a search box (name, label, description, area and group labels;
  case and diacritics ignored, `đ` too), labels and descriptions, presets as starting selections.
  Declared with `AuthProvider::permission_area`, `permission_meta` and
  `permission_preset`, checked when the panel registers. Bulk toggles act
  only on shown, grantable permissions.
- `PermissionInfo` gains `label`, `description`, `area`, `areaLabel`,
  `areaSort`; `GET {admin}/api/system/permission-presets`.
- Runtime branding (P9, ADR 0033): `AdminProvider::branding(impl
  BrandingSource)` with a checked `Branding` (name, logo, dark logo, favicon,
  color tokens, radius), cached per instance (`[admin] branding_ttl_secs`,
  `branding_timeout_ms`), `AdminBranding::invalidate`, the static theme on
  failure, `[admin] branding = "static"` (`RUSTCMS_ADMIN__BRANDING`) to
  recover, and `admin:branding` to check.
- The sign-in, reset and invitation pages show the brand name and logo; the
  brand names the tab on every page.

### Changed (0.7.0)
- Theme files are served at `{admin}/assets/brand/{theme.css,logo,favicon}?v=`
  (by kind, current bytes; cached for good only when `v` is current) instead
  of `assets/ext/`. The index config carries `brand` and `theme.logoDark`.
- The shell takes its brand from the index config (one branding per
  document); `PanelResponse.brand` is the current branding's.

## [0.6.1] — 2026-10-19

### Added (0.6.1)
- A per-user color scheme switch (Light / Dark / System) at the bottom of the
  sidebar. It is kept in the browser (localStorage, other tabs follow); the
  app's `Theme::color_scheme` stays the default until the user chooses.
- The SDK's `useColorScheme()` (`"light"` / `"dark"`, re-rendering on change),
  for canvas charts. No host API change; SDK 0.6.1.

## [0.6.0] — 2026-10-19

### Added (0.6.0 — W9 part B)
- `ui` level 2 for v2 extensions (ADR 0032 amended):
  - `props.resources` (`list`, `get`, `create`, `update`, `delete`,
    `restore`, `forceDelete`, `action`) through the resource routes, with
    idempotency keys, the shared paused-write scope and its banner in the
    view, and cross-view refresh;
  - `props.toast` (`success`, `error`);
  - `props.title(text | null)` on pages: the heading and breadcrumb of the
    current sub-path;
  - the SDK's `useResource`, `useRecord`, `useResourceMutation`,
    `fieldErrors` and `resourceKey`;
  - components: `Tabs*`, `DropdownMenu*`, `Tooltip`, `Table*`, `Skeleton`,
    `Separator`, `CodeBlock`. Menus and tooltips work inside dialogs and
    keep the CSP.
- `PanelPageMeta::permission_any([…])`: a page for holders of at least one
  of the listed permissions (any-of), combined with `.permission` (all-of);
  checked for the navigation, `home`, the page and its sub-paths.
- More navigation icons (home, dashboard, server, network, container,
  hard-drive, bell, git-branch, database, terminal, …).
- `make:sdk` lists the older `ui` declarations it finds in
  `src/providers/admin_panel_provider.rs` (inline, or a shared `Requires`
  constant); `rustcms upgrade` does so when the vendored SDK's VERSION
  differs from its own.

### Changed (0.6.0)
- `Requires.ui` is a minimum: the panel accepts a level up to its own
  (it was an exact match). `ui: 1` bundles keep working.
- `make:page --react` writes `ui: 2`. A bundle built with the new SDK
  refuses to register on an older panel, or when its `Requires` declares
  `ui: 1`.

## [0.5.1] — 2026-10-19

### Added (0.5.1 — W9 part A)
- `AdminProvider::home(slug)`: `/admin` opens that custom page for users who
  may open it (`PanelResponse.home`); others see the dashboard. The home is
  the first sidebar entry.
- `PanelPageMeta::sort` and `ResourceMeta::sort`: sidebar order (entries by
  sort, groups by their smallest sort; unchanged when not set).
- `ResourceMeta::navigation(false)`: no sidebar entry or dashboard card;
  routes, record pages and the API stay (`ResourceInfo.nav`).
- Breadcrumbs name a custom page on its sub-paths.

## [0.5.0] — 2026-10-18

### Added (0.5.0 — W5)
- Extension host API v2 (ADR 0032): React pages, slots and cells rendered
  inside the panel, on its React, React Query, UI kit, theme and router.
  - `AdminProvider::extension_script_v2(name, js, Requires { … })`: the
    requirements are checked before the script runs; incompatible bundles
    never load. `extension_timeout(Duration)` (1–60 s).
  - `window.rustcms.v2` (`modules`, `versions`, `registerPage/Slot/Cell`);
    `window.rustcms.apiVersion` stays 1 and v1 scripts are unchanged.
  - Props: `data`, `path`, `search`, `scope`, `can`, `api` (CSRF, unknown
    write outcomes paused per view), `realtime`; views are remounted per
    session epoch and have an error boundary with Retry.
  - Sub-pages: `GET {admin}/api/pages/{slug}/{*path}` (raw-path validation),
    route `/p/{slug}/{*path}` in the panel, `Ctx::page_path()`.
  - `GET {admin}/api/panel` lists the admin permissions the user holds
    (`permissions`).
- The admin SDK (`@rustcms/admin-sdk`): types, `useAdminQuery`,
  `useAdminMutation`, `useEvents`, `useSocket`, the Vite preset and
  `rustcms-build` (one IIFE and one CSS file per `src/<vendor>/<name>.tsx`,
  no runtime CSS injection).
- CLI: `make:page --react` (sources, build setup, provider registration),
  `make:sdk`; `rustcms upgrade` warns about an older SDK.
- Demo: a React page with sub-pages, an admin route, a stream, a CSRF write
  and a dialog.

## [0.4.1] — 2026-10-18

### Added (0.4.1 — W4)
- The scheduler (ADR 0031): `AppBuilder::schedule`, with `Schedule::every`
  and `Schedule::cron` (UTC, five fields) and the actions `.job(make)` and
  `.run(f)`.
  - Aligned slots, each claimed once in the database (`schedule_slots`,
    `schedule_states`) in the transaction of its job.
  - `.without_overlapping()` holds a slot back while the previous job is
    still pending or running.
  - Missed slots are skipped (`[schedule] grace_secs`).
  - Only `serve` runs it (never commands or tests); `[schedule] enabled`.
  - `schedule:list`, `schedule:run`; claims are pruned after 7 days.
- Registering the same job type twice is a no-op; two types sharing a `KIND`
  still panic.

### Added (0.4.1 — W8)
- Encrypted columns (ADR 0030):
  - `rustcms::crypto::Encrypted<T>`, a SeaORM value type holding only
    ciphertext;
  - explicit `seal` / `open` with the keyring and a `Context`
    (table/column/row AAD);
  - `Debug` redacted, `Serialize` `null`, never deserialized;
  - 32 KiB per value.
- Keyring: `APP_PREVIOUS_KEYS` (`app.previous_keys`) only opens. Sessions,
  JWTs and links still end at a rotation. Available as `state.keyring()` and
  `Ctx::keyring()`.
- `key:reencrypt` (`--dry-run`, `--list`, `--batch`):
  - authenticates every value;
  - compare-and-swap writes;
  - resumable; exits non-zero until done;
  - also reseals settings secrets.
  - Columns are registered with `AppBuilder::encrypted_column`, and admin
    fields register theirs.
- Admin: `Field::encrypted()` (`SecretInput`) and `Field::encrypted_map()`
  (`SecretMapInput`, vocabulary 8).
  - Write-only masked views (`state`, `keys`, `revision`).
  - Write operations: keep, replace, clear, and set/keep/remove with a
    revision (409 when stale).
  - Redacted in the audit log; out of sorting, search, filters, tables,
    imports and exports.
  - Idempotency hashes keyed.
- Settings secrets open with previous keys.
- Demo: Integrations (an API token and environment variables); E2E on
  Chrome.

### Changed (0.4.1)
- `rustcms::settings::{read, load, write}` take the `Keyring`
  (`state.keyring()`) instead of the `AppKey`.

## [0.4.0] — 2026-10-18

### Changed (0.4.0 — tooling)
- Test servers (docker compose PostgreSQL and MySQL) keep their data in
  memory and never fsync: the test matrix runs in 158 s instead of 360 s
  (MySQL variants about 3x faster).
- `just test-fast`: every test on SQLite only, no server.
- CI `test-rust` is back on: one `test-build` job archives the tests
  (`cargo nextest archive`), then three shards run them at once: SQLite plus
  every test without a database variant, PostgreSQL, and MySQL. The servers
  run in memory without fsync. The nextest `ci` profile retries a failure
  once and reports it as flaky.

### Security (0.4.0)
- ammonia 4.2.4 (RUSTSEC-2026-0373: case-insensitive CSS `url(` in style
  attributes). Not reachable here: the HTML allowlist drops `style`.
- inout 0.2.2 (aes-gcm's dependency): 0.2.3 was yanked.

### Added (0.4.0 — W2b, W6)
- WebSocket on admin routes (ADR 0029): `AdminRoutes::socket`, `LiveSocket`,
  `SocketResponse::target`, `AdminSocket` (`recv`, `send`, `authorize`).
  - Admission: a mandatory exact `Origin` and the session's CSRF token as
    the `rustcms.csrf.<token>` subprotocol. No tickets.
  - A supervisor exists before the `101`. Rechecks every
    `[realtime] socket_revalidate_secs` (10).
  - Bounded queues each way, deadlines on every write, and a keepalive with
    one outstanding ping.
  - Close codes 4401, 4403, 1001, 4008, 1009, 1011.
  - New keys: `max_message_bytes`, `send_timeout_secs`.
- `ctx.realtime.socket` for script views: no reconnect by default, epoch
  bound, `send` bounded.
- `TestApp::listen`, `TestClient::socket` / `socket_with`, and `TestSocket`
  for WebSocket tests.
- CSP (W6): the panel's policy adds the `ws(s)://` form of each panel origin
  to `connect-src` (`rustcms::admin::spa::content_security_policy`). Nothing
  else changes. `AuthState::allowed_origins`.
- Demo: an echo socket on the Live page, with a `whoami` command authorized
  when it runs; E2E on Chrome.

### Added (0.4.0 — W2a, W3)
- `Event::resync()`: a producer asks pages to reload (as the hub does for a
  subscriber that fell behind); `rustcms::auth::TargetFuture` for reusable
  target checks. From rust-panel's integration feedback.
- Server-sent events on admin routes (ADR 0027): `AdminRoutes::events`,
  `LiveSession`, `EventStream::target`.
  - Admission on the panel's own origin without a CSRF token.
  - Rechecks every `[realtime] revalidate_secs` (session, user and version,
    fresh permissions, target).
  - Bounded queues independent of the client, per-instance limits (429),
    `closed` events.
  - Shutdown within `shutdown_grace_secs` (`Shutdown`, `App::serve_on`).
- `rustcms::realtime::Hub` (ADR 0028): one producer per key, idle grace,
  `Lagged` → `resync`, bounded keys and events. `Event::json`, plus re-exported
  `stream`, `StreamExt` and `time`.
- SPA: `ctx.realtime.events` for script views (fetch-based, backoff, closed on
  unmount and on session change); `ApiClient.onEpochChange`.
- Demo: a Live page (hub counter, reset POST); E2E on Chrome.

### Added (0.4.0 — W1)
- `AdminRoutes` / `AdminAccess` (ADR 0026): JSON routes for the panel's own
  pages under `{admin_path}/api/x`, on the admin session (Origin and CSRF on
  writes), each with `authenticated()` or `can(permission)` checked before
  extractors; no public access; API tokens and JWTs get 401.
  `AuthProvider::admin_permissions` declares their permissions in the admin
  guard; `register` lists them in `route:list`.
- `rustcms_testing::admin_routes::assert_access`: an access table per route
  and actor that also fails on a route without a case.

### Added (0.4.0 — W9)
- `TestApp::new` / `with_config` take any `FnOnce(Config) -> AppBuilder`
  (closures binding test doubles; function items still work).
- `RUSTCMS_TEST_BACKENDS=mysql` (any of `sqlite,postgres,mysql`): `db_test!`
  runs only those, each then required; empty or unknown names, or a required
  backend left out, fail.

### Added (0.4.0 — W7)
- Configuration from files: every well-known variable and every
  `RUSTCMS_<PATH>` also reads `<VAR>_FILE` (Docker/Kubernetes secrets), in its
  variable's place; both set, a missing, empty, non-UTF-8 or over-64-KiB file
  fail boot without quoting the content; one trailing line break removed.

### Added (0.4.0)
- Guide: *Database schema* reference (users, roles, permissions and their
  assignments, sessions, tokens, JWT, one-time links, every other framework
  table), linked from *Roles & permissions*.

### Fixed (0.4.0)
- `rustcms self-update --version v0.3.1` (going back to a release that prints
  no target in `--version`) was refused; such a command is checked by version.

## [0.3.2] — 2026-10-16

### Added (0.3.2)
- `rustcms doctor`: Rust and Cargo against the minimum version, a C compiler
  that really builds, git and the RustCMS-Core tag, curl, the latest release,
  the application's version; what to do for each problem.
- `rustcms upgrade [--dry-run] [--force]`: the application to the command's
  version (Core tag, SeaORM pins, metadata) with `toml_edit`, keeping comments
  and features; a failed `cargo update` restores `Cargo.toml` and `Cargo.lock`.
- `rustcms self-update [--version]`: for a command installed by `install.sh`
  (`install.json`); one at a time; download, checksum, extraction and a run of
  the new command before a rename over the old one.
- `rustcms docs [--print-path]`: release builds embed the guide (offline, in
  the user cache directory); other builds open the published copy of their
  version. `rustcms --version` shows the target.

### Changed (0.3.2)
- The application is found by parsing `Cargo.toml` (`[package.metadata.rustcms]`),
  not by searching its text.
- `make:command` refuses the names of the command's own commands; `rustcms --
  <command>` always reaches the application.
- CI runs every test of the command (unit, `self-update`, generated app) on
  Linux and macOS.

## [0.3.1] — 2026-10-16

### Added (0.3.1)
- Distribution (ADR 0025): a tag publishes the `rustcms` command for macOS
  on Apple silicon and Linux (x86_64, arm64; Windows: WSL; Intel Macs: `cargo install`), the guide and the install script (`install.sh`)
  to irisvn/RustCMS (`.github/workflows/release.yml`), after
  each binary installed itself and built and tested a new application on its
  platform. `just release-bump` / `release-check` keep every version in step.
- `rustcms new` writes a CI workflow for the application (`cargo test --locked`)
  and resolves the dependencies (`--no-fetch` to skip).
- Tests of SMTP over TLS against a `mailpit-tls` service (STARTTLS, AUTH, a
  test CA from `just test-tls-cert`): delivery with the trusted root, refusal
  of another name and of an untrusted certificate, no password in the audit.

### Changed (0.3.1)
- `rustcms new` depends on the Git tag of RustCMS-Core matching the command's
  version (`rustcms = { git = …, tag = "vX.Y.Z" }`); a local checkout only with
  `--rustcms-path`. Paths are written as proper TOML strings.
- The repository is irisvn/RustCMS-Core; the guide links to it.
- `.gitattributes`: LF in every checkout (generators insert code at
  `\n`-terminated markers in the embedded templates).

### Fixed (0.3.1)
- Codex verify of M23: the restore procedure also ends JWT sign-ins; LMS
  viewers and editors see the news' images (`media.manage_all`, viewing
  only); a slug cleared while editing is made again from the title;
  Vietnamese slugs from decomposed text; the news seeder writes an article
  and its tags in one transaction; tutorial generator commands use the
  `rustcms` CLI; operations pages say what media pruning, scheduler locks
  and mail retries really do.
- Mail: a TLS handshake refused (a certificate not trusted, or for another
  name) is reported as `smtp_tls`, not `smtp_connect`.
- Local storage on Linux: every upload, import and export failed with
  `Bad file descriptor` (the parent directory was synced through an `O_PATH`
  handle); it is now opened for reading.
- Migrations run on one connection (`migrate`, `migrate:rollback`,
  `migrate:refresh`, as `migrate:fresh` already did): on SQLite under Linux a
  pooled connection could still see the schema from before a step, so a
  column dropped and added again was skipped. `TestDb::migrate` does the
  same; `TestDb::single_connection` for tests that change the schema.
- Security: ammonia 4.2.3 (RUSTSEC-2021-0157, -0158) and rustls 0.23.46
  (RUSTSEC-2026-0346 to -0349); `cargo deny` skips the licenses of the
  unpublished example apps.
- CI: Mailpit for the E2E suite and the SMTP test; test binaries built
  without debug info (the linker ran out of disk).

## [0.3.0] — 2026-10-16

### Added (0.3 — M23)
- `examples/lms`: news (categories, tags, articles with rich text, a cover,
  a unique slug made from the title, scheduled by `published_at`), editor and
  viewer roles and accounts, *Move up* / *Move down* on lessons (course lock,
  renumbering, audit of every move), news seeder.
- Guide: Operations chapter (queue, retention, backup and restore, APP_KEY,
  mail); LMS page; Vietnamese tutorial in `docs/tutorial/`.
- Upgrade tests: a 0.2 account's Argon2 password works after the upgrade; a
  partly applied invitations migration completes on the next run.

### Changed (0.3 — M23)
- Version 0.3.0 (workspace, examples, admin SPA).

### Changed (0.3 — docs)
- Documentation split: `docs/guide/` is the public developer guide (English,
  mdBook; getting started, basics, security, admin panel, APIs, digging deeper,
  reference, release notes, upgrade guide), `docs/core/` holds private material
  (architecture, admin protocol, ADRs, reports, this changelog, review
  checklist, contributor guides). `CHANGELOG.md` moved to `docs/core/`.

### Added (0.3 — M15e; ADR 0019)
- `AdminProvider::theme(Theme)`: color tokens (light and dark), radius, font
  family, PNG/WebP/ICO logo and favicon, `ColorScheme`; served as hashed
  assets; dark mode follows `data-scheme`.
- Custom pages (`PanelPage`, `AdminProvider::page`, `/admin/p/{slug}`) and
  slots beside records (`PanelSlot`, `Placement::RecordAside`,
  `AdminProvider::slot`), drawn from server templates (minijinja, always
  escaped, then sanitized with ammonia 4.2.0) or extension scripts;
  `GET pages/{slug}`, `GET slots`; navigation lists pages
  (`PanelResponse.pages`).
- `Ctx::records` / `Ctx::record`: resource reads with scope, policy and
  field rules, for custom code.
- Custom cells: `TableColumn::cell` / `props`, `registerCell`; script API
  `registerPage`, `registerSlot`; template classes `rc-*`.
- `make:page [--script]`; `resources/` folder (`views/admin`, `admin/js`,
  `admin/css`); `make:widget` writes to `resources/admin/js`.
- Examples: LMS theme, course overview page, seats slot, progress cell; demo
  post stats page and visibility slot.

### Fixed (0.3 — Codex review of M17b–M22)
- Import rows check `create` for the user as they are under the RBAC lock
  (a withdrawn `create` with `update` kept created records); `write_conflict`
  retries the import job instead of failing the row; the uploaded file is
  stored before the import and its idempotency receipt are committed.
- Using a reset or invitation link, or an administrator setting the
  password, leaves marks that supersede older requests (a retried older
  reset issued a new link after the reset); `auth.invitation_days` ≤ 30.
- Dashboard: date-only fields count whole UTC days the same way in SQL and
  in scans; widgets refresh after writes.
- Exports never start past their expiry; pruning waits an hour after it, so
  a late upload's object is always recorded and deleted.
- SPA: a lost answer to a reset or invitation says the password may already
  work (no retry offered); completions act only while their page is open.
- Full gate: a quoted empty cell of a *required* text field is refused again
  (it was imported empty); the marks of a credential change no longer count
  as a recent token for the reset cooldown (a reset right after accepting an
  invitation sent nothing); the admin SPA bundle is rebuilt; E2E covers
  invitations, resets, remember me and the dashboard (Mailpit).

### Added (0.3 — M22; ADR 0024)
- Dashboard widgets: `AdminProvider::dashboard(Dashboard)`, `Widget::stat`
  (`compare_last_days`), `Widget::chart` (`Bucket::Day/Week/Month`, `last`),
  `Widget::recent` (`columns`, `sort`, `limit`), `.filter/.label/.span/
  .permission`; permissions `dashboard.<name>` (group `dashboard`); routes
  `GET dashboard`, `GET dashboard/widgets/{name}`; `DynResource::counted`
  (database count on the list's subquery, or a bounded per-record scan;
  `too_many`); boot checks; SPA widgets (SVG bar chart without inline
  styles, a screen-reader table); demo and LMS dashboards; LMS
  `courses.created_at` field.

### Added (0.3 — M20; ADR 0023)
- Password resets: `[auth] password_reset` (+ `_minutes`, `reset_requests_per_*`,
  `reset_cooldown_secs`), `POST auth/forgot-password` (always 202),
  `POST auth/reset-password`; job `auth.password_reset` makes the token under
  the user lock and mails it; `user_tokens` table (HMAC of the secret, one per
  user and purpose, request id/time); link secret in the URL fragment, pages
  served `Referrer-Policy: no-referrer`, SPA fetches with
  `referrerPolicy: same-origin`.
- Invitations: `users.invite`, `RbacService::invite_user` /
  `resend_invitation`, `POST system/users/invitations`,
  `POST system/users/{id}/invitation`, `POST auth/invitation`, job
  `auth.invitation`; `users.invitation_pending` / `invited_at` / `invited_by`;
  `SystemUser.invitationPending`, `invitedAt`, ability `resendInvitation`;
  `SystemLinks.inviteUsers`.
- Remember me: `[auth] remember_days` (30), `remember` on login,
  `Session::login_remembered`.
- SPA: forgot / reset / invitation pages, remember box, invite option,
  pending badge, resend button; runtime config `signIn`.
- Issue template `token_invalid`; mail templates `auth.password_reset`,
  `auth.invitation` (en, vi).

### Changed (0.3 — M20)
- A password that is not an Argon2 hash verifies like an unknown user (same
  work, `false`); invited (pending) users cannot sign in; the last-super-admin
  rule counts accepted accounts only; an administrator setting a pending
  user's password ends the invitation.

### Added (0.3 — M19; ADR 0022)
- Mail (`rustcms_core::mail`, feature `mail`, default in `rustcms`):
  `MailProvider`, `[mail]` (drivers `disabled`/`smtp`/`log`/`memory`, source
  `settings`/`config`, `allowed_hosts`/`allowed_ports`/`allow_insecure`,
  timeout, per-process rate, `log_body` local only, `tls_ca_file`),
  `MailSettings` group, `render`/`send`/`send_later` (`mail.send` job, 8 runs),
  `mail_template!` + `AppBuilder::mail_template` (re-read in local
  development), `MailError` codes. lettre 0.11.23 (rustls).
- `queue::Failure::permanent()` / `queue::Permanent`: failed at once, not retried
  (ADR 0016 amended); `Failure`s are logged by code only.
- `rustcms_core::template`: the bounded minijinja environment (moved from the
  admin's custom pages, shared with mail).
- Admin: `AdminProvider::mail_settings()` (Mail page, policy-checked),
  `SettingField::kept_while` (a secret cleared when bound fields change),
  `SettingsPage::validate_in`, `POST settings/mail/test`, `abilities.test`
  on settings pages; SPA test-mail card.
- Testing: `TestApp::mails` / `take_mails` (feature `mail`), `test_config`
  sets the memory driver; Mailpit (`axllent/mailpit:v1.31.3`) in docker
  compose, `RUSTCMS_TEST_SMTP` in `just test-matrix`.
- Examples: LMS welcome mail (en/vi, sent once per seat), demo log driver;
  both have the Mail settings page.

### Changed (0.3 — M19)
- nextest stops a test after five minutes (`slow-timeout`), so a stuck test
  fails instead of holding the run.

### Added (0.3 — M17b; ADR 0021)
- CSV imports: `ResourceMeta::importable()` / `import_key(field)`, ability
  `import`; routes `POST {slug}/imports`, `GET imports/{id}`, `POST
  imports/{id}/preview`, `POST imports/{id}/run`; job `admin.import` (user
  reloaded per batch, a row per transaction holding the RBAC lock, with a
  receipt in `import_rows`); `imports` table; audit `imported`; templates
  `not_found`, `several_matches`, `not_imported`.
- `update` accepts an idempotency key.

### Fixed (0.3 — M17b Codex verify)
- Import re-runs skip rows by a receipt committed with the write (a created
  row was updated again, and every row spent the user's 1,000-key
  idempotency quota); each row's write holds the RBAC lock and re-admits the
  user; an expired or failed import is never written; abandonment is judged
  from `imports.started_at`, not the upload time.
- Reports keep only codes with numeric parameters (no hook text or quoted
  values); `""` stays the empty text; uploads honour `Idempotency-Key`, are
  parsed four at a time and bound header cells.
- Export runs use a fresh object key recorded in `export_objects` before the
  upload; pruning deletes those and the published object (M17a objects
  leaked). Migration `m20261024_000017_add_import_receipts`.
- SPA: an import whose run answer was lost is followed instead of stranded.
- SPA import page (`/r/{slug}/import`); LMS learners (by email) and demo tags
  (by name) importable. `csv` crate 1.3.1.

### Fixed (0.3 — M17a, Codex verify)
- Background exports: a file per run published together with its key list
  (a late run never mixes files and manifests); fields re-checked at each
  batch; 10 MiB / 50 MiB budgets; `Idempotency-Key` on creation; the
  per-user limit counted under a lock; a body is refused (the query is the
  URL's); failure reasons are codes translated by the panel; the last run's
  failure marks the export; temporary files removed on cancellation and
  swept; pruning deletes every run's file.
- CSV: controls after leading spaces protected; JSON columns written as
  their JSON text.
- SPA: a download finished after a sign-out or account change is dropped.

### Added (0.3 — M17a; ADR 0021)
- CSV exports: `ResourceMeta::exportable()`, ability `export`; `GET
  {slug}/export` (≤ 1 000 rows, built before sending), background exports
  `POST {slug}/exports`, `GET exports`, `GET exports/{id}/file` (job
  `admin.export` reloading its user per batch; downloads re-check
  credentials, permission, fields and every record), pruning every 10
  minutes; `exports` table; audit `exported` (query shape only).
- `DynResource`: `admit_export`, `export_columns`, `canonical_query`,
  `export_keys`, `export_rows`; `ApiClient.download` in the SPA; Export
  button and Exports panel.
- Examples: demo posts and LMS enrollments exportable.

### Fixed (0.3 — M16, Codex verify)
- History of link changes shows pivot values only with the targets (readers
  who may list the target).
- Boot refuses lock order cycles (belongs-to-many parent → target and
  `lock_parent` parent → child); declared `.required()` pivot fields without a
  default are allowed (attaching needs them; 403 when not writable).
- Pivot updates that change nothing write and record nothing.
- Permission matrix from `DynResource::permission_uses` (admission permissions
  of actions stay outside the matrix).
- SPA: attach sends only filled-in pivot columns, link edits only changed
  ones; detach acts on the rows shown (selection cleared on another query,
  no writes on placeholder rows); row and column toggles follow grantable
  permissions.
- Demo: `PostPolicy::admits` refuses link writes and `move` before the body.

### Added (0.3 — M16; ADR 0020)
- Belongs-to-many relations: `Relation::belongs_to_many` over a pivot entity
  with `.pivot(fields)`; routes `…/relations/{relation}/attach`, `/detach`,
  `PATCH …/links/{target}`, `GET …/options`; abilities `attach_{rel}`,
  `detach_{rel}`, `update_pivot_{rel}` on the parent; lists add readable pivot
  columns (`LinkPage.pivot`); audit `attached` / `detached` / `pivot_updated`
  shown in the history by reader.
- Action forms: `Action::form(vec![ActionField::…])` (text, textarea, integer,
  boolean, date, select, `belongs_to`), `ActionSchema.form`, options route
  `…/actions/{action}/fields/{field}/options`; SPA dialog.
- Permission matrix on the role page; `PermissionInfo.resource` / `ability`.
- Examples: demo tags (`post_tags` with `position`) and "Move to category…";
  LMS "Enroll a learner…" on courses (`enrollment_service::enroll_in`).

### Changed (0.3 — M16)
- Action requests are admitted before the body is read; `data` without a
  form is refused; the body holds only `keys` and `data`; actions the policy
  does not admit are not listed.
- `SCHEMA_VERSION` 2, `VOCABULARY_VERSION` 7; `RelationSchema.kind`.

### Added (0.3 — M15e, template files)
- `rustcms_admin::view!("file.html")` → `PageView::File(TemplateFile)`: the
  template `resources/views/admin/file.html` embedded at build time and, in a
  debug build with `APP_ENV=local`, read again at each render (checked as at
  boot; an error answers 500; a missing file falls back to the embedded
  copy). Release builds never read it. Stubs and examples use it.

### Fixed (0.3 — M15e, Codex verify)
- Template statements are found by a lexer: `{%autoescape%}` and other
  spacings no longer pass the boot check (also `include`, `import`, `from`,
  `extends`).
- Template output is written into a buffer bounded at 512 KiB: rendering
  stops at the limit instead of building the whole string first.
- `lang:missing` finds `t( "…" )` with spaces and escaped quotes, only inside
  template code.
- `GET slots`: a resource admitting by permission answers 403 before
  parameter errors (no 422 to users without `view`).
- SPA: a page or slot whose refetch fails (403/404) is no longer shown from
  cache; script views are bound to the session epoch (callbacks of an earlier
  epoch ignored, remounted; custom queries keyed by epoch); a view that does
  not return `{update, unmount}` is marked failed and its element cleared.
- `make:page --script` refuses script names the panel refuses at boot.
- Demo post stats count published posts through a filtered list total.

### Changed (0.3 — M15e)
- `VOCABULARY_VERSION` 6 (columns may name a cell).
- `AdminPanel` gains `custom`; `spa::Extension` carries bytes and a content
  type; `safeHref` refuses backslashes.

### Added (0.3 — M15d)
- Admin **Queue page** (System → Queue): counts per queue and status, job
  list (status, queue, kind filters; keyset pages), retry / forget / cancel
  of one job; `queue.view` and `queue.manage` (both for actions, re-read
  under the RBAC lock); audit events `queue.retried`, `queue.forgotten`,
  `queue.cancelled`; payloads and worker tokens never sent.
- `rustcms::queue::manage`: `JobStatus` (pending, scheduled, running,
  stalled, failed — the workers' own conditions, clock skew included),
  `Clock`, `counts`, `retry`, `forget`, `cancel` (conditional statements).
- `SystemLinks.queue`; resource slugs `job` and `queue` reserved; icon
  `layers`.

### Fixed (0.3 — M15d, Codex verify)
- Queue counts come from one query (no job counted twice while it changes
  status); retry and forget of a running job answer `not_failed`; the id is
  parsed only after the permissions are re-read under the RBAC lock.
- Queue page: a write with an unknown outcome pauses the actions (paused
  banner); notices follow a language change; the retry confirmation says
  which effects may repeat.

### Changed (0.3 — M15d)
- `queue:retry` also clears a stale reservation of the jobs it retries.

### Fixed (0.3 — R3, Codex verify)
- Parent locks: a parent set by a column default (field left out) or spelled
  differently (`01`, upper-case UUID) is checked against the locked keys after
  the write; an unlocked one rolls the write back.
- Generators: module indexes and their anchors are checked before any file is
  written (`make:api`, `make:resource`, `make:settings` writes both halves or
  neither); a flat `src/<module>.rs` or an undeclared folder is refused; Rust
  keywords (`make:command type`), `DatabaseSeeder`, settings groups over 32
  characters, job kinds and seeder names over 64, and test modules already
  declared (`support`) are refused.

### Changed (0.3 — R3, application layout; ADR 0017)
- Applications are laid out like Laravel inside `src/`: `bootstrap/`,
  `providers/` (auth, admin panel, app service providers), `routes/api.rs`,
  `http/{controllers,requests,resources}`, `models/`, `services/`,
  `policies/`, `jobs/`, `settings/`, `console/`, `admin/{resources,pages}`,
  `database/{migrations,seeders,factories}`, `tests/{support,feature}`.
  `rustcms new`, every generator, `examples/demo` and `examples/lms` follow
  it; generators refuse the old layout (upgrade guide: "Application layout").
- Generators check every file and anchor before writing anything.
- `make:resource` writes `admin/resources/<model>_resource.rs`; `make:api`
  writes a controller, a JSON resource and explicit routes in `routes/api.rs`;
  `make:migration` writes to `database/migrations`; `make:test` to
  `tests/feature` (one test binary).
- Demo: `demo:seed` replaced by seeders and `db:seed`; settings data and page
  split.

### Added (0.3 — R3)
- `rustcms::database`: `Seeder`, `call`, `Factory` (moved from
  `rustcms-testing`, still re-exported there; `try_create`,
  `try_create_many`); `AppBuilder::seeder`; `db:seed [--class] [--force]`;
  `migrate:fresh --seed`; `rustcms::clap`.
- Generators `make:service`, `make:job`, `make:seeder`, `make:factory`,
  `make:settings`, `make:command`.
- `ResourceMeta::lock_parent(field, target)` (ADR 0018): writes lock the
  parent record first (both when moving), in the order a parent's deletion
  cascades; hooks and actions may not move a record to a parent not locked;
  a parent changed before the lock is a 409.
- Unique constraint messages (`ResourceMeta::unique`) are translated in the
  request's language and listed by `lang:missing`.

### Added (0.3 — M15c)
- `examples/lms`: courses (capacity, instructor), lessons, learners and
  enrollments; seat rules in `services/enrollment_service.rs` (panel hooks and
  seeders alike) under the course lock (`lock_parent`); instructors scoped to
  their courses with `EXISTS` subqueries; progress behind
  `enrollments.update_progress`; `EnrollmentPolicy` refuses before lookup and
  needs no list scan; `lms.enrolled` job dispatched in the enrollment's
  transaction; `lms` settings group; seeders; Vietnamese catalog; tests on
  SQLite, PostgreSQL and MySQL. Guide page "Example: an LMS back office".

### Fixed (0.3 — M15c)
- The framework's Vietnamese catalog translates the "Audit" and "Settings"
  permission groups (`lang:missing vi` reported them in every app); demo and
  lms test that their catalogs are complete.

### Added (0.3 — M15b)
- S3 storage tested against a real S3-compatible server (S3Mock in docker
  compose and CI; MinIO no longer publishes images): round trips, multipart,
  media upload/download/deletion on a bucket, staging sweep, timeouts.
- `storage.s3.timeout_secs` (default 30): request timeout and retry budget.
- `rustcms_testing::TestS3`, `TestApp::router`; `RUSTCMS_TEST_S3` and `s3` in
  `RUSTCMS_TEST_REQUIRE`; `just test-matrix` and CI run with `rustcms/s3`.

### Fixed (0.3 — M15b)
- S3 uploads are staged in `<storage.root>/.s3-staging` (private, never a
  symbolic link), not a shared temp directory; the sweep removes only stale
  `*.upload` files.
- Control requests to the bucket (exists, delete, opening a download) end
  within `s3.timeout_secs`, retries included; data transfers get
  `s3.transfer_timeout_secs` (new, default 600).
- `media:prune` stops taking work after a five-minute pass.
- `allow_http` is kept when client options are set (plain-HTTP endpoints).

### Added (0.3 — M15a)
- **Job queue** (ADR 0016): `rustcms::queue` (`Job`, `dispatch` in the
  change's transaction, `dispatch_with`, `enqueue`, `once`, `Failure`),
  `AppBuilder::job`, `Ctx::dispatch`; `jobs` and `job_receipts` tables (new
  migration); workers in `serve` and `queue:work` with owner tokens, leases,
  heartbeats, timeouts, retries with backoff, rolling-deploy tolerance and
  graceful shutdown; `queue:failed`, `queue:retry`, `queue:forget`; `[queue]`
  settings validated at boot; audit actor `job` (shown as "Background job"
  in the audit viewer).

### Added (0.3 — M14b)
- **Audit log viewer**: System → Audit log (`audit.view_all`; sign-in
  families and addresses with `audit.view_auth`), filters and keyset paging in
  SQL; **History** on record pages (`audit.view` + record `view`), readable
  fields only. Routes `GET {admin}/api/audit` and
  `…/records/{key}/history`; `SystemLinks.audit`; schema ability `history`.

### Changed (0.3 — M14b)
- Resource slugs `user`, `role`, `permission`, `settings`, `session` are reserved.
- Audit entries store instants in microseconds; a key that looks like a
  shortened key is shortened too (stored keys are one to one).

### Added (0.3 — M14a)
- **Settings pages** (ADR 0015): groups as Rust structs (`rustcms::settings::
  Settings`, `get`, `load`), edited in the panel (`AdminProvider::settings`,
  `SettingsPage`, `SettingField`; kinds text, textarea, integer, boolean,
  select, key/value, date, secret), stored one row per group (`settings`, new
  migration) with optimistic versions; permissions `settings.<group>.view` /
  `.update`; audit `settings.updated`.
- `Secret`: AES-256-GCM with a key derived from `APP_KEY`, bound to group,
  field and version; never serialized or sent; explicit replace / clear.
- Vocabulary 5: `secret` input (`SecretInput`); `PanelResponse.settings`;
  issue codes `not_option`, `unreadable_secret`.
- `rustcms_auth::rbac::begin_as`: a write transaction under the RBAC lock
  with the actor's current grants; `AppKey::derive`.

### Changed (0.3 — M14a)
- Value checks of resources moved to `engine::validate` (`check_input`,
  `check_written`), shared with settings; behaviour unchanged.
- Docs: `#[rustcms::async_trait]` (which Rust rejects: attribute paths may not
  start with `rustc`) replaced by `use rustcms::async_trait;` + `#[async_trait]`.

### Added (0.3 — M13)
- **Audit log** (`audit_logs`, new migration; ADR 0014): resource writes,
  actions, relation creates, uploads, every `RbacService` change, account
  changes, API tokens and console commands recorded in the write's
  transaction; sign-ins, sign-outs, failed sign-ins and JWT events best
  effort. Actor, credential (`Principal.credential`), IP, user agent and
  request id; diff of declared fields, redacted by declaration
  (`Field::audit_redact`, `readable`, `guarded`, password inputs) and bounded;
  `ResourceMeta::audit(false)`; `ctx.audit` and `rustcms::audit::record` for
  application events.
- `[audit]` settings (`retention_days`, `sign_in_failures_per_minute`), hourly
  pruning and `rustcms audit:prune`.
- An `x-request-id` that is not a short plain token, or is sent twice, is
  replaced by a generated one.

### Changed (0.3 — M13)
- `credentials::change_password` / `logout_everywhere` take an
  `&AuditContext`; `credentials::issue_token` and `jwt_session::{issue, rotate,
  logout}` a `&RequestMeta`. `Principal` has a `credential` field.
- Revoking an API token (`DELETE /api/v1/auth/token`) runs in a transaction
  with its entry.
- `Ctx::new` audits writes as its principal (was: the system).
- Resource slugs are at most 64 bytes and action names 57; a primary key field
  cannot be `.audit_redact()` (registration panics).

### Added (0.3 — M12)
- **Admin interface in English and Vietnamese.** Users choose on their account
  page (saved in `users.locale`, new migration; `POST {admin}/api/auth/locale`);
  otherwise the browser's language, else `app.locale` (`app.locales` lists the
  choices). Before sign-in the login page offers the choice.
- Application labels translated from TOML catalogs keyed by the English source
  (`AdminProvider::translations("vi", include_str!("../lang/vi.toml"))`, per
  resource with `[slug]` tables); `rustcms lang:missing <locale>`; `ctx.t` for
  hook messages; translated permission group labels (`groupLabel`).
- Validation errors carry `issues` / `formIssues` (stable code and parameters
  per message; messages unchanged) and conflicts a `reason`; the panel
  translates by code. Extension widgets get `ctx.locale`, `ctx.t` and
  `registerTranslations`.
- New apps get `lang/vi.toml`; the demo is translated (ADR 0013).

### Changed (0.3 — M12)
- `Principal` and `user::Model` have a `locale` field; `AppError::Conflict` has
  a `reason`. Struct literals of these types need the new field.
- The panel no longer infers an uncertain write from the text of an error.

## 0.1 and 0.2 (never tagged)

Developed on `main` before tags existed; first released in the 0.3.0 tag.

### Added (0.2 — M11)
- **JWT sign-ins** on the API guard: `POST /api/v1/auth/jwt` (access + refresh
  token), `POST /api/v1/auth/refresh` (rotating, single-use refresh tokens; a
  reused one ends the sign-in), `POST /api/v1/auth/jwt/logout`. HS256 with a key
  derived from `APP_KEY`, a fixed header, checked against the user and the
  sign-in on every request (revocation from the next request). `[auth.jwt]`
  settings. Tables `jwt_families`, `jwt_refresh_tokens` (ADR 0011).
- **Several instances:** permissions cached per instance but keyed by a
  database counter (`auth_generations`) every RBAC change advances, so a change
  applies everywhere from the next request. Feature `redis` with `[redis] url`
  (`REDIS_URL`) and `prefix`: shared rate limits (all-or-nothing Lua, fail
  closed) and leases so periodic jobs run on one instance
  (`rustcms_core::jobs::exclusive`). Media deletions are claimed with
  `FOR UPDATE SKIP LOCKED` (ADR 0012).
- `TestDb::reopen()` for tests with two instances on one database.

### Changed (0.2 — demo)
- The demo's post form uses tabs (Content, Publishing, Attributes) and a
  key/value `attributes` column (new migration; seeded).

### Changed (0.2 — M11)
- `RateLimiter::reserve`, `release` and `clear` are async; `session:prune` also
  removes ended JWT sign-ins.

### Added (0.2 — R2)
- **Application API layer:** `ApiRoutes` and `Access` (`public`, `authenticated`,
  `can`): every API route states its access, checked before the handler's
  extractors run (so before the body is read); routes are mounted and listed for
  `route:list` from one declaration; API routes under the admin path are refused.
  `AuthProvider::api_permissions` declares their permissions. `Throttle<N>`
  (per user and route, before the body). In `rustcms_core::http`: `Pagination`,
  `ApiPage<T>`, `Validated<T>` + `Validate`.
- `rustcms make:api <Model>`: `src/http/api/<table>/` with list and show (JSON from
  the model's columns, wire format), registered at anchors. New apps have
  `src/http/api/` (`ping` moved there).

### Changed (0.2 — R2)
- The demo API moved to `src/http/api/posts/` (handlers, requests, json). Its
  error shapes follow the framework: malformed JSON is 400 (was 422 on `body`),
  a body that is not the expected shape is 422 with a form error.

### Changed (0.2 — R1)
- Modules grouped by role, large files split (admin `adapter.rs`, `media.rs`,
  `system.rs`, `ui.rs`; auth `rbac.rs`; core `db.rs`, `storage.rs`; SPA system
  pages). No behaviour change; every existing public path is kept (re-exports,
  checked by `tests/paths.rs`). New grouping modules: `rustcms_admin::{resources,
  forms, tables, panel, engine, contract, features}`, `rustcms_auth::{guards,
  console, support}`. Architecture page in the guide.
- The demo is laid out like a generated app: one resource per file in
  `src/admin/`, policies in `src/policies/`.

### Added (0.2 — M10)
- **MySQL 8.4** as a third database (feature `mysql`, on by default in
  `rustcms`). Connections use UTC and `utf8mb4_0900_bin`; write transactions run
  `READ COMMITTED`. `rustcms_core::db::check` refuses a database, string column
  or table that is not `utf8mb4_0900_bin` / InnoDB / strict (with the SQL to fix
  it), when the app builds and before `migrate`. MariaDB is not supported.
- `rustcms_core::db::xact_lock` / `lock_slot` (named transaction locks: advisory
  on PostgreSQL, a preseeded `rustcms_locks` row on MySQL) and
  `set_lock_wait`; framework migration `rustcms_locks`.
- Migration helpers in `rustcms::migration`: `create_table`, `create_index`,
  `add_column`, `drop_column`, `drop_table`, `ColumnDefExt::{instant, instant_now,
  local_date_time}` (instants are `DATETIME(6)` on MySQL). Framework migrations
  can be re-run after a partial failure.
- Tests: `db_test!` runs on SQLite, PostgreSQL and MySQL (`RUSTCMS_TEST_MYSQL`);
  `RUSTCMS_TEST_REQUIRE` fails a skipped server; `rustcms_testing::create_table`.
  docker compose and CI run MySQL 8.4. ADR 0010.

### Fixed (0.2 — M10)
- Idempotency claims take the user's lock before reading the receipt: two
  requests can no longer both replace an expired, unpruned receipt, and a retry
  racing the request that used the last key of the quota replays instead of
  getting 429 (all databases).
- Media and rich text columns are refused with any entity default (JSON and
  expression defaults were missed).
- Lock wait timeouts, deadlocks and serialization failures of MySQL (and
  PostgreSQL 40001) answer 409 `write_conflict`.

### Added (0.2 — M9)
- **Has-many relations** (`Resource::relations()`,
  `Relation::has_many(name, target, field)`): the parent's view page lists the
  children (the target's list, scope, policy and field rules, plus
  `fk = parent`) and creates them with the foreign key set by the server.
  Routes `GET`/`POST {admin}/api/resources/{slug}/records/{key}/relations/{relation}`;
  `ResourceSchema.relations`. Checked at startup (target, `belongs_to`, key
  kinds). Idempotency keys are bound to the relation and the parent.
- **Custom inputs** (`CustomInput::make(field, "vendor/name").props(json)`),
  drawn by widgets from **extension scripts and styles**
  (`AdminProvider::extension_script` / `extension_style`), served under
  content-hashed URLs within the existing CSP. Widget API version 1
  (`window.rustcms.registerWidget`); missing or failing widgets fall back to
  read-only and are never submitted. Vocabulary 4.
- `rustcms make:widget vendor/name` generates a widget script.
- Demo: categories list and create their posts, and pick a color with a demo
  widget. ADR 0009; guides "Relationships" and "Custom fields".

### Added (0.2 — M8)
- **Storage** (`rustcms::storage`): `Storage` trait, local driver (symlink-safe)
  and S3-compatible driver (feature `s3`), `StorageProvider`, `[storage]`.
  `AppBuilder::body_limit(method, path, bytes)` for one route's body limit.
- **Media library** (`AdminProvider::media()`): `media` resource (soft deletes,
  owner scope, `media.manage_all`), `POST {admin}/api/media` multipart uploads
  (type from content, fixed allowlist, pixel budget, concurrency and rate limits,
  idempotency keys), `GET {admin}/api/media/{id}/file` authorized downloads with
  safe headers, crash-safe cleanup (`media:prune`, worker). Migrations `media`,
  `media_uploads`, `media_deletions`. `Resource::after_force_delete` hook.
- **Media fields** (`Field::media()`, `media_list(n)`, `accept`, `max_size`) and
  **rich text** (`Field::rich_text()`, `rustcms_admin::rich_text`): a validated
  JSON document, exported as escaped HTML. Inputs `FileUpload`, `RichEditor`
  (TipTap, CSP-clean); vocabulary 3. ADR 0008.

### Changed (0.2 — M8)
- Relationship checks (`belongs_to`) now run after `mutate_before_*` hooks, on
  what is actually written, together with media checks.

### Added (0.2 — M7)
- **Soft deletes**: `ResourceMeta::soft_deletes()` / `soft_deletes_column(..)`.
  Delete moves records to the trash; trashed records are hidden on every path
  (lists, reads, writes, actions, options, labels, relationship checks).
  `POST …/records/{key}/restore` and `DELETE …/records/{key}/force` with their
  own abilities (`{slug}.restore`, `{slug}.force_delete`); `trashed=with|only`
  lists; envelopes carry `trashed`; `make:resource --generate` detects a
  nullable `deleted_at`. The panel gets a trash filter and restore / permanent
  delete buttons.
- **Idempotency keys** for resource creates and actions (`Idempotency-Key`):
  claimed in the write transaction, replayed (re-rendered for the user now) on
  retry, 422 for a reused key with another request, 409 `write_conflict` after a
  bounded wait. Migration `idempotency_keys`, `idempotency:prune`, hourly
  pruning. The panel's paused banner offers **Retry safely**. ADR 0007.
- **Vocabulary 2**: `Tabs`/`Tab` layouts (keyboard tablist, errors open their
  tab) and `KeyValue` inputs on JSON columns (`Field::max_items`, bounds on
  every write). `Rule::MaxItems`; `LayoutSchema.id`.
- `AppError::Conflict` (409 with a stable code).

### Security
- Write responses re-read the record in scope: a create or update that moves a
  record out of the user's `scope()` returns only its key.
- `Policy::admits(ctx, ability)` is checked before any lookup by key; the default
  policy refuses users without `{slug}.{ability}` with 403 whether or not the key
  exists (no existence probing). Blind edits (update without view) still work.
- **Breaking:** admin session ids are HMAC-SHA256 of the cookie keyed with
  `APP_KEY` (`AuthState::new` takes the key; `AuthState::session_id`). Rotating
  `APP_KEY` signs every admin out, as `key:generate` always said. Upgrading
  signs existing sessions out once. API tokens are unaffected.
- `make:resource --generate` checks secret and owner column names under both the
  Rust field name and `column_name`.
- Token lifetimes are limited to 1–3,650 days (`expires_in_days` → 422,
  `token:create --expires-days`); huge values no longer panic.
- Numbers too large for an `f32` column are rejected (422) instead of stored as
  infinity.

### Fixed
- The session cookie no longer carries `Max-Age` and is not re-sent on sliding
  renewal (expiry is enforced by the server), so a late response cannot restore
  a cookie replaced by a newer one.
- `make:resource --generate` emits a required input for natural `String` keys;
  `rustcms new` refuses Rust keywords as package names; `make:user` enforces the
  maximum password length.
- Admin UI: a blind-edit draft survives a refetch that grants `view`; the list
  search box follows back/forward; Account shows form-level errors and the
  paused-writes banner (as do System pages); read-only details label boolean
  select options and mask password-mode values; hidden required inputs no longer
  block submit; the i32 check accepts exactly `-2^31..2^31-1`.

### Changed
- MSRV is Rust 1.94 (required by SeaORM 2.0.4), checked in CI (`just msrv`).
- `rustcms_core::db::migrate_fresh` backs `migrate:fresh` (one connection, so
  SQLite foreign keys stay off for every drop).
- Docs: revocation semantics, `just ci` scope, review report in `docs/reports/`.
- Admin UI redesigned with a monochrome, shadcn-block-inspired layout: collapsible
  desktop sidebar, native mobile navigation drawer, breadcrumbs, authorized
  overview shortcuts, consistent CRUD/System/Account layouts and OS light/dark
  themes. Responsive tables scroll locally; list filters and bulk actions have
  distinct toolbars. Details show selectable values without inferring defaults.
- Improved keyboard/focus behavior, form help/error associations, table sort and
  partial-selection semantics, and relationship picker keyboard controls. Strict
  CSP, authorization and session/write lifecycle contracts are unchanged.

### Added
- M6 developer experience and release prep: `make:resource [--generate]
  [--belongs-to field=slug]` (fields, form, table and filters from the model's
  columns; unknown `*_id` columns flagged), `make:policy`, `route:list`
  (routes described by core, providers and the app via
  `AppBuilder::describe_routes`), `TestApp::make_super_admin`; new apps get
  `src/admin`, `src/policies` and a `// rustcms:resources` anchor; demo token API
  `GET/POST /api/v1/posts`; docs: getting started with the demo, API guide,
  support boundary with footprint and roadmap; README quick start.
- Public API: `RouteInfo`, `AppBuilder::{describe_routes, described_routes}`,
  `rustcms_admin::spa::routes`, `Auth::authorize_guard`, `rustcms::async_trait`
  (`use rustcms::async_trait;`; stubs no longer need an `async-trait` dependency), `role:assign` command.
- M6 review batch: generator skips unsupported types and secret columns, handles
  `r#`, `column_name`, `ignore` and natural keys, marks NOT NULL fields required,
  creates module directories, registers idempotently, validates `--belongs-to`
  and policy names; `route:list` lists catch-all and trailing-slash routes; demo
  API authorizes before reading input, validates `page`, hides scheduled posts,
  limits creates per user; `demo:seed` refuses production.
- M5 follow-ups: RBAC COMMIT and cache eviction run to completion even when the
  request is cancelled; deleting a role you hold is refused; stale-credential
  requests never renew or re-send their cookie; hashing budgets count successful
  hashes; id lists are deduplicated and chunked; rotation start/end across tabs.
- `migrate:fresh` works on SQLite with foreign keys on (drops run on one
  connection with `PRAGMA foreign_keys = OFF`).
- Publishing: LICENSE-MIT / LICENSE-APACHE, crates.io metadata, internal
  dev-dependencies path-only (tests excluded from those packages);
  `just publish-check` packages and verifies every crate.
- M5 users, roles and account: System pages (Users, Roles with a permission
  matrix) and an Account page (password change, sign out other sessions);
  `/admin/api/system/…` endpoints, all writes through `RbacService`
  (new `create_user_with_roles`, `set_user_roles`, `update_profile`,
  `reset_password`), `PanelResponse.system`, authz oracle rows for every new
  route, escalation tests, E2E for role/user creation, disabling and password change.
- M5 hardening (exhaustive review batch): authorization before any state is read or
  compared; protected accounts (any guard) managed by super admins of every guard;
  optimistic checks for roles, permissions, profile and status; credential changes
  re-checked against `auth_version` under lock (fail closed); email changes revoke
  other sessions and tokens; strict email/role-name validation; per-caller hashing
  budget after cheap checks; batched role loading; accurate `abilities`; client
  epoch around credential rotation (also across tabs); `docs/review-checklist.md`.
- M4 admin SPA (`admin-ui/`, React 19 + Vite + Tailwind + TanStack Router/Query):
  sign-in, navigation, list (search, sort, filters, pagination, row and bulk
  actions), create/edit/view forms rendering the whole 0.1 vocabulary, relationship
  combobox; embedded in `rustcms-admin` (`assets/`, `rust-embed`) and served under
  the admin path with a strict CSP and runtime `<base>`/config injection
  (ADR 0006). Vitest + MSW unit/contract tests, Playwright E2E against the demo.
- `InputSchema.nullable`; typed `PanelResponse` / `DataResponse`; TypeScript
  bindings generated from the Rust DTOs (`ts` feature); server-generated
  `fixtures/contract/kitchen-sink.*` covering every component and wire kind.
- `AppError::CsrfMismatch` (403 `csrf_mismatch`) for missing/stale CSRF tokens.
- Admin API responses are `Cache-Control: private, no-store`; unknown
  `/admin/api/*` paths answer JSON 404 inside the admin stack.
- `auth.admin_path` is validated and normalized (`/segment(/segment)*`; not `/`
  or under `/api`).
- Demo: categories and posts resources with an ownership policy, `demo:seed`.
- `just dev`, `admin-build`, `test-ui`, `e2e`; CI jobs `test-ui`, `e2e`
  (release binary run outside the checkout), assets/types drift checks, `npm audit`.
- M3 resource engine: Filament-style builders (`TextInput`, `SelectInput`,
  `Toggle`, …, `Section`, `Grid`, table columns and filters), action metadata
  (`label`, `bulk`, `requires_confirmation`, …); versioned `ResourceSchema`
  filtered by field permissions per operation; `AdminProvider` with the admin
  API (`panel`, `schema`, records CRUD, actions, relationship options) behind
  the admin guard; resource permissions declared for `permission:sync`;
  `Field::max_length` / `label`; case-insensitive escaped search; typed
  query-string filters; action ability required before per-record checks;
  `make:test`; contract fixture `fixtures/contract/posts.schema.json`;
  authorization oracle `fixtures/authz/admin.toml`.
- New apps register `AdminProvider`.

### Fixed (Codex review of M3)
- Relationship options are available for every operation the user may perform
  with the field (create/update, not only `view_any`); edit forms pass
  `record=<key>` so record-level policies (ownership) decide on that record.
- `max_length` is re-checked on the values hooks produce, before writing.
- Search folds case with the database's `LOWER` on both sides (exact non-ASCII
  text matches on SQLite) and uses table-qualified columns (safe with joined scopes).
- Action names may not reuse CRUD ability names (`view`, `update`, …).
- Record-checked lists and option lookups read all batches from one snapshot
  (`db::begin_snapshot`); option lookups that exhaust their scan budget return a
  422 instead of an incomplete list; search casts columns to text (numeric/UUID
  titles on PostgreSQL).
- Record-checked scans read only keys (bounded) in a snapshot, release it, then
  load records 100 at a time in that order: memory does not grow with row size,
  and policies or label lookups never wait on the scan's connection (works with
  one connection and database-backed policies).
  Key scans keep the query's own projection (100 rows per batch, only keys
  retained), so `DISTINCT`/`ORDER BY` from `scope()` stay valid on PostgreSQL.
  Reloaded records are re-checked against the full query (scope, search,
  filters), so concurrent edits cannot slip non-matching records into results.
- Search on UUID columns matches the displayed (hyphenated) form on SQLite.
- A committed create/update is always reported as a success: relationship
  labels are best effort, and a failing or slow envelope falls back to the key
  alone, within 2 s and within the request's own deadline
  (`http::request_deadline`).
  Optional sliding session renewal is bounded the same way (skipped if it would
  outlast the request).
- Writes lock only target rows found in scope and allowed by the policy
  (PostgreSQL `FOR UPDATE`), then re-check scope and policy under the lock, so
  ownership changed concurrently is seen and hidden records are never locked; policies
  and hooks query `Ctx::conn()`, which is the write's own transaction during
  writes (no second connection needed, no deadlock with small pools).
- Edit schemas keep write-only fields (writable, not readable) as empty,
  `writeOnly` inputs; their values are never returned.
- Envelopes carry data only when the user may view the record (blind updates
  return key and abilities); page offsets are checked for overflow (422).
- Lists exclude (and do not count) records a record-level policy denies `view`;
  `Policy::view_depends_on_record` keeps SQL pagination for scope-only policies.
- `permission:sync` declares action admission permissions; relationship options
  skip policy-denied candidates instead of returning an empty first batch.
- Actions are authorized per record; `Action::requires_permission` declares an
  explicit admission permission instead of inferring it from `allows(…, None)`.
- M2 auth (`rustcms-auth`): admin guard with server-side sessions (hashed ids,
  regeneration, sliding expiry, pruning), synchronizer CSRF + Origin allowlist;
  api guard with personal access tokens (`rcms_…`, hashed, abilities, expiry,
  revocation); deterministic guard selection with no fallback; Argon2id on a
  bounded blocking pool; sign-in throttling per account+IP and per IP;
  credential revocation via `auth_version`; roles/permissions per guard with a
  cached resolver; `RbacService` enforcing anti-escalation rules and the
  last-super-admin invariant under a lock; `Gate`; auth endpoints; commands
  `make:user`, `token:create`, `permission:sync`, `session:prune`.
- Migrations: `personal_access_tokens`, RBAC tables, seeded `super_admin` roles.
- Core: `AppCommand` (provider console commands), `AppBuilder::middleware` and
  `::service`, `AppError::TooManyRequests` with `Retry-After`.
- Testing: `TestClient` with `acting_as`, `login_as`, `with_token`;
  `UserFactory::password`; authorization oracle `fixtures/authz/auth.toml`.
- New apps register `AuthProvider` and ship `config/auth.toml`.
- Docs: security section (authentication, roles & permissions, revocation,
  production checklist), ADR 0005.

### Fixed (Codex review of M2)
- Password change, sign-out everywhere and token issuance re-validate the
  verified credential snapshot under a user-row lock (no revival of revoked
  sessions, no token outliving a concurrent password change).
- Sign-in throttling reserves attempts before Argon2 verification; concurrent
  bursts can no longer exceed the limit.
- Permission cache keyed by RBAC generation: in-flight loads cannot undo eviction.
- A revoked session cookie can obtain a CSRF token and sign in again.
- RBAC changes are serialized by a transaction-scoped lock (`rbac::lock`) and
  authorize with the actor's grants re-read under it: concurrent role edits
  cannot restore revoked permissions, and promotions racing removals cannot
  bypass the last-super-admin rule (PostgreSQL READ COMMITTED).
- A session ended by a concurrent request no longer turns the other request's
  committed response into a 500 (conditional renewal, no revival).
- Argon2 permits move into the blocking work: cancelled requests cannot start
  more concurrent hashes than configured.
- `make:user` is atomic (`RbacService::create_user`): a failed super-admin
  assignment leaves no half-created account.
- Anonymous session creation (`GET csrf`) is throttled per IP and globally, and
  anonymous sessions have a fixed 15-minute lifetime (never extended): storage
  cannot be flooded.
- Credential restamps never re-create a session signed out concurrently:
  sessions carry a `lineage`; re-issue and sign-out take a per-lineage lock, and
  sign-out ends the whole lineage (any number of concurrent re-issues).
- Session renewal failures no longer replace a committed handler response.
- Rate limiter memory is bounded: oversized emails/passwords rejected early,
  keys hashed to a fixed size, expired windows reclaimed, capacity capped.
- SQLite write transactions use `BEGIN IMMEDIATE` (`db::begin_write`): concurrent
  writers wait instead of failing with "database is locked".
- M1 core: layered configuration (`config/*.toml`, `.env`, env vars) with required
  `APP_KEY`; `AppBuilder`, `ServiceProvider` (`register`/`boot`), `Inject<T>`;
  HTTP stack (request id, tracing, trusted-proxy client IP, timeout, body limit,
  JSON 404/405, `/health`); `rustcms::Json`; `Console` with `serve`, `migrate*`,
  `key:generate`.
- `rustcms-migration`: `users` and `sessions` tables.
- `rustcms-testing`: per-test SQLite/PostgreSQL databases (PostgreSQL templates),
  `db_test!`, `TestApp`, `Factory`/`UserFactory`.
- `rustcms` facade crate and `rustcms` CLI: `new`, `make:model`, `make:migration`,
  `docs:cli`, forwarding of app commands.
- `examples/demo` generated with `rustcms new`.

### Fixed (Codex review)
- Migrations (framework and generated) are transactional on SQLite too, so a failed
  run leaves no partial schema and can be retried.
- `key:generate` replaces every dotenv form of `APP_KEY` (`export`, spaces, quotes,
  duplicates) instead of appending a definition that the old one shadows.
- The body limit wraps every request body (Content-Length and chunked), not only
  extractors; all 413 responses use the JSON error format.
- Docs: getting started, configuration, basics, generated CLI reference, ADR 0004.
- M0: workspace, CI (lint, features, tests on SQLite + PostgreSQL, docs, cargo-deny).
- `rustcms-core`: `AppError` with Laravel-compatible JSON errors, `Principal` with
  super-admin bypass bounded by token abilities.
- `rustcms-admin`: strict wire format codec, `Resource` trait, type-erased generic CRUD
  engine (key strategies, field permissions, scope, relationships, actions, hooks in
  transactions, constraint mapping, request limits).
- Docs: mdBook skeleton, admin protocol, ADR 0001–0003.
