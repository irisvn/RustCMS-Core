//! rustcms: a Laravel- and Filament-inspired framework for admin panels and
//! headless CMS backends.
//!
//! Applications depend on this crate only (plus `sea-orm`, whose derive macros
//! need it by name). Start with `rustcms new <app>`.
#![warn(missing_docs)]

pub use rustcms_core::*;

/// Admin panel engine (resources, fields, policies).
pub use rustcms_admin as admin;

/// HTTP toolkit used by rustcms.
pub use axum;

/// Migrations: the SeaORM migration prelude plus the framework's own migrations.
pub mod migration {
    pub use rustcms_migration::migrations as framework;
    pub use sea_orm_migration::prelude::*;
}

/// Common imports for application code.
pub mod prelude {
    pub use crate::{AppBuilder, AppError, AppResult, AppState, ClientIp, Config, Inject, Json, ServiceProvider};
    pub use axum::Router;
    pub use axum::extract::{Path, Query, State};
    pub use axum::routing::{delete, get, patch, post, put};
    pub use serde_json::json;
}
