//! Public paths kept when modules were grouped by role (R1): every path that
//! existed before still names the same item, and the admin API route list is
//! unchanged. A compile error here is a broken public path.

#![allow(unused_imports)]

use rustcms_admin::action::{Action, ActionOutcome};
use rustcms_admin::context::Ctx;
use rustcms_admin::field::{Field, Operation};
use rustcms_admin::http::{AdminCtx, AdminPanel, DataResponse, PanelResponse, PanelUser, routes};
use rustcms_admin::idempotency::{MAX_LIVE_KEYS, prune};
use rustcms_admin::media::entity::{Entity as MediaEntity, Model as MediaModel};
use rustcms_admin::media::{MediaItem, MediaPolicy, MediaResource, SLUG, clean_name};
use rustcms_admin::policy::{PermissionPolicy, Policy, ability};
use rustcms_admin::registry::{DynResource, Registry};
use rustcms_admin::resource::{
    Envelope, Filter, FilterOp, KeyStrategy, Limits, ListQuery, Page, PageMeta, Relation, Resource, ResourceEntity,
    ResourceMeta, Trashed,
};
use rustcms_admin::rich_text::{Document, DocumentError, MAX_BYTES, Mark, Node, media_ids, parse, safe_href, to_html};
use rustcms_admin::schema::{InputSchema, RelationSchema, ResourceInfo, ResourceSchema, SchemaOperation, TableSchema};
use rustcms_admin::spa::Extension;
use rustcms_admin::system::{PermissionInfo, RoleRef, SystemRole, SystemUser, UserPage};
use rustcms_admin::ui::{
    BadgeColumn, BooleanColumn, Checkbox, CheckboxList, Component, CustomInput, DateColumn, DatePicker,
    DateRangeFilter, DateTimePicker, FileUpload, Form, Grid, Hidden, Input, InputType, KeyValue, Layout, LayoutType,
    NumberInput, RichEditor, Section, SelectFilter, SelectInput, SelectOption, Tab, Table, TableColumn, TableFilter,
    Tabs, TernaryFilter, TextColumn, TextInput, Textarea, Toggle,
};
use rustcms_admin::wire::FieldKind;
use rustcms_admin::{AdminProvider, Data, components};

#[test]
fn media_prune_and_system_routes_keep_their_paths() {
    let _ = rustcms_admin::media::prune;
    let _ = rustcms_admin::system::routes;
    let _ = Form::new().schema(components![TextInput::make("title")]);
}

#[test]
fn the_admin_route_list_is_unchanged() {
    let listed: Vec<String> = routes("/admin").into_iter().map(|(m, p)| format!("{m} {p}")).collect();
    let expected = [
        "GET /admin/api/panel",
        "GET /admin/api/resources/{slug}/schema",
        "GET /admin/api/resources/{slug}/records",
        "POST /admin/api/resources/{slug}/records",
        "GET /admin/api/resources/{slug}/records/{key}",
        "PATCH /admin/api/resources/{slug}/records/{key}",
        "DELETE /admin/api/resources/{slug}/records/{key}",
        "POST /admin/api/resources/{slug}/records/{key}/restore",
        "DELETE /admin/api/resources/{slug}/records/{key}/force",
        "GET /admin/api/resources/{slug}/records/{key}/relations/{relation}",
        "POST /admin/api/resources/{slug}/records/{key}/relations/{relation}",
        "POST /admin/api/resources/{slug}/records/{key}/relations/{relation}/attach",
        "POST /admin/api/resources/{slug}/records/{key}/relations/{relation}/detach",
        "PATCH /admin/api/resources/{slug}/records/{key}/relations/{relation}/links/{target}",
        "GET /admin/api/resources/{slug}/records/{key}/relations/{relation}/options",
        "POST /admin/api/resources/{slug}/actions/{action}",
        "GET /admin/api/resources/{slug}/actions/{action}/fields/{field}/options",
        "GET /admin/api/resources/{slug}/fields/{field}/options",
        "POST /admin/api/media",
        "GET /admin/api/media/{id}/file",
        "GET /admin/api/system/users",
        "POST /admin/api/system/users",
        "GET /admin/api/system/users/{id}",
        "PATCH /admin/api/system/users/{id}",
        "DELETE /admin/api/system/users/{id}",
        "PUT /admin/api/system/users/{id}/roles",
        "PUT /admin/api/system/users/{id}/active",
        "PUT /admin/api/system/users/{id}/password",
        "GET /admin/api/system/roles",
        "POST /admin/api/system/roles",
        "GET /admin/api/system/roles/{id}",
        "DELETE /admin/api/system/roles/{id}",
        "PUT /admin/api/system/roles/{id}/permissions",
        "GET /admin/api/system/permissions",
        "GET /admin/api/settings/{group}",
        "PUT /admin/api/settings/{group}",
        "GET /admin/api/audit",
        "GET /admin/api/resources/{slug}/records/{key}/history",
        "GET /admin/api/queue",
        "GET /admin/api/queue/jobs",
        "POST /admin/api/queue/jobs/{id}/retry",
        "POST /admin/api/queue/jobs/{id}/forget",
        "POST /admin/api/queue/jobs/{id}/cancel",
        "GET /admin/api/pages/{slug}",
        "GET /admin/api/slots",
    ];
    assert_eq!(listed, expected);
}
