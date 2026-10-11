# Database & migrations

rustcms uses [SeaORM](https://www.sea-ql.org/SeaORM/) 2.0 (pinned). PostgreSQL
and SQLite are supported in 0.1.

SQLite connections always enable foreign keys, WAL and a 5s busy timeout.

## Migrations

`src/migrations/mod.rs` runs the framework migrations (`users`, `sessions`) and
then yours:

| Command                              | Does                                        |
|--------------------------------------|---------------------------------------------|
| `rustcms migrate`                    | apply pending migrations                    |
| `rustcms migrate:status`             | list applied / pending                      |
| `rustcms migrate:rollback --step N`  | undo the last N                             |
| `rustcms migrate:refresh`            | undo all, apply all (`--force` in production) |
| `rustcms migrate:fresh`              | drop all tables, apply all (`--force` in production) |

Framework and generated migrations run in a transaction on every backend
(`use_transaction() -> Some(true)`), including SQLite whose default is none: a
failed migration leaves no partial schema and can simply be retried. Keep this
in your own migrations unless a statement cannot run in a transaction.

Never edit a migration that has run in production; add a new one. A migration
that cannot be reversed must say so in its `down` and in its doc comment.
