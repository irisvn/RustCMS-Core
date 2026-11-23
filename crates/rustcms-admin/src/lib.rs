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
pub mod infolists;
pub mod panel;
pub mod resources;
pub mod tables;
pub mod ui;

// Module paths kept from before the layout by role (R1).
pub use contract as schema;
pub use custom::{
    AdminBranding, Branding, BrandingError, BrandingSource, ColorScheme, PageView, Palette, PanelPage, PanelPageMeta,
    PanelSlot, Placement, SlotMeta, TemplateFile, Theme,
};
pub(crate) use engine::adapter;
pub use engine::{context, idempotency, registry, wire};
pub use features::{audit, custom, dashboard, exports, imports, media, queue, rich_text, schedules, settings, system};
pub(crate) use panel::provider;
pub use panel::spa;
pub use panel::spa::Requires;
pub use resources::{action, field, links, policy, resource};
pub use settings::{SettingField, SettingKind, SettingsMeta, SettingsPage};

pub use action::{Action, ActionField, ActionOutcome, BoxFuture};
pub use adapter::ImportField;
pub use context::Ctx;
pub use field::{Field, Operation};
pub use http::{AdminCtx, AdminPanel, LinkPage};
pub use links::{LinkOp, LinkOption, LinkOutcome, LinkRequest, RelationChange};
pub use policy::{PermissionPolicy, Policy};
pub use provider::AdminProvider;
pub use registry::{DynResource, Registry};
pub use resource::{
    Envelope, Filter, FilterOp, KeyStrategy, Limits, ListQuery, Page, PageMeta, Relation, RelationFilter, Resource,
    ResourceEntity, ResourceMeta, Trashed,
};
pub use schema::{ResourceInfo, ResourceSchema, SchemaOperation};
pub use ui::{
    BadgeColumn, BelongsToManySelect, BooleanColumn, BooleanEntry, Breakpoint, Checkbox, CheckboxList, Computed,
    ComputedEntry, Condition, CustomEntry, CustomInput, DateColumn, DatePicker, DateRangeFilter, DateTimePicker,
    FileUpload, Form, FormPatch, FormState, FormWidth, Grid, Hidden, Infolist, KeyValue, KeyValueEntry, MediaEntry,
    NumberInput, RelationEntry, RepeatableEntry, Repeater, RichEditor, RichTextEntry, RowId, Scope, SecretInput,
    SecretMapInput, Section, SelectFilter, SelectInput, SelectOption, Split, StateRow, Tab, Table, Tabs, TernaryFilter,
    TextColumn, TextEntry, TextInput, Textarea, Toggle, When, when, when_parent, when_root,
};

/// A JSON object in wire format (input, hook data, trusted records).
pub type Data = serde_json::Map<String, serde_json::Value>;
