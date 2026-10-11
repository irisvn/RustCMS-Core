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

pub(crate) type RouteFn = Box<dyn FnOnce(Router<AppState>) -> Router<AppState> + Send>;

/// Assembles an [`App`].
pub struct AppBuilder {
    config: Config,
    services: ServiceMap,
    routes: Vec<RouteFn>,
    providers: Vec<Arc<dyn ServiceProvider>>,
}

impl AppBuilder {
    /// Starts from a loaded configuration.
    pub fn new(config: Config) -> Self {
        Self { config, services: ServiceMap::default(), routes: Vec::new(), providers: Vec::new() }
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

    /// Adds routes.
    pub fn routes(&mut self, routes: impl FnOnce(Router<AppState>) -> Router<AppState> + Send + 'static) -> &mut Self {
        self.routes.push(Box::new(routes));
        self
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
    pub async fn build_with_db(self, db: DatabaseConnection) -> anyhow::Result<App> {
        let state = AppState(Arc::new(StateInner { config: self.config, db, services: self.services }));
        for provider in &self.providers {
            provider.boot(&state).await?;
        }
        let router = http::router(state.clone(), self.routes);
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
        axum::serve(listener, self.router.into_make_service_with_connect_info::<std::net::SocketAddr>())
            .with_graceful_shutdown(shutdown_signal())
            .await?;
        Ok(())
    }
}

async fn shutdown_signal() {
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
