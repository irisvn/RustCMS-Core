# Changelog

All notable changes are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added (0.4 — W1)
- `AdminRoutes` / `AdminAccess` (ADR 0026): JSON routes for the panel's own
  pages under `{admin_path}/api/x`, on the admin session (Origin and CSRF on
  writes), each with `authenticated()` or `can(permission)` checked before
  extractors; no public access; API tokens and JWTs get 401.
  `AuthProvider::admin_permissions` declares their permissions in the admin
  guard; `register` lists them in `route:list`.
- `rustcms_testing::admin_routes::assert_access`: an access table per route
  and actor that also fails on a route without a case.

### Added (0.4 — W9)
- `TestApp::new` / `with_config` take any `FnOnce(Config) -> AppBuilder`
  (closures binding test doubles; function items still work).
- `RUSTCMS_TEST_BACKENDS=mysql` (any of `sqlite,postgres,mysql`): `db_test!`
  runs only those, each then required; empty or unknown names, or a required
  backend left out, fail.

### Added (0.4 — W7)
- Configuration from files: every well-known variable and every
  `RUSTCMS_<PATH>` also reads `<VAR>_FILE` (Docker/Kubernetes secrets), in its
  variable's place; both set, a missing, empty, non-UTF-8 or over-64-KiB file
  fail boot without quoting the content; one trailing line break removed.

### Added
- Guide: *Database schema* reference (users, roles, permissions and their
  assignments, sessions, tokens, JWT, one-time links, every other framework
  table), linked from *Roles & permissions*.

### Fixed
- `rustcms self-update --version v0.3.1` (going back to a release that prints
  no target in `--version`) was refused; such a command is checked by version.

## [0.3.2] — 2026-10-16

### Added
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

### Changed
- The application is found by parsing `Cargo.toml` (`[package.metadata.rustcms]`),
  not by searching its text.
- `make:command` refuses the names of the command's own commands; `rustcms --
  <command>` always reaches the application.
- CI runs every test of the command (unit, `self-update`, generated app) on
  Linux and macOS.

## [0.3.1] — 2026-10-16

### Added
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

### Changed
- `rustcms new` depends on the Git tag of RustCMS-Core matching the command's
  version (`rustcms = { git = …, tag = "vX.Y.Z" }`); a local checkout only with
  `--rustcms-path`. Paths are written as proper TOML strings.
- The repository is irisvn/RustCMS-Core; the guide links to it.
- `.gitattributes`: LF in every checkout (generators insert code at
  `\n`-terminated markers in the embedded templates).

### Fixed
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

## [0.3.0] — 2026-10-26

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

### Changed (docs)
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

### Changed (demo)
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

### Changed
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
