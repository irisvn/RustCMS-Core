//! Core of the rustcms framework: configuration, application assembly, HTTP
//! stack, console, errors and the authenticated principal.
#![warn(missing_docs)]

pub mod app;
pub mod config;
pub mod console;
pub mod db;
pub mod error;
pub mod http;
pub mod key;
pub mod models;
pub mod principal;
pub mod telemetry;

pub use app::{App, AppBuilder, AppCommand, AppState, Inject, ServiceProvider};
pub use config::{Config, Environment};
pub use console::Console;
pub use error::{AppError, AppResult, ValidationErrors};
pub use http::{ClientIp, Json};
pub use key::AppKey;
pub use principal::Principal;
