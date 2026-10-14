# Limitations

What RustCMS does not do yet, and behaviours to know before relying on it.

## Not supported yet

- Many-to-many relationship editing in the panel (attach/detach through a
  pivot table): use a resource for the pivot table, or an action.
- Actions with a form (modal input), import and export, dashboard widgets.
- Settings pages, an audit log viewer in the panel, mail, password reset and email verification.
- Translations of content (only the interface is translated: English, Vietnamese).
- Composite primary keys; sorting by related columns.
- Image resizing and thumbnails, range requests, public media URLs (add your
  own route).
- MariaDB; Redis Cluster.
- Idempotency keys outside resource creates, actions and media uploads.
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
