# Installation & first app

Requirements: Rust (stable), optionally Docker for PostgreSQL.

```sh
cargo install --path crates/rustcms-cli    # from a rustcms checkout (crates.io after 0.1)
rustcms new blog
cd blog
rustcms migrate      # SQLite file by default (DATABASE_URL in .env)
rustcms serve        # http://127.0.0.1:8000/health
```

`rustcms new` writes a fresh `APP_KEY` to `.env`. Never commit `.env`.

## Layout

```text
blog/
├── .env                 # secrets and per-machine values (git-ignored)
├── config/              # app.toml, http.toml, log.toml, … (committed)
├── src/
│   ├── main.rs          # rustcms::Console — the `artisan` of your app
│   ├── app.rs           # providers, services, routes
│   ├── models/          # SeaORM entities   (rustcms make:model)
│   └── migrations/      # migrations        (rustcms make:migration)
└── tests/               # run on SQLite, and on PostgreSQL with RUSTCMS_TEST_PG
```

## Generating code

```sh
rustcms make:model Post -m           # src/models/post.rs + create_posts_table migration
rustcms make:migration add_slug_to_posts
```

Generators register new modules at the `// rustcms:…` anchor comments. If an anchor
was removed, they print the line to add instead of editing the file.

All commands are listed in the [CLI reference](cli.md).
