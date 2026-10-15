# demo

A small blog built with rustcms: categories (with their posts and a custom
color widget), posts with an ownership policy (editors edit only their own
posts), soft deletes, a publish action, a tabbed form (content, publishing,
key/value attributes), cover images and rich text, and a token/JWT API.

```sh
cp .env.example .env && cargo run -- key:generate   # first time only
cargo run -- migrate
cargo run -- db:seed     # admin@example.test / editor@example.test, password `password123`
cargo run -- serve         # http://localhost:8000/admin
```

API (`GET/POST /api/v1/posts`):

```sh
cargo run -- token:create --email admin@example.test --name cli --abilities posts.view_any,posts.create
curl -H "Authorization: Bearer rcms_…" http://localhost:8000/api/v1/posts
```

Tests: `cargo test` (SQLite; PostgreSQL with `RUSTCMS_TEST_PG`, MySQL with
`RUSTCMS_TEST_MYSQL`). The layout is the one `rustcms new` generates (resources in
`src/admin/resources/`, settings in `src/settings/` with their page in
`src/admin/pages/`, seeders in `src/database/seeders/`), so every
`rustcms make:*` works here too.
