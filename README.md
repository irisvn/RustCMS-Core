# rustcms

A Rust framework for admin panels and headless CMS backends, inspired by
Laravel and FilamentPHP: declare resources in Rust, get an authorized CRUD API
and a generated React admin panel.

> **Status: pre-release, milestone M1.** Not ready for use.

```sh
cargo install --path crates/rustcms-cli
rustcms new blog && cd blog
rustcms migrate && rustcms serve   # http://127.0.0.1:8000/health
```

## Development

```sh
brew install rustup just && rustup default stable
docker compose up -d --wait   # PostgreSQL on localhost:55439
just test-matrix              # tests on SQLite and PostgreSQL
just ci                       # everything CI runs (+ `just test-cli`)
```

- Docs: `docs/` (mdBook) — start with [`docs/src/admin/resources.md`](docs/src/admin/resources.md)
- Admin protocol: [`docs/src/admin-protocol.md`](docs/src/admin-protocol.md)
- Decisions: [`docs/adr/`](docs/adr)

## License

MIT OR Apache-2.0
