//! Core of the rustcms framework: configuration, application assembly, HTTP
//! stack, console, errors and the authenticated principal.
#![warn(missing_docs)]

pub mod app;
pub mod audit;
pub mod config;
pub mod console;
pub mod db;
pub mod error;
pub mod http;
pub mod i18n;
pub mod jobs;
pub mod key;
pub mod models;
pub mod principal;
#[cfg(feature = "redis")]
pub mod redis;
pub mod settings;
pub mod storage;
pub mod telemetry;

pub use app::{App, AppBuilder, AppCommand, AppState, Inject, RouteInfo, ServiceProvider};
pub use config::{Config, Environment};
pub use console::Console;
pub use error::{AppError, AppResult, ValidationErrors};
pub use http::{ApiPage, ClientIp, Json, Pagination, Validate, Validated};
pub use key::AppKey;
pub use principal::{Credential, Principal};
