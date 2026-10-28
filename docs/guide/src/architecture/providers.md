# Service providers & container

A **provider** is a unit of setup, as in Laravel: it binds services and adds
routes, commands, jobs and schedules while the application is assembled, then
optionally checks or prepares things once the database is connected. The
framework's own features are providers too. The **container** holds one shared
instance per type, which handlers receive with `Inject<T>`.

## Quick start

`src/bootstrap/mod.rs` assembles the application from the providers listed, in
order, in `src/bootstrap/providers.rs` (like Laravel's
`bootstrap/providers.php`). A new application's list:

```rust
// src/bootstrap/providers.rs
pub fn register(app: &mut AppBuilder, api: &ApiRoutes) {
    app.provider(providers::auth_service_provider::provider(api));
    app.provider(providers::admin_panel_provider::provider());
    app.provider(providers::app_service_provider::AppServiceProvider);
}
```

Each comes from a file of `src/providers/`:

| File | Returns | Registers |
|---|---|---|
| `auth_service_provider.rs` | `AuthProvider` | authentication, and the permissions beyond each resource's own (those of your API routes included) |
| `admin_panel_provider.rs` | `AdminProvider` | the panel: resources, pages, extensions, translations |
| `app_service_provider.rs` | your `ServiceProvider` | jobs, seeders, console commands, your services |

Your own providers go in the same list: `app.provider(BillingProvider);`.

## Framework providers

| Provider | Adds | Needed for |
|---|---|---|
| `AuthProvider` (`rustcms::auth`) | the admin session and API guards, sign-in routes, permissions, `make:user`, `permission:sync`, … | authentication and the admin panel; before every provider that uses its services |
| `rustcms::storage::StorageProvider` | the configured disk (`[storage]`) | uploads, the media library (`AdminProvider::media()`) |
| `rustcms::mail::MailProvider` | the mailer (`[mail]`) and its job | sending mail: invitations, password resets, your own mail |
| `AdminProvider` (`rustcms::admin`) | the admin panel | the panel |

Order matters during `register`: a provider can only use what earlier
providers bound (in `boot`, every binding is available). `AdminProvider` reads
the services of `AuthProvider`, and of `StorageProvider` when the panel has a
media library, in its `register`: register them before it. A new
application has neither storage nor mail; add them when you need them.

## A full list: the demo

The demo application registers storage and mail, and passes its
[admin routes](../admin/admin-routes.md) to `AuthProvider` so that their
permissions are declared too:

<<< @/../../../examples/demo/src/bootstrap/providers.rs#register [src/bootstrap/providers.rs]

<<< @/../../../examples/demo/src/providers/auth_service_provider.rs#auth-provider [src/providers/auth_service_provider.rs]

`AuthProvider` builders:

| Method | Does |
|---|---|
| `AuthProvider::new()` | the built-in permissions only (users, roles, permissions) |
| `.permission(guard, name, group)` | declares a permission for `permission:sync` |
| `.api_permissions(&api)` | declares every permission the API routes require (`api` guard) |
| `.admin_permissions(&admin)` | declares every permission the admin routes require (`admin` guard) |
| `.permission_area(id, area)` | a section of the Roles page |
| `.permission_meta(guard, name, meta)` | the label, description and area of a permission on the Roles page |
| `.permission_preset(id, preset)` | a starting selection on the Roles page (it grants nothing by itself) |
| `.define(ability, check)` | a gate ability ([Authorization](../admin/authorization.md)) |

See [Users & roles](../admin/users-roles.md) for the Roles page.

## Writing a provider

A provider has two phases:

- `register(&self, app: &mut AppBuilder)` runs while the application is
  assembled, right when `app.provider(…)` adds it: read configuration, bind
  services, add routes, commands, jobs, schedules. It cannot reach the
  database. A configuration error here is a panic with a clear message.
- `async fn boot(&self, state: &AppState) -> anyhow::Result<()>` runs once the
  database is connected and every provider is registered, in the same order.
  An error stops the command.

`boot` runs for `serve`, `db:seed`, `migrate:fresh --seed` and the console
commands of providers and the application. It does not run for `migrate`,
`migrate:rollback`, `migrate:refresh`, `migrate:status`, `route:list` or
`key:generate` ([What each command loads](lifecycle.md#what-each-command-loads)).
Both methods have empty defaults.

A sketch (`Stripe`, `StripeConfig` and `SyncInvoices` stand for your own
types):

```rust
// Cargo.toml: anyhow = "1"
use rustcms::async_trait;
use rustcms::prelude::*;

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

The demo's application provider binds a service, registers seeders and keeps
the generator anchors:

<<< @/../../../examples/demo/src/providers/app_service_provider.rs#app-service-provider [src/providers/app_service_provider.rs]

::: warning Background work
Do not start loops with `tokio::spawn` in `register` or `boot`: they would run
in every command that boots, on every instance. Use
[`app.schedule(…)`](../digging-deeper/scheduling.md) for periodic work (run
automatically only by `serve`, each slot once across instances; by hand with
`schedule:run <name>`) and
[jobs](../digging-deeper/queues.md) for work triggered by a request. A
long-lived task tied to requests (a stream producer) takes `app.shutdown()`,
which is cancelled when the server stops, as the demo's `Ticker` does.
:::

## The container

`app.bind(value)` stores one shared instance per type; binding the same type
again replaces it. Read it back:

<<< @/../../../examples/demo/src/http/controllers/live_controller.rs#inject-reset [In a handler: src/http/controllers/live_controller.rs]

```rust
// While assembling, after the provider that bound it:
let ticker = app.service::<Ticker>();       // Option<Arc<Ticker>>
// Anywhere with the state (boot, jobs, commands, seeders):
let ticker = state.service::<Ticker>();     // Option<Arc<Ticker>>
```

A missing binding in a handler is a 500 (logged), never a panic. Bind a type
of your own (a struct, or a newtype around a client) rather than a common type
such as `String`, which another provider could bind too.

`AppState` also gives every handler and job `config()`, `db()` (the pool) and
`keyring()` (`APP_KEY` and the previous keys, for
[encrypted values](../digging-deeper/encryption.md)). Handlers take it as
`State(state): State<AppState>`.

## The builder

| Method | Does |
|---|---|
| `AppBuilder::new(config)` | an empty application (with the queue, schedule and `key:reencrypt` commands) |
| `app.provider(p)` | adds a provider (its `register` runs now) |
| `app.bind(service)` / `app.service::<T>()` | the container |
| `app.config()` | the configuration |
| `app.routes(\|router\| …)` | adds Axum routes (prefer `ApiRoutes` and `AdminRoutes`, which declare access) |
| `app.middleware(\|router, state\| …)` | wraps the application's routes ([Adding middleware](lifecycle.md#adding-middleware)) |
| `app.describe_routes(source, routes)` | lists routes added with `routes` for `route:list`: `routes` yields `(Method, String)` pairs |
| `app.described_routes()` | every described route, sorted |
| `app.body_limit(method, path, bytes)` | the body limit of requests with exactly this method and path (larger or smaller than `http.body_limit`) |
| `app.command(cmd)` / `app.commands()` | a console command, an `AppCommand` ([Console](../basics/console.md)) |
| `app.job::<J>()` | a job the queue's workers run ([Queues](../digging-deeper/queues.md)) |
| `app.schedule(schedule)` | periodic work, run automatically only by `serve` ([Scheduling](../digging-deeper/scheduling.md)) |
| `app.seeder(seeder)` / `app.seeders()` | a seeder for `db:seed` ([Seeding](../basics/database.md#seeding)) |
| `app.encrypted_column::<E>(column, binding)` | an encrypted column for `key:reencrypt` ([Encrypted columns](../digging-deeper/encryption.md)) |
| `app.add_encrypted_column(column)` / `app.encrypted_columns()` | the same, for a column built elsewhere; the columns so far |
| `app.mail_template(template)` | a mail template, `mail` feature ([Mail](../digging-deeper/mail.md#templates)) |
| `app.shutdown()` | the signal long-lived tasks stop on when the server stops |
| `app.build().await` | connects and boots: an `App` (the console does this) |
| `app.build_with_db(db).await` | the same on an existing connection (tests, tools) |

These registrations panic on a mistake, so it shows at the first start:
`job` on an invalid kind or a kind another job type already uses
(registering the same type again does nothing); `schedule` and `seeder` on an
invalid or repeated name; `mail_template` on an invalid template or one added
twice for the same name and locale; `encrypted_column` on a column registered
twice with another binding.

## Pitfalls

- **A service is `None` in `register`**: the provider that binds it comes
  later in the list. Move it up, or read it in `boot`.
- **`AdminProvider::media()` panics at start**: `StorageProvider` is missing
  or registered after `AdminProvider`.
- **A permission is missing on the Roles page**: declare it on `AuthProvider`
  (or through `api_permissions` / `admin_permissions`), then run
  `permission:sync`.

## Related pages

- [Request lifecycle](lifecycle.md)
- [Directory structure](../getting-started/directory-structure.md)
- [Console](../basics/console.md)
- [Testing](../basics/testing.md)
