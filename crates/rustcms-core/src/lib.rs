//! Core primitives shared by every rustcms crate.
//!
//! M0 scope: the error model and the authenticated [`Principal`]. The app
//! builder, config and console land in M1.
#![warn(missing_docs)]

pub mod error;
pub mod principal;

pub use error::{AppError, AppResult, ValidationErrors};
pub use principal::Principal;
