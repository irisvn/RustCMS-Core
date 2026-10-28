# Database & migrations

RustCMS stores everything in one SQL database through
[SeaORM](https://www.sea-ql.org/SeaORM/) 2.0. SQLite, PostgreSQL and MySQL 8.4
are supported (MariaDB is not). This page covers models, migrations, seeders,
factories, and the transactions and locks you need when several requests write
the same rows.

## Quick start

Set the connection in `.env` ([Configuration](../getting-started/configuration.md)):

```sh
DATABASE_URL=sqlite://database.sqlite?mode=rwc
# DATABASE_URL=postgres://postgres:postgres@localhost:5432/blog
# DATABASE_URL=mysql://root:secret@localhost:3306/blog
```

Generate a model and the migration that creates its table, then run it:

```sh
rustcms make:model Post -m      # src/models/post.rs + src/database/migrations/m…_create_posts_table.rs
rustcms migrate
```

The generator writes only the key and the timestamps; add your columns to both
files:

```rust
// src/models/post.rs
use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "posts")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
```

Query models with SeaORM, using the pool from the state:

```rust
let posts = post::Entity::find().filter(post::Column::Title.contains("rust")).all(state.db()).await?;
```

## Models

Models are SeaORM entities in `src/models/`, one file per table. The demo's
`posts` model, after a few migrations:

<<< @/../../../examples/demo/src/models/post.rs#post-model [src/models/post.rs]

The framework's own tables come with the framework's migrations, which run
before yours. Their entities live in `rustcms::models`: `user`, `audit_log`,
`job`, `job_receipt`, `job_run`, `schedule_run`, `schedule_slot`,
`schedule_state` and `setting`. Roles, permissions, sessions and tokens are in
`rustcms::auth::entities` ([Database schema](../reference/database-schema.md)).
Change them only through their services (`RbacService` for roles and
permissions), never with direct writes.

Every connection opened by `rustcms::db::connect` (the pool of `serve`, the
console and the tests) is set up the same way:

- **SQLite:** foreign keys on, WAL journal, 5 s busy timeout.
- **MySQL:** UTC time zone and the `utf8mb4_0900_bin` collation
  (`rustcms::db::MYSQL_COLLATION`), whatever the URL says. See [MySQL](#mysql).
- Pool size: `database.max_connections` (10). `database.log_statements = true`
  logs every SQL statement at debug level.

## Migrations

`src/database/migrations/mod.rs` lists the framework's migrations
(`rustcms::migration::framework()`) and then yours.
`rustcms make:migration <name>` adds a file and registers it there:

<<< @/../../../examples/demo/src/database/migrations/mod.rs#migrator [src/database/migrations/mod.rs]

A migration creating a table, as `make:migration create_posts_table` writes it:

<<< @/../../../examples/demo/src/database/migrations/m20261011_155203_create_posts_table.rs#create-posts [src/database/migrations/m20261011_155203_create_posts_table.rs]

A migration adding a column:

<<< @/../../../examples/demo/src/database/migrations/m20261013_000001_add_deleted_at_to_posts.rs#add-column [src/database/migrations/m20261013_000001_add_deleted_at_to_posts.rs]

Keep `use_transaction() -> Some(true)` in every migration, as the generators
write it. The schema change and the migration record then commit together,
also on SQLite (whose default is no transaction): a failed migration leaves no
partial schema and can run again. Leave it out only for a statement that cannot
run in a transaction. MySQL is different: each DDL statement commits on its
own, so write migrations that can run again (the helpers below skip what
exists).

### Commands

| Command | Does |
|---|---|
| `rustcms migrate` | applies pending migrations |
| `rustcms migrate --allow-newer` | also accepts migrations applied by a newer binary (they are logged and left alone) |
| `rustcms migrate:status` | lists each migration as `Applied`, `Pending` or `Newer` (applied, unknown to this binary) |
| `rustcms migrate:rollback --step N` | undoes the last N (default 1), in the migrator's order |
| `rustcms migrate:refresh [--force]` | undoes every migration, then applies all |
| `rustcms migrate:fresh [--force] [--seed]` | drops every table, applies all, then runs the `database` seeder with `--seed` |

- `migrate:refresh` and `migrate:fresh` refuse in production without
  `--force`.
- `migrate`, `migrate:rollback`, `migrate:refresh` and `migrate:fresh` run on a
  single connection (`rustcms::db::connect_for_migrations`): with a pool, other
  SQLite connections could still see the old schema between steps.
- `migrate:fresh` turns SQLite foreign keys off while it drops the tables, then
  back on.
- `migrate` refuses migrations it does not know, unless `--allow-newer` or
  `[database] allow_newer_migrations = true`. That is safe only when the newer
  migrations are additive (new tables, nullable columns).

`migrate:rollback` undoes migrations last first **in the migrator's order**
(the framework's, then yours), not in the order they were applied: after a
RustCMS upgrade that added framework migrations, `--step 1` undoes your newest
migration, not the framework's. To run an older binary on a newer schema, keep
the migrations and use `migrate --allow-newer`: see
[Upgrading](../getting-started/deployment.md#upgrading).

Never edit a migration that has run in production; add a new one. A migration
that cannot be reversed must say so in its `down` and in its doc comment.

### Column helpers

`rustcms::migration` re-exports SeaORM's migration prelude and adds helpers
that build the same table on every database and skip what a previous run did:

| Helper | Does |
|---|---|
| `create_table(manager, statement)` | `IF NOT EXISTS`; on MySQL: InnoDB, `utf8mb4_0900_bin` |
| `create_index(manager, table, statement)` | skips an index the table has (MySQL ignores `IF NOT EXISTS` on indexes) |
| `add_column(manager, table, column)` | skips a column that exists |
| `drop_column(manager, table, column)` | skips a column that is gone |
| `drop_table(manager, table)` | skips a table that is gone |
| `.instant(manager)` | an instant: `TIMESTAMPTZ` on PostgreSQL, `DATETIME(6)` (UTC) on MySQL |
| `.instant_now(manager)` | the same, defaulting to the current time |
| `.local_date_time(manager)` | date and time without zone, microseconds on MySQL |

`instant`, `instant_now` and `local_date_time` come from the `ColumnDefExt`
trait, which `use rustcms::migration::*` brings in.

## MySQL

Create the database with the collation RustCMS needs, so text compares as on
PostgreSQL (case, accents and trailing spaces all count):

```sql
CREATE DATABASE app CHARACTER SET utf8mb4 COLLATE utf8mb4_0900_bin;
```

The app builder (so `serve` and every command that boots the application),
`migrate`, `migrate:rollback`, `migrate:refresh` and `migrate:fresh` check the
database first (`rustcms::db::check`) and refuse to start when:

- the database default, or any string column, uses another character set or
  collation (`migrate:refresh` and `migrate:fresh` check only the database
  itself, since they recreate every table);
- a table is not InnoDB;
- `sql_mode` (global or session) is not strict (`STRICT_TRANS_TABLES`) or has
  `NO_BACKSLASH_ESCAPES`;
- the connection is not in UTC with that collation (pools opened with
  `rustcms::db::connect` always are).

The error lists the SQL that fixes each problem. `migrate:status` only reads
and does not run the check.

Differences to know:

- **DDL is not transactional:** each statement commits. A migration that fails
  half-way is retried on a partly changed schema. Use the helpers above, which
  skip what exists, and stop writers while migrating.
- **Column types:** a bare `String` field is `varchar(255)` (PostgreSQL:
  unbounded); `Text` holds 64 KiB. JSON columns accept no literal default
  (only an expression default such as `('[]')`).
- **Sort order** of text is binary (code points), not a locale.
- **Instants** are `DATETIME(6)` in UTC (MySQL's `TIMESTAMP` stops in 2038 and
  drops fractions); fractions below a microsecond are truncated.
- **Write transactions** run in `READ COMMITTED`, as on PostgreSQL, and wait at
  most 10 s for a row lock (see below).
- **Named locks** (`xact_lock`) accept only the framework's own names: lock
  rows instead (see [Named locks](#named-locks)).

## Transactions and locks

Use `rustcms::db::begin_write` for every transaction that writes. It behaves
the same on the three databases:

| Database | `begin_write` |
|---|---|
| SQLite | `BEGIN IMMEDIATE`: takes the write lock at once, so a concurrent writer waits for the busy timeout instead of failing with "database is locked" when a read turns into a write |
| PostgreSQL | a plain transaction (`READ COMMITTED`) |
| MySQL | `READ COMMITTED`, and the session's row lock wait reset to `MYSQL_LOCK_WAIT_SECONDS` (10) |

A transaction that waits too long for a lock, or that deadlocks, fails and
becomes a 409 `write_conflict` with `reason: "busy"`
([Errors](errors.md#database-errors)): nothing was saved and the client may
retry.

To keep a rule that reads other rows ("no more enrollments than seats"), lock
the parent row first. The LMS example locks the course, then checks and writes
in the same transaction:

<<< @/../../../examples/lms/src/services/enrollment_service.rs#lock-course [src/services/enrollment_service.rs]

<<< @/../../../examples/lms/src/services/enrollment_service.rs#enroll [src/services/enrollment_service.rs]

SQLite has no row locks: the write transaction already holds the whole
database, which is why `lock_course` skips `lock_exclusive` there. When a write
locks rows of several tables, lock parents before children, and keys in key
order, as the admin panel does ([Resources](../admin/resources.md)).

| Function | Does |
|---|---|
| `begin_write(db)` | a write transaction, as above |
| `begin_snapshot(db)` | a read-only transaction whose reads all see one snapshot (`REPEATABLE READ` on PostgreSQL and MySQL; on SQLite in WAL mode a read transaction keeps the snapshot of its first read), for scans in several queries |
| `set_lock_wait(&txn, seconds)` | MySQL only: this session's row lock wait (at least 1 s); `begin_write` restores the default |

### Named locks

RustCMS serializes some of its own work with `rustcms::db::xact_lock(&txn, name)`
(advisory locks on PostgreSQL, rows of the `rustcms_locks` table on MySQL,
nothing extra on SQLite). It is public, but it is not a general lock for
application code:

::: warning On MySQL, only the framework's names exist
On MySQL, `xact_lock` locks a row of `rustcms_locks`, which holds only the
framework's own names (`rbac`, and the slots of its `lineage` and `idem`
namespaces, the names `rustcms::db::lock_slot` produces). Any other name fails
there with ``lock `…` is missing from rustcms_locks`` (a 500), while the same
code works on PostgreSQL and SQLite. An application cannot add names, and the
framework's names are not part of the public API: do not borrow them.
:::

In application code, serialize with **row locks inside a write transaction**,
as `lock_course` does above: lock the row the rule is about (a parent, an
account, a counter row you create for the purpose) with `lock_exclusive()` on
PostgreSQL and MySQL, and rely on `begin_write`'s database lock on SQLite.
This works the same on the three databases, and the lock ends with the
transaction.

## Seeding

Seeders fill the database with the records an application needs, or shows
off. They live in `src/database/seeders/`: `DatabaseSeeder` (named `database`)
calls the others in order with `rustcms::database::call`, and every seeder is
registered on the app builder. `rustcms make:seeder CourseSeeder` writes the
file, registers it and adds it to `DatabaseSeeder`.

```rust
use rustcms::database::Seeder;
use rustcms::{AppState, async_trait};

pub struct CourseSeeder;

#[async_trait]
impl Seeder for CourseSeeder {
    fn name(&self) -> &'static str {
        "course" // `rustcms db:seed --class course`
    }

    async fn run(&self, state: &AppState) -> anyhow::Result<()> {
        // Find by a natural key before creating: seeders run again.
        Ok(())
    }
}
```

The demo registers its seeders in `AppServiceProvider::register`:

<<< @/../../../examples/demo/src/providers/app_service_provider.rs#seeders [src/providers/app_service_provider.rs]

A seeder that creates roles goes through `RbacService`, and finds each role
before creating it, so it can run again:

<<< @/../../../examples/lms/src/database/seeders/role_seeder.rs#role-seeder [src/database/seeders/role_seeder.rs]

| Command | Does |
|---|---|
| `rustcms db:seed` | runs the `database` seeder (`rustcms::database::DEFAULT_SEEDER`) |
| `rustcms db:seed --class course` | runs one seeder |
| `rustcms migrate:fresh --seed` | drops every table, migrates, seeds |

- **Names:** 1–64 characters of `a-z`, `0-9` and `_`. `app.seeder(…)` panics on
  an invalid name or a name registered twice. `AppBuilder::seeders()` returns
  what is registered.
- **Production:** `db:seed` refuses without `--force`. A seeder that creates
  accounts with known passwords must refuse in production itself, whatever
  `--force` says:

  <<< @/../../../examples/demo/src/database/seeders/user_seeder.rs#refuse-in-production [src/database/seeders/user_seeder.rs]

- **No shared transaction:** a seeder that fails halfway leaves what it wrote.
  Write seeders that complete on the next run (find rows by a natural key),
  and write through your services (which open their own transactions) so the
  application's rules hold.
- `db:seed` boots the whole application first, so every provider's `boot`
  runs (see [Console](console.md)).

## Factories

A factory builds records from a definition, for seeders and tests (Laravel's
model factories). `rustcms make:factory Course` writes
`src/database/factories/course_factory.rs`; fill in `definition`:

<<< @/../../../examples/lms/src/database/factories/course_factory.rs#course-factory [src/database/factories/course_factory.rs]

| Method of `rustcms::database::Factory` | Does |
|---|---|
| `definition(&self, n)` | the column values of record number `n`; required |
| `try_create(db)` / `try_create_many(db, count)` | inserts, returning `Result<_, DbErr>` (seeders) |
| `create(db)` / `create_many(db, count)` | inserts, panicking on a database error (tests) |

`n` is unique per process, so use it in unique columns (`slug`, `email`).
Builder methods such as `capacity(1)` above are yours to add. `db` is any
SeaORM connection, a transaction included. In tests,
`rustcms_testing::Factory` is the same trait ([Testing](testing.md#factories)).

## Pitfalls

- Writing without `begin_write` on SQLite: a transaction that reads and then
  writes can fail with "database is locked" under load.
- Using `xact_lock` with a name of your own: it works on PostgreSQL and SQLite
  and fails on MySQL. Lock rows instead (see [Named locks](#named-locks)).
- A `String` column is `varchar(255)` on MySQL: longer values fail there only.
- `migrate:rollback --step N` follows the migrator's order, not the order
  migrations ran.

## Related pages

- [Configuration](../getting-started/configuration.md) (`[database]` keys)
- [Console](console.md)
- [Testing](testing.md)
- [Deployment: upgrading](../getting-started/deployment.md#upgrading)
- [Database schema](../reference/database-schema.md)
- [Limitations](../reference/limitations.md)
