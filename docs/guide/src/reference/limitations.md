# Limitations

What RustCMS does not do yet, the fixed limits it enforces, and behaviours to
know before relying on it. Read it when you plan an application, so a limit
does not surprise you in production.

## Not supported yet

- Pivot columns in the record's own form (a `BelongsToManySelect` links with
  the pivot's database defaults; edit pivot columns in the relation manager),
  and a `BelongsToManySelect` for relations with required pivot fields or
  records with more than 100 links; belongs-to-many to the same resource, two
  relations over one pivot table, or to a resource declaring `lock_parent`.
- Excel files; dashboard widgets with sums or averages (widgets count or list
  records); charts in time zones other than UTC.
- Email verification flows (invitations mark the address verified).
- Mail rate limits shared between instances (`max_per_minute` counts per
  process); DKIM signing; attachments.
- Settings of kinds other than text, textarea, integer (`i32`), boolean, select,
  key/value, date, secret and media (one file, or a list) (store larger numbers
  or decimals as text).
- Translations of content (only the interface is translated: English, Vietnamese).
- Composite primary keys; sorting by related columns.
- Column types without a wire kind: `smallint`, `tinyint`, unsigned integers,
  `time`, binary, enums and arrays cannot be admin fields
  ([Wire format](wire-format.md#unsupported-column-types)).
- Server-side image resizing (the Media grid scales the originals in the
  browser), range requests, presigned or public media URLs (files are served
  only through the panel, after an authorization check; add your own route
  for public files).
- MariaDB; Redis Cluster.
- Idempotency keys outside resource creates (relation creates included),
  actions, media uploads, CSV import uploads and background export requests.
- Managing API-guard roles from the panel (use `rustcms role:assign`).
- Cron expressions in a time zone other than UTC.

## Fixed limits

| Limit | Value | Where |
|---|---|---|
| Rows a list, export or dashboard count checks one by one (per-record policies, trashed lists, SQLite dated counts) | 10,000 | beyond: 422 `too_many_matches` for lists and exports at once, a `too_many` widget on the dashboard ([Admin API](admin-api.md#limits)) |
| Keys per action or lookup | 500 (100 for a field's options lookup) | [Admin API](admin-api.md#limits) |
| Keys per attach or detach | 100 | [Relationships](../admin/relationships.md) |
| `per_page` | 100 | [Admin API](admin-api.md#lists) |
| Recipients of one mail | 50 | [Mail](../digging-deeper/mail.md) |
| An encrypted value (its JSON) | 32 KiB; never searched, sorted or filtered | [Encrypted columns](../digging-deeper/encryption.md#pitfalls-and-limits) |
| Live idempotency keys per user | 1,000 (429 beyond) | [Admin API](admin-api.md#retrying-writes-safely) |

A background export of more than 10,000 rows currently fails and is
reported as `revoked` (as if access had changed) instead of a clear limit
error: narrow the export with filters.

## Writes in flight

Signing out one session, or revoking one API token, does not abort a change
that the same credential already started. Sign-out everywhere, password
changes and disabling a user apply from the next request, and changes made
through `RbacService` re-check the credential under lock. A resource write
keeps the permissions it started with.

## Background work

- Schedules run only in `rustcms serve`, never in console commands,
  `queue:work` or tests ([Scheduling](../digging-deeper/scheduling.md)).
- Some built-in clean-ups are not schedules yet: the hourly auth clean-up
  (expired sessions, JWT sign-ins, reset links, old audit entries) runs in
  every process that boots the application, including `db:seed` and other
  console commands, and media, export and import pruning run in a loop of the
  admin provider. With Redis, a lease keeps one instance running each
  clean-up at a time; without Redis every instance runs them, and a run that
  outlasts its lease can overlap the next. They are written to stay correct
  when that happens.

## Databases

- **SQLite decimals** are stored as REAL: values are shown at the column
  scale, but precision is that of a 64-bit float. Use PostgreSQL or MySQL for
  money.
- **Search** compares `LOWER(column) LIKE LOWER(pattern)`: case-insensitive
  for all of Unicode on PostgreSQL, for ASCII only on SQLite. It uses no index;
  full-text search is out of scope.
- **Named locks on MySQL**: `rustcms::db::xact_lock` works only with the names
  the framework creates (`rbac`, `lineage:<slot>`, `idem:<slot>`); other names
  fail on MySQL ([Database schema](database-schema.md#rustcms_locks)).
- MySQL specifics (collation, non-transactional DDL) are in
  [Database](../basics/database.md#mysql).

## Deployments

One instance, or several sharing one standalone Redis and shared media
storage ([Several instances](../digging-deeper/instances.md)).
Password-hashing and upload concurrency limits are per instance; runtime
branding is cached per instance, and each instance runs its own realtime
hubs.

## Size

The panel ships as one JavaScript file of about 1.36 MB (about 417 KB
gzipped) and one stylesheet of about 42 KB (about 8 KB gzipped), embedded in
the binary. The server sends them uncompressed, with a one-year immutable
cache: put a compressing reverse proxy in front if the first load matters.
Binary sizes depend on the features and drivers you build with; measure your
own release build.
