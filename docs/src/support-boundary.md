# Support boundary (0.1)

**Supported:** PostgreSQL and SQLite; generated CRUD with `belongs_to`
relationships; record and field authorization; hooks and actions; admin
session and API tokens; roles and permissions per guard; single-instance
deployments (rate limits and the permission cache are in memory).

**Not supported in 0.1:** MySQL, JWT, password reset and email verification,
managing api-guard roles from an admin session (use the console), composite primary keys, sorting by
related columns, soft deletes, uploads, rich text, password reset, multiple
instances sharing caches, custom field types.

## Database notes

- **SQLite decimals** are stored as REAL. Values are rendered with the column
  scale, but precision is that of a 64-bit float. Use PostgreSQL for money.
- **Search** uses `LIKE`, which is case-sensitive on PostgreSQL (until M3 adds
  `ILIKE`).
