# 0010 — MySQL as a third database

- Status: accepted
- Date: 2026-10-15

## Context

0.2 adds MySQL. rustcms relies on database behaviour that PostgreSQL and SQLite
give for free and MySQL does not: `READ COMMITTED` reads after a lock,
transaction-scoped advisory locks, exact string comparison, instants past 2038
with microseconds, transactional DDL. A spike on MySQL 8.4 with the pinned
SeaORM/sqlx confirmed each gap (and that `ON CONFLICT DO NOTHING` renders as
invalid SQL, and that sqlx reports one affected row for a no-op upsert).

## Decision

- **Version:** MySQL 8.4 LTS (8.0 is end-of-life). MariaDB is not supported.
- **Strings:** the database, every string column and every connection use
  `utf8mb4_0900_bin` (case-, accent- and trailing-space-sensitive, like
  PostgreSQL). Tables must be InnoDB and `sql_mode` strict without
  `NO_BACKSLASH_ESCAPES`; connections must use UTC and that collation. These are checked
  when the app builds and before `migrate`; a mismatch refuses to start and
  prints the SQL that fixes it. Checking the physical columns (not only the
  database default) is what makes application `scope()` comparisons safe.
- **Time:** connections are forced to UTC; instants are `DATETIME(6)` (UTC wall
  time), through migration helpers (`instant`, `instant_now`) that keep the
  PostgreSQL and SQLite DDL unchanged.
- **Isolation:** write transactions run `READ COMMITTED`, set per transaction
  (session hooks did not reach every pooled connection in the spike).
- **Named locks:** `rustcms_core::db::xact_lock(txn, name)` replaces the
  PostgreSQL-only advisory locks: PostgreSQL keeps an advisory lock, MySQL
  locks a row of the preseeded `rustcms_locks` table `FOR UPDATE` (no inserts at
  lock time, so no duplicate-key deadlocks; not `GET_LOCK`, which is session
  scoped and leaks on error paths), SQLite relies on its write lock. Hashed
  namespaces (`lineage`, `idem`) use 1,024 slots each, so the table is bounded;
  values that share a slot only wait for each other.
- **Lock order:** a transaction takes at most one named lock, as its first
  lock, before any row lock. Row locks on MySQL are point locks in key order
  (InnoDB locks while scanning, so `ORDER BY` alone does not fix the order).
- **Lock waits:** `innodb_lock_wait_timeout` is a session setting: every write
  transaction resets it (10 s); an idempotency claim shortens it only for the
  claim and restores it right after. A request cancelled during the claim can
  leave the shorter wait (at least one second) on its pooled connection until
  that connection's next write transaction resets it. Lock wait timeouts and deadlocks
  (MySQL 1205/1213/3572 by error number; PostgreSQL 55P03/40P01/40001 by
  SQLSTATE; the message only as a fallback) map to 409 `write_conflict`; the
  whole transaction is rolled back.
- **DDL:** MySQL commits each statement (and with it what the migration wrote
  before), so framework migrations are written to be re-run, up and down
  (tables, indexes, columns and seeds skip what exists or is gone), and the demo
  alters tables in place instead of rebuilding them.
- **Tests:** `db_test!` runs on the three databases; MySQL test databases are
  copied from a migrated template (tables from `SHOW CREATE TABLE`, rows).
  `RUSTCMS_TEST_REQUIRE` turns a skipped server into a failure in CI.

## Consequences

- Same behaviour on the three databases for everything rustcms checks;
  remaining differences (text sort order, `varchar(255)` for bare strings,
  64 KiB `TEXT`, no literal JSON defaults) are documented.
- An existing MySQL database with another collation needs a one-time
  conversion before rustcms starts.
- Migrations are not atomic on MySQL: stop writers while migrating.
