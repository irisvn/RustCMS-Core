# Release notes

RustCMS is in pre-release. The current version is **0.7.0**; 0.1.0-alpha.0
was the first published one.

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
