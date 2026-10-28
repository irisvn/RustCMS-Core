# Database & migrations

RustCMS uses [SeaORM](https://www.sea-ql.org/SeaORM/) 2.0. SQLite, PostgreSQL
and MySQL 8.4 are supported (MariaDB is not). Configure the connection with
`DATABASE_URL` ([Configuration](../getting-started/configuration.md)).

## Models

Models are SeaORM entities in `src/models/`. `rustcms make:model Post -m`
writes the entity and a migration creating its table:

```rust
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "posts")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub title: String,
    pub published_at: Option<DateTimeUtc>,
    pub created_at: DateTimeUtc,
}
```

Query them with SeaORM, using the pool from the state:

```rust
let posts = post::Entity::find().filter(post::Column::Title.contains("rust")).all(app.db()).await?;
```

The framework's own tables (`users`, `sessions`, roles and permissions,
media, …) come with the framework's migrations, which run before yours.
`rustcms::models::user` is the users entity.

- SQLite connections always enable foreign keys, WAL and a 5s busy timeout.
- MySQL connections always use UTC and the `utf8mb4_0900_bin` collation; write
  transactions run in `READ COMMITTED`, as on PostgreSQL. See
  [MySQL](#mysql) for the database settings rustcms checks at startup.

## MySQL

Create the database with the collation rustcms needs, so text compares as on
PostgreSQL (case, accents and trailing spaces all count):

```sql
CREATE DATABASE app CHARACTER SET utf8mb4 COLLATE utf8mb4_0900_bin;
```

`serve`, the `migrate` commands and the app builder refuse to start when the
database, any string column or the engine of any table differs (every table
InnoDB), when `sql_mode` (global or session) is not strict or has
`NO_BACKSLASH_ESCAPES`, or when the connection is not in UTC with that
collation (pools opened with `rustcms::db::connect` always are). The error
lists the SQL that fixes it.

Differences to know:

- **DDL is not transactional:** each statement commits. A migration that fails
  half-way is retried on a partly changed schema. Use the helpers below, which
  skip what exists, and stop writers while migrating.
- **Column types:** a bare `String` field is `varchar(255)` (PostgreSQL:
  unbounded); `Text` holds 64 KiB. JSON columns accept no literal default
  (only an expression default such as `('[]')`).
- **Sort order** of text is binary (code points), not a locale.
- **Instants** are `DATETIME(6)` in UTC (MySQL's `TIMESTAMP` stops in 2038 and
  drops fractions); fractions below a microsecond are truncated.

## Column helpers

`rustcms::migration` has helpers that give the same table on every database
(and that a re-run skips):

| Helper | Does |
|---|---|
| `create_table(manager, statement)` | `IF NOT EXISTS`; MySQL: InnoDB, `utf8mb4_0900_bin` |
| `create_index(manager, table, statement)` | skips an index the table has (MySQL ignores `IF NOT EXISTS` on indexes) |
| `add_column(manager, table, column)` / `drop_column(manager, table, column)` / `drop_table(manager, table)` | skip what is done |
| `.instant(manager)` / `.instant_now(manager)` | `TIMESTAMPTZ` (with `CURRENT_TIMESTAMP`), `DATETIME(6)` on MySQL |
| `.local_date_time(manager)` | date and time without zone, microseconds on MySQL |

```rust
create_table(manager, Table::create().table(Posts::Table)
    .col(ColumnDef::new(Posts::Id).big_integer().not_null().auto_increment().primary_key())
    .col(ColumnDef::new(Posts::PublishedAt).instant(manager).null())
    .col(ColumnDef::new(Posts::CreatedAt).instant_now(manager).not_null())
    .to_owned()).await
```

## Migrations

`src/database/migrations/mod.rs` runs the framework migrations and then
yours (one file per change, `rustcms make:migration <name>`):

| Command                              | Does                                        |
|--------------------------------------|---------------------------------------------|
| `rustcms migrate`                    | apply pending migrations                    |
| `rustcms migrate --allow-newer`      | also accept migrations applied by a newer binary |
| `rustcms migrate:status`             | list applied / pending / `Newer` (unknown to this binary) |
| `rustcms migrate:rollback --step N`  | undo the last N, in the migrator's order    |
| `rustcms migrate:refresh`            | undo all, apply all (`--force` in production) |
| `rustcms migrate:fresh`              | drop all tables, apply all (`--force` in production) |

Framework and generated migrations run in a transaction on every backend
(`use_transaction() -> Some(true)`), including SQLite whose default is none: a
failed migration leaves no partial schema and can simply be retried. On MySQL
each DDL statement also commits what the migration wrote before it, so a
failed migration can leave both schema changes and data behind: write
migrations that can run again (see above). Keep `Some(true)` in your own
migrations unless a statement cannot run in a transaction.

Never edit a migration that has run in production; add a new one. A migration
that cannot be reversed must say so in its `down` and in its doc comment.

`migrate:rollback` undoes applied migrations last first in the migrator's
order (the framework's, then yours), not in the order they were applied: after
a rustcms upgrade that added framework migrations, `--step 1` undoes your
newest migration, not the framework's. To run an older binary on a newer
schema, keep the migrations and use `migrate --allow-newer`: see
[Upgrading](../getting-started/deployment.md#upgrading).

## Seeding

Seeders fill the database with the records an application needs, or shows
off. They live in `src/database/seeders/`: `DatabaseSeeder` (named
`database`) calls the others in order, and every seeder is registered in
`src/providers/app_service_provider.rs` (`rustcms make:seeder CourseSeeder`
does both).

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

| Command | Does |
|---|---|
| `rustcms db:seed` | runs the `database` seeder |
| `rustcms db:seed --class course` | runs one seeder |
| `rustcms migrate:fresh --seed` | drops every table, migrates, seeds |

- **Production**: `db:seed` refuses without `--force`. A seeder that creates
  accounts with known passwords must refuse in production itself, whatever
  `--force` says.
- **No shared transaction**: a seeder that fails halfway leaves what it wrote.
  Write seeders that complete on the next run (find rows by a natural key),
  and write through your services (which open their own transactions) so the
  application's rules hold; roles and permissions go through `RbacService`.

