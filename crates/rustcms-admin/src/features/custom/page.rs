//! Custom pages and slots: declared in Rust (permissions, data), rendered
//! from a server template or by an extension script.

use std::marker::PhantomData;
use std::sync::Arc;

use async_trait::async_trait;
use rustcms_core::AppResult;
use serde_json::Value;

use crate::context::Ctx;
use crate::resource::Envelope;

/// How a page or slot is drawn.
#[derive(Debug, Clone, Copy)]
pub enum PageView {
    /// A server template (minijinja, HTML escaping always on), usually
    /// `include_str!` of a file in `resources/views/admin/`.
    Template(&'static str),
    /// An extension script that registered `vendor/name` with
    /// `window.rustcms.registerPage` (or `registerSlot`).
    Script(&'static str),
}

/// What the navigation shows and who may open a page.
#[derive(Debug, Clone)]
pub struct PanelPageMeta {
    pub(crate) label: String,
    pub(crate) icon: Option<String>,
    pub(crate) group: Option<String>,
    pub(crate) permissions: Vec<String>,
    pub(crate) declared: Vec<String>,
}

impl PanelPageMeta {
    /// A page titled `label`.
    pub fn new(label: impl Into<String>) -> Self {
        Self { label: label.into(), icon: None, group: None, permissions: Vec::new(), declared: Vec::new() }
    }

    /// Navigation icon (as resources').
    pub fn icon(mut self, icon: impl Into<String>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    /// Navigation group.
    pub fn group(mut self, group: impl Into<String>) -> Self {
        self.group = Some(group.into());
        self
    }

    /// A permission the user must hold (every one given; at least one is required).
    pub fn permission(mut self, permission: impl Into<String>) -> Self {
        self.permissions.push(permission.into());
        self
    }

    /// Like [`Self::permission`], and declares it for `permission:sync` (a
    /// permission of this page alone, e.g. `reports.view`).
    pub fn declare_permission(mut self, permission: impl Into<String>) -> Self {
        let permission = permission.into();
        self.declared.push(permission.clone());
        self.permissions.push(permission);
        self
    }
}

/// A page of the panel at `/admin/p/{SLUG}`, like a Filament custom page.
///
/// `data` is application code with the user's [`Ctx`]: read records through
/// [`Ctx::records`] / [`Ctx::record`] (scope, policy and readable fields
/// apply); anything else it returns, it must have authorized itself.
#[async_trait]
pub trait PanelPage: Send + Sync + 'static {
    /// URL segment: 1–64 of `a-z`, `0-9`, `-`.
    const SLUG: &'static str;

    /// Title, navigation and permissions.
    fn meta() -> PanelPageMeta;

    /// Template or script.
    fn view() -> PageView;

    /// What the template (as `data`) or the script (`ctx.data`) gets.
    async fn data(_ctx: &Ctx) -> AppResult<Value> {
        Ok(Value::Null)
    }
}

/// Where a slot appears.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// Beside a record of the resource registered under this slug (its view page).
    RecordAside(&'static str),
}

/// A slot's heading and who may see it.
#[derive(Debug, Clone)]
pub struct SlotMeta {
    pub(crate) label: Option<String>,
    pub(crate) permissions: Vec<String>,
}

impl SlotMeta {
    /// A slot (no heading).
    pub fn new() -> Self {
        Self { label: None, permissions: Vec::new() }
    }

    /// Heading above the slot.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// A permission the user must hold (every one given; at least one is required).
    pub fn permission(mut self, permission: impl Into<String>) -> Self {
        self.permissions.push(permission.into());
        self
    }
}

impl Default for SlotMeta {
    fn default() -> Self {
        Self::new()
    }
}

/// Content shown in a place of a built-in page (beside a record).
///
/// `data` gets the record as the user sees it (scope, policy, readable
/// fields): read anything else through [`Ctx::records`] / [`Ctx::record`], or
/// authorize it yourself.
#[async_trait]
pub trait PanelSlot: Send + Sync + 'static {
    /// Name: 1–64 of `a-z`, `0-9`, `-`.
    const NAME: &'static str;

    /// Where it appears.
    fn placement() -> Placement;

    /// Heading and permissions.
    fn meta() -> SlotMeta;

    /// Template or script.
    fn view() -> PageView;

    /// What the template or script gets.
    async fn data(ctx: &Ctx, record: &Envelope) -> AppResult<Value>;
}

/// Object-safe pages.
#[async_trait]
pub(crate) trait DynPage: Send + Sync {
    fn slug(&self) -> &'static str;
    fn meta(&self) -> &PanelPageMeta;
    fn view(&self) -> PageView;
    async fn data(&self, ctx: &Ctx) -> AppResult<Value>;
}

pub(crate) struct PageAdapter<P: PanelPage> {
    meta: PanelPageMeta,
    view: PageView,
    page: PhantomData<P>,
}

impl<P: PanelPage> PageAdapter<P> {
    pub(crate) fn shared() -> Arc<dyn DynPage> {
        Arc::new(Self { meta: P::meta(), view: P::view(), page: PhantomData })
    }
}

#[async_trait]
impl<P: PanelPage> DynPage for PageAdapter<P> {
    fn slug(&self) -> &'static str {
        P::SLUG
    }

    fn meta(&self) -> &PanelPageMeta {
        &self.meta
    }

    fn view(&self) -> PageView {
        self.view
    }

    async fn data(&self, ctx: &Ctx) -> AppResult<Value> {
        P::data(ctx).await
    }
}

/// Object-safe slots.
#[async_trait]
pub(crate) trait DynSlot: Send + Sync {
    fn name(&self) -> &'static str;
    fn placement(&self) -> Placement;
    fn meta(&self) -> &SlotMeta;
    fn view(&self) -> PageView;
    async fn data(&self, ctx: &Ctx, record: &Envelope) -> AppResult<Value>;
}

pub(crate) struct SlotAdapter<S: PanelSlot> {
    meta: SlotMeta,
    view: PageView,
    placement: Placement,
    slot: PhantomData<S>,
}

impl<S: PanelSlot> SlotAdapter<S> {
    pub(crate) fn shared() -> Arc<dyn DynSlot> {
        Arc::new(Self { meta: S::meta(), view: S::view(), placement: S::placement(), slot: PhantomData })
    }
}

#[async_trait]
impl<S: PanelSlot> DynSlot for SlotAdapter<S> {
    fn name(&self) -> &'static str {
        S::NAME
    }

    fn placement(&self) -> Placement {
        self.placement
    }

    fn meta(&self) -> &SlotMeta {
        &self.meta
    }

    fn view(&self) -> PageView {
        self.view
    }

    async fn data(&self, ctx: &Ctx, record: &Envelope) -> AppResult<Value> {
        S::data(ctx, record).await
    }
}
