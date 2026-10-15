# Release notes

RustCMS is in pre-release. The current published version is
**0.1.0-alpha.0**; everything below "Unreleased" is in the next release.

## Unreleased

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
