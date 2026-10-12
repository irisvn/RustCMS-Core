# Installation & first app

Requirements: Rust (stable), optionally Docker for PostgreSQL.

```sh
cargo install --path crates/rustcms-cli    # from a rustcms checkout (crates.io after 0.1)
rustcms new blog
cd blog
rustcms migrate      # SQLite file by default (DATABASE_URL in .env)
rustcms make:user --email you@example.com --name You --super-admin
rustcms serve        # http://localhost:8000/admin
```

Open **http://localhost:8000/admin** and sign in. Use the host of `APP_URL`:
sign-in and every change are only accepted from that origin (opening
`http://127.0.0.1:8000/admin` shows the page but sign-in answers 403; list other
origins in `auth.admin_origins`). No Node is needed: the panel is embedded in the
binary.

## Try the demo

The repository's `examples/demo` is a small blog (categories, posts, an editor
who may edit only their own posts, a token API):

```sh
cd examples/demo
cp .env.example .env && cargo run -- key:generate   # first time only
cargo run -- migrate
cargo run -- demo:seed          # admin@example.test / editor@example.test, password `password123`
cargo run -- serve              # http://localhost:8000/admin
```

The API part (`/api/v1/posts`) uses tokens:

```sh
cargo run -- token:create --email admin@example.test --name cli --abilities posts.view_any,posts.create
curl -H "Authorization: Bearer rcms_…" http://localhost:8000/api/v1/posts
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
│   ├── admin/           # admin resources   (rustcms make:resource)
│   ├── policies/        # record policies   (rustcms make:policy)
│   └── migrations/      # migrations        (rustcms make:migration)
└── tests/               # run on SQLite, and on PostgreSQL with RUSTCMS_TEST_PG
```

## Generating code

```sh
rustcms make:model Post -m           # src/models/post.rs + create_posts_table migration
rustcms make:migration add_slug_to_posts
rustcms make:resource Post --generate --belongs-to category_id=categories
                                     # src/admin/post.rs: fields, form, table, filters from the model
rustcms make:policy PostPolicy       # src/policies/post_policy.rs
rustcms make:test PostAdminTest      # tests/post_admin_test.rs
rustcms route:list                   # routes described by the framework, providers and your app
```

`make:resource --generate` maps column types to inputs (text, textarea for
`body`/`content`/…, number, toggle, date, date-time) and adds table columns and
filters. `*_id` columns become relationship selects only with `--belongs-to`
(which needs `--generate`); otherwise they get a text input and a
`TODO(rustcms)` comment, with a warning. The model must be in
`src/models/<snake_name>.rs`. Secret-looking columns (`password`, `token`, …)
and types the panel does not support are skipped with a warning; owner columns
(`author_id`, `user_id`, …) are shown but set by the server. `route:list` shows
the routes your app describes with `app.describe_routes(…)` (new apps describe
theirs).

Generators register new modules at the `// rustcms:…` anchor comments. If an anchor
was removed, they print the line to add instead of editing the file.

All commands are listed in the [CLI reference](cli.md).
