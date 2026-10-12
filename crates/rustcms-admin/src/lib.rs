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
pub mod http;
pub mod policy;
mod provider;
pub mod registry;
pub mod resource;
pub mod schema;
pub mod spa;
pub mod ui;
pub mod wire;

pub use action::{Action, ActionOutcome, BoxFuture};
pub use context::Ctx;
pub use field::{Field, Operation};
pub use http::{AdminCtx, AdminPanel};
pub use policy::{PermissionPolicy, Policy};
pub use provider::AdminProvider;
pub use registry::{DynResource, Registry};
pub use resource::{
    Envelope, Filter, FilterOp, KeyStrategy, Limits, ListQuery, Page, PageMeta, Resource, ResourceEntity, ResourceMeta,
};
pub use schema::{ResourceInfo, ResourceSchema, SchemaOperation};
pub use ui::{
    BadgeColumn, BooleanColumn, Checkbox, CheckboxList, DateColumn, DatePicker, DateRangeFilter, DateTimePicker, Form,
    Grid, Hidden, NumberInput, Section, SelectFilter, SelectInput, Table, TernaryFilter, TextColumn, TextInput,
    Textarea, Toggle,
};

/// A JSON object in wire format (input, hook data, trusted records).
pub type Data = serde_json::Map<String, serde_json::Value>;
