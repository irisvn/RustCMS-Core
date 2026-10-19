# Limitations

What RustCMS does not do yet, and behaviours to know before relying on it.

## Not supported yet

- Choosing belongs-to-many links in the record's own form (use the relation
  manager on its page); belongs-to-many to the same resource, two relations
  over one pivot table, or to a resource declaring `lock_parent`.
- Excel files; dashboard widgets other than counts (no sums or averages), charts in time zones other than UTC.
- Email verification flows (invitations mark the address verified).
- Mail rate limits shared between instances (`max_per_minute` counts per process); DKIM signing; attachments.
- Settings of kinds other than text, integer (`i32`), boolean, select, key/value,
  date and secret (store larger numbers or decimals as text).
- Translations of content (only the interface is translated: English, Vietnamese).
- Composite primary keys; sorting by related columns.
- Image resizing and thumbnails, range requests, public media URLs (add your
  own route).
- MariaDB; Redis Cluster.
- Idempotency keys outside resource creates (relation creates included), actions, media uploads,
  CSV import uploads and background export requests.
- Managing API-guard roles from the panel (use `rustcms role:assign`).

## Writes in flight

Signing out one session, or revoking one API token, does not abort a change
that the same credential already started. Sign-out everywhere, password
changes and disabling a user apply from the next request, and changes made
through `RbacService` re-check the credential under lock. A resource write
keeps the permissions it started with.

## Databases

- **SQLite decimals** are stored as REAL: values are shown at the column
  scale, but precision is that of a 64-bit float. Use PostgreSQL or MySQL for
  money.
- **Search** compares `LOWER(column) LIKE LOWER(pattern)`: case-insensitive
  for all of Unicode on PostgreSQL, for ASCII only on SQLite. It uses no index;
  full-text search is out of scope.
- MySQL specifics (collation, non-transactional DDL) are in
  [Database](../basics/database.md#mysql).

## Deployments

One instance, or several sharing one standalone Redis and shared media
storage ([Several instances](../digging-deeper/instances.md)).
Password-hashing and upload concurrency limits are per instance.

## Size

A release binary with the panel and the three database drivers is about
29 MB (23 MB stripped); the panel's JavaScript is about 0.55 MB (175 KB gzipped).
