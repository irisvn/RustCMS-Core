# Service providers & container

`src/app.rs` assembles the application from **providers**. RustCMS's own
features are providers too: `AuthProvider`, `AdminProvider`, `StorageProvider`.

```rust,ignore
use rustcms::prelude::*;

pub fn build(config: Config) -> AppBuilder {
    let mut app = AppBuilder::new(config);
    app.provider(AuthProvider::new());
    app.provider(BillingProvider);
    app.routes(|router| router.route("/webhooks/stripe", post(stripe_webhook)));
    app
}
```

## Writing a provider

Like Laravel, a provider has two phases: `register` runs while the
application is assembled (bind services, add routes and commands); `boot`
runs once the database is connected.

```rust,ignore
// Cargo.toml: anyhow = "1"
use rustcms::async_trait;

struct BillingProvider;

#[async_trait]
impl ServiceProvider for BillingProvider {
    fn register(&self, app: &mut AppBuilder) {
        let config: StripeConfig = app.config().section("stripe").expect("[stripe] settings");
        app.bind(Stripe::new(config));
        app.command(SyncInvoices);
    }

    async fn boot(&self, state: &AppState) -> anyhow::Result<()> {
        state.service::<Stripe>().expect("bound in register").verify().await
    }
}
```

Register providers in dependency order: `AdminProvider` uses the services of
`AuthProvider` (and of `StorageProvider` for the media library).

## The container

`app.bind(value)` stores one shared instance per type. Read it back:

```rust,ignore
// In a handler:
async fn charge(Inject(stripe): Inject<Stripe>, State(app): State<AppState>) -> AppResult<Json<Receipt>> {
    // app.db() and app.config() are always available
}

// Anywhere with the state:
let stripe = state.service::<Stripe>();     // Option<Arc<Stripe>>
```

A missing binding in a handler is a 500 (logged), never a panic.

## The builder

| Method | Does |
|---|---|
| `app.provider(p)` | adds a provider (its `register` runs now) |
| `app.bind(service)` / `app.service::<T>()` | the container |
| `app.routes(\|router\| …)` | adds Axum routes |
| `app.describe_routes(group, routes)` | lists routes for `route:list` |
| `app.body_limit(method, path, bytes)` | a larger body limit for one route |
| `app.command(cmd)` | a console command ([Console](../basics/console.md)) |
| `app.config()` | the configuration |
