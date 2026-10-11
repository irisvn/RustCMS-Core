# Application & providers

`src/app.rs` assembles the application:

```rust,ignore
use rustcms::prelude::*;

pub fn build(config: Config) -> AppBuilder {
    let mut app = AppBuilder::new(config);
    app.provider(BillingProvider);
    app.routes(|router| router.route("/api/v1/ping", get(ping)));
    app
}
```

## Service providers

Like Laravel, a provider has two phases: `register` (bind services, add routes)
runs while the app is assembled; `boot` runs once the database is connected.

```rust,ignore
struct BillingProvider;

#[async_trait::async_trait]
impl ServiceProvider for BillingProvider {
    fn register(&self, app: &mut AppBuilder) {
        app.bind(Stripe::new(app.config().section::<StripeConfig>("stripe").unwrap()));
    }

    async fn boot(&self, state: &AppState) -> anyhow::Result<()> {
        state.service::<Stripe>().unwrap().verify().await
    }
}
```

## Using services

```rust,ignore
async fn charge(Inject(stripe): Inject<Stripe>, State(state): State<AppState>) -> AppResult<Json<Receipt>> {
    // state.db(), state.config() are always available
}
```

A missing binding is a 500 (logged), never a panic.
