# Support boundary (0.1)

**Supported:** PostgreSQL and SQLite; generated CRUD with `belongs_to`
relationships; record and field authorization; hooks and actions; the embedded
admin panel (vocabulary 0.1: text, textarea, number, select, toggle, checkbox,
checkbox list, date, date-time, hidden; section and grid; text, badge, boolean
and date columns; select, ternary and date-range filters); users, roles and
account pages; admin session and API tokens; roles and permissions per guard;
generators (`make:model`, `make:migration`, `make:resource --generate`,
`make:policy`, `make:test`) and `route:list`; single-instance deployments (rate
limits and the permission cache are in memory).

Since 0.2: soft deletes, idempotency keys for creates and actions, tabs and
key/value inputs (M7); media library on local or S3-compatible storage, file
uploads and rich text documents (M8).

**Not supported yet:** idempotency keys outside resource creates and actions
(system creates are protected by unique email/name; token issuance and password
changes are not keyed), MySQL, JWT, email verification,
managing api-guard roles from an admin session (use `rustcms role:assign`), many-to-many
relationship editing in the panel, modal-form actions, composite primary keys, sorting by
related columns, image resizing/thumbnails, range requests and public media
URLs (apps add their own route), password reset, multiple
instances sharing caches, custom field types.

## Database notes

- **SQLite decimals** are stored as REAL. Values are rendered with the column
  scale, but precision is that of a 64-bit float. Use PostgreSQL for money.
- **Search** compares `LOWER(column) LIKE LOWER(pattern)`: case-insensitive for
  all of Unicode on PostgreSQL, for ASCII only on SQLite (exact text always
  matches). No index use on large tables; full-text search is out of scope.

## Size and footprint

The demo's release binary (panel, auth, admin, SQLite and PostgreSQL drivers)
is about 23 MB, 18 MB stripped, on macOS arm64 with Rust 1.99
(`cargo build --release -p demo`, then `strip`). The panel's JavaScript is about
0.55 MB (175 KB gzipped).

## Roadmap

0.2: JWT guard with rotating refresh tokens, MySQL, rich text and uploads,
tabs and key-value inputs, custom field bundles, soft deletes, relationship
managers, Redis for cache and rate limits, idempotency keys.
Later: events, settings, queues, mail, password reset and email verification,
audit log, content (pages, menus) with a headless API, i18n, dashboard widgets,
plugins.
