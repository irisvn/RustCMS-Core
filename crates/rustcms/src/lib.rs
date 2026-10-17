//! rustcms: a Laravel- and Filament-inspired framework for admin panels and
//! headless CMS backends.
//!
//! Applications depend on this crate only (plus `sea-orm`, whose derive macros
//! need it by name). Start with `rustcms new <app>`.
#![warn(missing_docs)]

pub use rustcms_core::*;

/// Admin panel engine (resources, fields, policies).
pub use rustcms_admin as admin;

/// Guards, sessions, tokens, roles and permissions.
pub use rustcms_auth as auth;

/// HTTP toolkit used by rustcms.
pub use axum;

/// Command-line parser of console commands (`AppCommand::definition`).
pub use clap;

/// `use rustcms::async_trait;` then `#[async_trait]` for policies, hooks and migrations
/// (no extra dependency; `#[rustcms::…]` paths are reserved by rustc's `rustc` prefix rule).
pub use async_trait::async_trait;

/// Migrations: the SeaORM migration prelude plus the framework's own migrations.
pub mod migration {
    pub use rustcms_migration::migrations as framework;
    pub use rustcms_migration::schema::{
        ColumnDefExt, add_column, create_index, create_table, drop_column, drop_table,
    };
    pub use sea_orm_migration::prelude::*;
}

/// Common imports for application code.
pub mod prelude {
    pub use crate::admin::AdminProvider;
    pub use crate::auth::{Access, AdminAccess, AdminRoutes, ApiRoutes, Auth, AuthProvider, MaybeAuth, Throttle};
    pub use crate::{
        ApiPage, AppBuilder, AppError, AppResult, AppState, ClientIp, Config, Inject, Json, Pagination,
        ServiceProvider, Validate, Validated, ValidationErrors,
    };
    pub use axum::Router;
    pub use axum::extract::{Path, Query, State};
    pub use axum::http::{Method, StatusCode};
    pub use axum::routing::{delete, get, patch, post, put};
    pub use serde_json::json;
}
