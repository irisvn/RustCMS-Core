//! Filament-style admin engine for rustcms.
//!
//! Applications implement [`Resource`] for their SeaORM entities and register
//! them in a [`Registry`]; the engine serves generic, authorized CRUD through
//! the object-safe [`DynResource`].
#![warn(missing_docs)]

pub mod action;
mod adapter;
pub mod context;
pub mod field;
pub mod policy;
pub mod registry;
pub mod resource;
pub mod wire;

pub use action::{Action, ActionOutcome, BoxFuture};
pub use context::Ctx;
pub use field::{Field, Operation};
pub use policy::{PermissionPolicy, Policy};
pub use registry::{DynResource, Registry};
pub use resource::{
    Envelope, Filter, FilterOp, KeyStrategy, Limits, ListQuery, Page, PageMeta, Resource, ResourceEntity, ResourceMeta,
};

/// A JSON object in wire format (input, hook data, trusted records).
pub type Data = serde_json::Map<String, serde_json::Value>;
