# Release notes

RustCMS is in pre-release. The current version is **0.4.0** ("panel
platform"); 0.1.0-alpha.0 was the first published one.

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
- **Example**: `examples/lms` grows news (categories, tags, scheduled
  articles), editor and viewer roles, lessons moved up and down, a dashboard,
  welcome mails and instructor invitations ([An LMS back office](examples/lms.md)).
- **Operations**: running the queue, retention, backup and restore,
  rotating `APP_KEY`, running mail ([Operations](operations/queue.md)).
- **Permission matrix**: the role page lays resource permissions out as
  resource × ability ([Users and roles](admin/users-roles.md)).
- **Example**: `examples/lms`, a small LMS back office (seats under a lock,
  instructor scopes, a job per enrollment) ([Example](examples/lms.md)).
- **Databases**: MySQL 8.4 alongside SQLite and PostgreSQL.
- **APIs**: `ApiRoutes` with declared access, `Validated<T>`, `Pagination`,
  `Throttle<N>`, `make:api`; JWT access tokens with rotating refresh tokens.
- **Operations**: several instances with Redis (shared rate limits, periodic
  jobs mostly on one instance); S3-compatible storage tested against a real
  server, Cloudflare R2 setup, bounded bucket timeouts
  ([File storage](digging-deeper/storage.md)).

## 0.1.0-alpha.0

- Resources rendered as an admin panel: forms, tables, filters, actions,
  `belongs_to` relationships, field and record authorization, hooks.
- Admin sessions with CSRF and origin checks; API tokens with abilities;
  roles and permissions per guard; users, roles and account pages.
- Application layer: configuration, providers, container, HTTP stack,
  migrations, console and generators, testing kit.

See the [upgrade guide](upgrade.md) when moving between versions.
