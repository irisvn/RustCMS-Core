# Service providers & container

`src/bootstrap/mod.rs` assembles the application from **providers**, listed
in order in `src/bootstrap/providers.rs` (like Laravel's
`bootstrap/providers.php`). RustCMS's own features are providers too:
`AuthProvider`, `AdminProvider`, `StorageProvider`. A new application has
three of its own in `src/providers/`:

| File | Registers |
|---|---|
| `auth_service_provider.rs` | permissions beyond each resource's own (`AuthProvider`) |
| `admin_panel_provider.rs` | the panel: resources, settings pages, translations (`AdminProvider`) |
| `app_service_provider.rs` | jobs, seeders and console commands (`ServiceProvider`) |

<<< @/../../../examples/demo/src/bootstrap/providers.rs#register [src/bootstrap/providers.rs]

Your own providers go in the same list: `app.provider(BillingProvider);`.

## Writing a provider

Like Laravel, a provider has two phases: `register` runs while the
application is assembled (bind services, add routes and commands); `boot`
runs once the database is connected.

```rust
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

```rust
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
| `app.job::<J>()` | a job the queue's workers run ([Queues](../digging-deeper/queues.md)) |
| `app.schedule(schedule)` | periodic work, run only by `serve` ([Scheduling](../digging-deeper/scheduling.md)) |
| `app.seeder(seeder)` | a seeder for `db:seed` ([Seeding](../basics/database.md#seeding)) |
| `app.encrypted_column::<E>(column, binding)` | an encrypted column for `key:reencrypt` ([Encrypted columns](../digging-deeper/encryption.md)) |
| `app.mail_template(template)` | a mail template, `mail` feature ([Mail](../digging-deeper/mail.md#templates)) |
| `app.config()` | the configuration |
