# Directory structure

`rustcms new blog` creates:

```text
blog/
├── Cargo.toml
├── .env                  # secrets and per-machine values (git-ignored)
├── .env.example
├── config/               # app.toml, auth.toml, http.toml, log.toml (committed)
├── lang/
│   └── vi.toml           # translations of your admin labels
├── src/
│   ├── main.rs           # the console: `serve`, `migrate`, generators, your commands
│   ├── lib.rs
│   ├── app.rs            # providers, services, routes: the application is assembled here
│   ├── models/           # SeaORM entities                    (make:model)
│   ├── migrations/       # your migrations, after the framework's (make:migration)
│   ├── admin/            # admin resources, one per file       (make:resource)
│   ├── policies/         # record policies                     (make:policy)
│   └── http/
│       └── api/          # JSON API, one module per model      (make:api)
└── tests/                # integration tests                   (make:test)
```

## `src/app.rs`

The application is a function from configuration to an `AppBuilder`:

```rust,ignore
pub fn build(config: Config) -> AppBuilder {
    let mut app = AppBuilder::new(config);
    let api = crate::http::api::routes();
    app.provider(AuthProvider::new().api_permissions(&api));
    app.provider(
        AdminProvider::new()
            .brand("Blog")
            .translations("vi", include_str!("../lang/vi.toml"))
            .resource::<crate::admin::post::PostResource>()
            // rustcms:resources
    );
    api.register(&mut app);
    app
}
```

Generators register what they create at the `// rustcms:…` anchor comments
(`rustcms:resources`, `rustcms:resource-mods`, `rustcms:migrations`, …). If an
anchor was removed, they print the line to add instead of editing the file.

## `src/main.rs`

```rust,ignore
fn main() -> std::process::ExitCode {
    rustcms::Console::<blog::migrations::Migrator>::new(blog::app::build).run_blocking()
}
```

The binary is the whole application: HTTP server, console commands and the
embedded admin panel.

## Admin resources

One file per resource in `src/admin/`, as in Filament: `src/admin/post.rs`
holds `PostResource` with its fields, form, table, actions and hooks. Record
policies live in `src/policies/`.

## JSON API

`src/http/api/` holds one module per model:

```text
src/http/api/
├── mod.rs              routes(): every API route
└── posts/
    ├── mod.rs          routes() of posts, with the access each requires
    ├── handlers.rs     list, show, create, …        (≈ controller)
    ├── requests.rs     input structs + Validate     (≈ form request)
    └── json.rs         output structs               (≈ API resource)
```

See [Routing](../basics/routing.md).
