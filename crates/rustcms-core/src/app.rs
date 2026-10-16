//! Application assembly: services, providers, routes.

use std::any::{Any, TypeId, type_name};
use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use axum::Router;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use sea_orm::DatabaseConnection;

use crate::config::Config;
use crate::error::AppError;
use crate::http;

/// Services bound by type, shared by every request.
#[derive(Default)]
pub struct ServiceMap(HashMap<TypeId, Arc<dyn Any + Send + Sync>>);

impl ServiceMap {
    fn insert<T: Send + Sync + 'static>(&mut self, service: T) {
        self.0.insert(TypeId::of::<T>(), Arc::new(service));
    }

    fn get<T: Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        self.0.get(&TypeId::of::<T>()).cloned().and_then(|s| s.downcast().ok())
    }
}

struct StateInner {
    config: Config,
    db: DatabaseConnection,
    services: ServiceMap,
}

/// Shared application state, cheap to clone. Available to handlers via `State<AppState>`.
#[derive(Clone)]
pub struct AppState(Arc<StateInner>);

impl AppState {
    /// Loaded configuration.
    pub fn config(&self) -> &Config {
        &self.0.config
    }

    /// Database pool.
    pub fn db(&self) -> &DatabaseConnection {
        &self.0.db
    }

    /// A service bound with [`AppBuilder::bind`].
    pub fn service<T: Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        self.0.services.get()
    }
}

/// Extracts a bound service: `Inject(mailer): Inject<Mailer>`. A missing binding is a 500.
pub struct Inject<T>(pub Arc<T>);

impl<T: Send + Sync + 'static> FromRequestParts<AppState> for Inject<T> {
    type Rejection = AppError;

    async fn from_request_parts(_parts: &mut Parts, state: &AppState) -> Result<Self, AppError> {
        state
            .service::<T>()
            .map(Inject)
            .ok_or_else(|| AppError::internal(format!("service `{}` is not bound", type_name::<T>())))
    }
}

/// A unit of framework or application setup, like Laravel service providers.
///
/// `register` runs while the app is assembled (bind services, add routes);
/// `boot` runs once the database is connected, before serving.
#[async_trait]
pub trait ServiceProvider: Send + Sync + 'static {
    /// Binds services and routes.
    fn register(&self, _app: &mut AppBuilder) {}

    /// Runs after the state is built.
    async fn boot(&self, _state: &AppState) -> anyhow::Result<()> {
        Ok(())
    }
}

/// A console command contributed by a provider or the application
/// (`make:user`, `permission:sync`, …). Runs with the booted application state.
#[async_trait]
pub trait AppCommand: Send + Sync + 'static {
    /// Clap definition; its name is the command name.
    fn definition(&self) -> clap::Command;

    /// Executes the command.
    async fn run(&self, state: &AppState, args: &clap::ArgMatches) -> anyhow::Result<()>;
}

pub(crate) type RouteFn = Box<dyn FnOnce(Router<AppState>) -> Router<AppState> + Send>;
pub(crate) type MiddlewareFn = Box<dyn FnOnce(Router<AppState>, AppState) -> Router<AppState> + Send>;

/// Assembles an [`App`].
pub struct AppBuilder {
    config: Config,
    services: ServiceMap,
    routes: Vec<RouteFn>,
    providers: Vec<Arc<dyn ServiceProvider>>,
    commands: Vec<Arc<dyn AppCommand>>,
    middleware: Vec<MiddlewareFn>,
    described: Vec<RouteInfo>,
    body_limits: Vec<http::BodyLimit>,
    jobs: crate::queue::Jobs,
    seeders: crate::database::Seeders,
    #[cfg(feature = "mail")]
    mail_templates: crate::mail::MailTemplates,
}

/// A route as listed by `route:list`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteInfo {
    /// HTTP method.
    pub method: axum::http::Method,
    /// Path template (`/admin/api/resources/{slug}`).
    pub path: String,
    /// Who registered it (`core`, `auth`, `admin`, `app`, …).
    pub source: String,
}

impl AppBuilder {
    /// Starts from a loaded configuration.
    pub fn new(config: Config) -> Self {
        Self {
            config,
            services: ServiceMap::default(),
            routes: Vec::new(),
            providers: Vec::new(),
            commands: vec![
                Arc::new(crate::queue::QueueWork),
                Arc::new(crate::queue::QueueFailed),
                Arc::new(crate::queue::QueueRetry),
                Arc::new(crate::queue::QueueForget),
            ],
            middleware: Vec::new(),
            described: vec![RouteInfo {
                method: axum::http::Method::GET,
                path: "/health".into(),
                source: "core".into(),
            }],
            body_limits: Vec::new(),
            jobs: crate::queue::Jobs::default(),
            seeders: crate::database::Seeders::default(),
            #[cfg(feature = "mail")]
            mail_templates: crate::mail::MailTemplates::default(),
        }
    }

    /// Describes routes for `route:list` (axum routers cannot be listed back):
    /// providers describe theirs; applications describe what they add with [`Self::routes`].
    pub fn describe_routes(
        &mut self,
        source: &str,
        routes: impl IntoIterator<Item = (axum::http::Method, String)>,
    ) -> &mut Self {
        self.described.extend(routes.into_iter().map(|(method, path)| RouteInfo {
            method,
            path,
            source: source.into(),
        }));
        self
    }

    /// Every described route, sorted by path then method.
    pub fn described_routes(&self) -> Vec<RouteInfo> {
        let mut routes = self.described.clone();
        routes.sort_by(|a, b| a.path.cmp(&b.path).then_with(|| a.method.as_str().cmp(b.method.as_str())));
        routes.dedup();
        routes
    }

    /// Configuration.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Binds a service, replacing any previous binding of the same type.
    pub fn bind<T: Send + Sync + 'static>(&mut self, service: T) -> &mut Self {
        self.services.insert(service);
        self
    }

    /// A service bound earlier (by this or another provider).
    pub fn service<T: Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        self.services.get()
    }

    /// Wraps every application route (not the 404 fallback) once all routes are added.
    /// Runs inside the framework stack (request id, client IP, limits).
    pub fn middleware(
        &mut self,
        wrap: impl FnOnce(Router<AppState>, AppState) -> Router<AppState> + Send + 'static,
    ) -> &mut Self {
        self.middleware.push(Box::new(wrap));
        self
    }

    /// Adds routes.
    pub fn routes(&mut self, routes: impl FnOnce(Router<AppState>) -> Router<AppState> + Send + 'static) -> &mut Self {
        self.routes.push(Box::new(routes));
        self
    }

    /// A body limit of its own (bytes) for requests with exactly this method and
    /// path (e.g. an upload route); every other request keeps `http.body_limit`.
    pub fn body_limit(&mut self, method: axum::http::Method, path: impl Into<String>, bytes: usize) -> &mut Self {
        self.body_limits.push(http::BodyLimit { method, path: path.into(), bytes });
        self
    }

    /// Registers a job the queue's workers run (see [`crate::queue::Job`]).
    ///
    /// # Panics
    /// On an invalid or repeated kind.
    pub fn job<J: crate::queue::Job>(&mut self) -> &mut Self {
        self.jobs.register::<J>();
        self
    }

    /// Adds a mail template ([`crate::mail_template!`]).
    ///
    /// # Panics
    /// On an invalid template, or one added twice.
    #[cfg(feature = "mail")]
    pub fn mail_template(&mut self, template: crate::mail::MailTemplate) -> &mut Self {
        self.mail_templates.add(template);
        self
    }

    /// Registers a seeder for `db:seed` (the one named `database` runs by default).
    ///
    /// # Panics
    /// On an invalid or repeated name.
    pub fn seeder(&mut self, seeder: impl crate::database::Seeder) -> &mut Self {
        self.seeders.register(Arc::new(seeder));
        self
    }

    /// The registered seeders (`db:seed`).
    pub fn seeders(&self) -> crate::database::Seeders {
        self.seeders.clone()
    }

    /// Adds a console command.
    pub fn command(&mut self, command: impl AppCommand) -> &mut Self {
        self.commands.push(Arc::new(command));
        self
    }

    /// Console commands added so far.
    pub fn commands(&self) -> Vec<Arc<dyn AppCommand>> {
        self.commands.clone()
    }

    /// Registers a provider (its `register` runs immediately).
    pub fn provider(&mut self, provider: impl ServiceProvider) -> &mut Self {
        provider.register(self);
        self.providers.push(Arc::new(provider));
        self
    }

    /// Connects to the database and builds the app.
    pub async fn build(self) -> anyhow::Result<App> {
        let db = crate::db::connect(&self.config.database).await?;
        self.build_with_db(db).await
    }

    /// Builds the app on an existing connection (tests, tools).
    pub async fn build_with_db(
        #[cfg_attr(not(feature = "redis"), allow(unused_mut))] mut self,
        db: DatabaseConnection,
    ) -> anyhow::Result<App> {
        crate::db::check(&db, true).await?;
        #[cfg(feature = "redis")]
        if let Some(redis) = crate::redis::from_config(&self.config).await? {
            self.bind(redis);
        }
        #[cfg(not(feature = "redis"))]
        if self
            .config
            .section::<serde_json::Value>("redis")
            .ok()
            .and_then(|r| r.get("url").cloned())
            .is_some_and(|u| u.as_str().is_some_and(|u| !u.trim().is_empty()))
        {
            anyhow::bail!("[redis] url is set, but rustcms was built without its `redis` feature");
        }
        crate::queue::QueueConfig::of(&self.config)?;
        self.services.insert(self.jobs);
        #[cfg(feature = "mail")]
        self.services.insert(self.mail_templates);
        let state = AppState(Arc::new(StateInner { config: self.config, db, services: self.services }));
        for provider in &self.providers {
            provider.boot(&state).await?;
        }
        let router = http::router(state.clone(), self.routes, self.middleware, self.body_limits);
        Ok(App { state, router })
    }
}

/// A built application, ready to serve.
pub struct App {
    state: AppState,
    router: Router,
}

impl App {
    /// Shared state.
    pub fn state(&self) -> &AppState {
        &self.state
    }

    /// The complete router with middleware, for serving or testing.
    pub fn router(&self) -> Router {
        self.router.clone()
    }

    /// Serves HTTP until Ctrl-C / SIGTERM.
    pub async fn serve(self, listen: Option<std::net::SocketAddr>) -> anyhow::Result<()> {
        let addr = listen.unwrap_or(self.state.config().app.listen);
        let listener = tokio::net::TcpListener::bind(addr).await?;
        tracing::info!(%addr, "listening");
        // Workers run beside the server and stop after it (jobs may finish).
        let queue = crate::queue::QueueConfig::of(self.state.config())?;
        let workers = (queue.enabled && queue.workers > 0)
            .then(|| crate::queue::Workers::start(&self.state, &queue, None, queue.workers));
        // One signal for both: workers stop claiming at once and drain while
        // the server drains its requests.
        let (signalled, signal) = tokio::sync::watch::channel(false);
        let listen = tokio::spawn(async move {
            shutdown_signal().await;
            let _ = signalled.send(true);
        });
        let (mut server_signal, mut worker_signal) = (signal.clone(), signal.clone());
        let server = axum::serve(listener, self.router.into_make_service_with_connect_info::<std::net::SocketAddr>())
            .with_graceful_shutdown(async move {
                let _ = server_signal.wait_for(|s| *s).await;
            });
        let drain = async move {
            let _ = worker_signal.wait_for(|s| *s).await;
            if let Some(workers) = workers {
                workers.shutdown().await;
            }
        };
        let mut server = std::pin::pin!(async { server.await });
        let mut drain = std::pin::pin!(drain);
        let mut drained = false;
        let served = tokio::select! {
            served = &mut server => served,
            () = &mut drain => {
                drained = true;
                server.await
            }
        };
        // After the signal both drain; a server that stopped on its own (an
        // error) drops the workers, which stop without waiting.
        if !drained && *signal.borrow() {
            drain.await;
        }
        listen.abort();
        served?;
        Ok(())
    }
}

pub(crate) async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut signal) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            signal.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
    tracing::info!("shutting down");
}

#[cfg(test)]
mod route_tests {
    use super::*;

    #[test]
    fn described_routes_are_sorted_and_deduplicated() {
        let figment = crate::config::Config::figment(std::path::Path::new("/nonexistent"))
            .unwrap()
            .merge(figment::providers::Serialized::default("app.key", crate::key::AppKey::generate().encode()))
            .merge(figment::providers::Serialized::default("database.url", "sqlite::memory:"));
        let mut app = AppBuilder::new(crate::config::Config::from_figment(figment).unwrap());
        app.describe_routes("app", [(axum::http::Method::POST, "/b".into()), (axum::http::Method::GET, "/a".into())]);
        app.describe_routes("app", [(axum::http::Method::GET, "/a".into())]);
        let routes: Vec<(String, String)> =
            app.described_routes().into_iter().map(|r| (r.method.to_string(), r.path)).collect();
        assert_eq!(
            routes,
            [("GET".into(), "/a".into()), ("POST".into(), "/b".into()), ("GET".into(), "/health".into())]
        );
    }
}
