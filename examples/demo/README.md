# demo

A small blog built with rustcms: categories, posts with an ownership policy
(editors edit only their own posts), a publish action, and a token API.

```sh
cp .env.example .env && cargo run -- key:generate   # first time only
cargo run -- migrate
cargo run -- demo:seed     # admin@example.test / editor@example.test, password `password123`
cargo run -- serve         # http://localhost:8000/admin
```

API (`GET/POST /api/v1/posts`):

```sh
cargo run -- token:create --email admin@example.test --name cli --abilities posts.view_any,posts.create
curl -H "Authorization: Bearer rcms_…" http://localhost:8000/api/v1/posts
```

Tests: `cargo test` (SQLite; PostgreSQL with `RUSTCMS_TEST_PG`). Unlike a fresh
`rustcms new` app, the demo keeps its resources in `src/admin.rs`.
