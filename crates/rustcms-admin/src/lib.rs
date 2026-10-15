//! Filament-style admin engine for rustcms.
//!
//! Applications implement [`Resource`] for their SeaORM entities and register
//! them in a [`Registry`]; the engine serves generic, authorized CRUD through
//! the object-safe [`DynResource`].
#![warn(missing_docs)]

pub mod contract;
pub mod engine;
pub mod features;
pub mod forms;
pub mod http;
pub mod panel;
pub mod resources;
pub mod tables;
pub mod ui;

// Module paths kept from before the layout by role (R1).
pub use contract as schema;
pub(crate) use engine::adapter;
pub use engine::{context, idempotency, registry, wire};
pub use features::{audit, media, queue, rich_text, settings, system};
pub(crate) use panel::provider;
pub use panel::spa;
pub use resources::{action, field, policy, resource};
pub use settings::{SettingField, SettingKind, SettingsMeta, SettingsPage};

pub use action::{Action, ActionOutcome, BoxFuture};
pub use context::Ctx;
pub use field::{Field, Operation};
pub use http::{AdminCtx, AdminPanel};
pub use policy::{PermissionPolicy, Policy};
pub use provider::AdminProvider;
pub use registry::{DynResource, Registry};
pub use resource::{
    Envelope, Filter, FilterOp, KeyStrategy, Limits, ListQuery, Page, PageMeta, Relation, Resource, ResourceEntity,
    ResourceMeta, Trashed,
};
pub use schema::{ResourceInfo, ResourceSchema, SchemaOperation};
pub use ui::{
    BadgeColumn, BooleanColumn, Checkbox, CheckboxList, CustomInput, DateColumn, DatePicker, DateRangeFilter,
    DateTimePicker, FileUpload, Form, Grid, Hidden, KeyValue, NumberInput, RichEditor, SecretInput, Section,
    SelectFilter, SelectInput, Tab, Table, Tabs, TernaryFilter, TextColumn, TextInput, Textarea, Toggle,
};

/// A JSON object in wire format (input, hook data, trusted records).
pub type Data = serde_json::Map<String, serde_json::Value>;
