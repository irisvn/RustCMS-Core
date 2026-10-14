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
  redacted by declaration; `audit:prune` and hourly retention
  ([Audit log](digging-deeper/audit-log.md)).
- **Databases**: MySQL 8.4 alongside SQLite and PostgreSQL.
- **APIs**: `ApiRoutes` with declared access, `Validated<T>`, `Pagination`,
  `Throttle<N>`, `make:api`; JWT access tokens with rotating refresh tokens.
- **Operations**: several instances with Redis (shared rate limits, periodic
  jobs mostly on one instance); S3-compatible storage.

## 0.1.0-alpha.0

- Resources rendered as an admin panel: forms, tables, filters, actions,
  `belongs_to` relationships, field and record authorization, hooks.
- Admin sessions with CSRF and origin checks; API tokens with abilities;
  roles and permissions per guard; users, roles and account pages.
- Application layer: configuration, providers, container, HTTP stack,
  migrations, console and generators, testing kit.

See the [upgrade guide](upgrade.md) when moving between versions.
