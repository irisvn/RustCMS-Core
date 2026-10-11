# demo

A [rustcms](https://github.com/irisvn/rustcms) application.

```sh
rustcms migrate          # create tables (SQLite file by default, see .env)
rustcms serve            # http://localhost:8000/health
cargo test               # set RUSTCMS_TEST_PG to also run tests on PostgreSQL
```

| Command                              | Does                                   |
|--------------------------------------|----------------------------------------|
| `rustcms make:model Post -m`         | entity in `src/models` + migration     |
| `rustcms make:migration add_x_to_y`  | blank migration in `src/migrations`    |
| `rustcms migrate:status`             | applied / pending migrations           |
| `rustcms migrate:rollback --step 1`  | undo the last migration                |
| `rustcms key:generate --force`       | rotate `APP_KEY` (signs everyone out)  |
