# rustcms

A Rust framework for admin panels and headless CMS backends, inspired by
Laravel and FilamentPHP: declare resources in Rust, get an authorized CRUD API
and an admin panel (React, embedded in your binary — no Node needed to use it).

> **Status: 0.1 pre-release.** Feature-complete for the 0.1 scope
> ([limitations](docs/guide/src/reference/limitations.md)); not yet published to crates.io.

```rust,ignore
impl Resource for PostResource {
    type Entity = post::Entity;
    const SLUG: &'static str = "posts";

    fn fields() -> Vec<Field> {
        vec![
            Field::column(post::Column::Title).required().searchable().sortable(),
            Field::column(post::Column::CategoryId).belongs_to("categories").filterable(),
            Field::column(post::Column::Published).updatable(|ctx| ctx.can("posts.publish")),
        ]
    }
}
```

## Quick start

```sh
cargo install --path crates/rustcms-cli
rustcms new blog && cd blog
rustcms make:model Post -m                     # edit the model and migration, then:
rustcms make:resource Post --generate
rustcms migrate
rustcms make:user --email you@example.com --name You --super-admin
rustcms serve                                  # http://localhost:8000/admin
```

Or run the demo (blog with categories, posts, an editor role and a token API):

```sh
cd examples/demo && cargo run -- migrate && cargo run -- db:seed && cargo run -- serve
# http://localhost:8000/admin — admin@example.test / password123
```

## What you get

- **Resources → admin panel**: forms, tables, filters, actions and relationship
  selects generated from Rust; field- and record-level authorization on every path.
- **Auth**: admin sessions (CSRF, Origin checks, rotation) and API tokens with
  abilities; immediate revocation; rate limits.
- **Roles & permissions** per guard, escalation-safe (`RbacService`), with Users,
  Roles and Account pages in the panel.
- **CLI**: `new`, `make:model|migration|resource|policy|test`, `route:list`,
  `migrate*`, `make:user`, `token:create`, `permission:sync`.
- **Testing kit**: every database test runs on SQLite and PostgreSQL;
  `acting_as`, `login_as`, `with_token`, factories.

## Development

```sh
brew install rustup just && rustup default stable
docker compose up -d --wait   # PostgreSQL on localhost:55439
just test-matrix              # Rust tests on SQLite and PostgreSQL
just test-ui                  # admin SPA unit tests (Node 20+)
just e2e                      # browser tests against the demo
just ci                       # most of CI; also `just test-cli`, `just e2e`, `just msrv`
```

- Developer guide (public, mdBook): [`docs/guide/`](docs/guide/src/SUMMARY.md) — `mdbook serve docs/guide`
- Core docs (private): [`docs/core/`](docs/core/README.md) — architecture, admin protocol, ADRs,
  reports, [CHANGELOG](docs/core/CHANGELOG.md), review checklist

## License

MIT OR Apache-2.0
