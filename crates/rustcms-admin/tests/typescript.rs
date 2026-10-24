//! TypeScript bindings of the admin protocol for the SPA
//! (`admin-ui/src/api/types.gen.ts`). Regenerate with
//! `UPDATE_TYPES=1 cargo test -p rustcms-admin --features ts --test typescript`.
#![cfg(feature = "ts")]

use rustcms_admin::http::{DataResponse, PanelResponse, PanelUploads, PanelUser};
use rustcms_admin::schema::*;
use rustcms_admin::system::{
    PermissionInfo, PermissionPresetInfo, RoleRef, SystemLinks, SystemRole, SystemUser, UserPage,
};
use rustcms_admin::ui::{Breakpoint, ColumnType, FilterType, FormWidth, InputType, LayoutType, SelectOption};
use rustcms_admin::{ActionOutcome, Envelope, Page, PageMeta};
use ts_rs::{Config, TS};

fn bindings() -> String {
    // u64 page numbers are JSON numbers; i64 *values* travel as strings inside `data`.
    let cfg = Config::new().with_large_int("number");
    let decls = [
        serde_json::Value::decl(&cfg),
        InputType::decl(&cfg),
        LayoutType::decl(&cfg),
        ColumnType::decl(&cfg),
        Breakpoint::decl(&cfg),
        FormWidth::decl(&cfg),
        FilterType::decl(&cfg),
        SelectOption::decl(&cfg),
        ResourceInfo::decl(&cfg),
        SchemaOperation::decl(&cfg),
        Rule::decl(&cfg),
        Relationship::decl(&cfg),
        rustcms_admin::schema::MediaSchema::decl(&cfg),
        rustcms_admin::schema::LinksSchema::decl(&cfg),
        rustcms_admin::schema::RelationKind::decl(&cfg),
        rustcms_admin::schema::RelationSchema::decl(&cfg),
        rustcms_admin::media::MediaItem::decl(&cfg),
        InputSchema::decl(&cfg),
        LayoutSchema::decl(&cfg),
        ComponentSchema::decl(&cfg),
        FormSchema::decl(&cfg),
        ColumnSchema::decl(&cfg),
        BadgeColor::decl(&cfg),
        FilterSchema::decl(&cfg),
        ActionSchema::decl(&cfg),
        TableSchema::decl(&cfg),
        ResourceSchema::decl(&cfg),
        Envelope::decl(&cfg),
        PageMeta::decl(&cfg),
        Page::decl(&cfg),
        ActionOutcome::decl(&cfg),
        rustcms_admin::LinkOutcome::decl(&cfg),
        rustcms_admin::LinkOption::decl(&cfg),
        rustcms_admin::LinkPage::decl(&cfg),
        rustcms_admin::exports::ExportView::decl(&cfg),
        rustcms_admin::ImportField::decl(&cfg),
        rustcms_admin::imports::RowIssue::decl(&cfg),
        rustcms_admin::imports::ImportView::decl(&cfg),
        rustcms_admin::imports::ImportPreview::decl(&cfg),
        PanelUser::decl(&cfg),
        PanelUploads::decl(&cfg),
        SystemLinks::decl(&cfg),
        RoleRef::decl(&cfg),
        SystemUser::decl(&cfg),
        UserPage::decl(&cfg),
        SystemRole::decl(&cfg),
        PermissionInfo::decl(&cfg),
        PermissionPresetInfo::decl(&cfg),
        rustcms_admin::settings::SettingsLink::decl(&cfg),
        rustcms_admin::settings::SecretStatus::decl(&cfg),
        rustcms_admin::settings::SettingsAbilities::decl(&cfg),
        rustcms_admin::settings::SettingsResponse::decl(&cfg),
        rustcms_admin::settings::MailTestResult::decl(&cfg),
        rustcms_admin::dashboard::Bucket::decl(&cfg),
        rustcms_admin::dashboard::WidgetInfo::decl(&cfg),
        rustcms_admin::dashboard::DashboardView::decl(&cfg),
        rustcms_admin::dashboard::ChartBucket::decl(&cfg),
        rustcms_admin::dashboard::RecentColumn::decl(&cfg),
        rustcms_admin::dashboard::RecentRecord::decl(&cfg),
        rustcms_admin::dashboard::WidgetData::decl(&cfg),
        rustcms_admin::audit::AuditActorView::decl(&cfg),
        rustcms_admin::audit::AuditEntryView::decl(&cfg),
        rustcms_admin::audit::AuditPageMeta::decl(&cfg),
        rustcms_admin::audit::AuditPage::decl(&cfg),
        rustcms_admin::queue::QueueStats::decl(&cfg),
        rustcms_admin::queue::QueueWorkers::decl(&cfg),
        rustcms_admin::queue::QueueOverview::decl(&cfg),
        rustcms_admin::queue::QueueUser::decl(&cfg),
        rustcms_admin::queue::QueueJob::decl(&cfg),
        rustcms_admin::queue::QueueJobPageMeta::decl(&cfg),
        rustcms_admin::queue::QueueJobPage::decl(&cfg),
        rustcms_admin::queue::QueueLastDay::decl(&cfg),
        rustcms_admin::queue::JobRun::decl(&cfg),
        rustcms_admin::queue::JobRunPage::decl(&cfg),
        rustcms_admin::queue::QueueJobDetail::decl(&cfg),
        rustcms_admin::schedules::ScheduleActionInfo::decl(&cfg),
        rustcms_admin::schedules::ScheduleRunInfo::decl(&cfg),
        rustcms_admin::schedules::ScheduleInfo::decl(&cfg),
        rustcms_admin::schedules::SchedulerInfo::decl(&cfg),
        rustcms_admin::schedules::ScheduleList::decl(&cfg),
        rustcms_admin::schedules::ScheduleRunPage::decl(&cfg),
        rustcms_admin::schedules::ScheduleRunResponse::decl(&cfg),
        rustcms_admin::custom::PageLink::decl(&cfg),
        rustcms_admin::custom::CustomContent::decl(&cfg),
        rustcms_admin::custom::CustomPageResponse::decl(&cfg),
        rustcms_admin::custom::SlotContent::decl(&cfg),
        rustcms_admin::custom::SlotsResponse::decl(&cfg),
        PanelResponse::decl(&cfg),
        DataResponse::<ts_rs::Dummy>::decl(&cfg),
    ];
    let mut out = String::from(
        "// Generated from the Rust schema DTOs by crates/rustcms-admin/tests/typescript.rs. Do not edit.\n\n",
    );
    for decl in decls {
        out.push_str("export ");
        out.push_str(&decl);
        out.push_str("\n\n");
    }
    out.truncate(out.trim_end().len());
    out.push('\n');
    out
}

#[test]
fn typescript_bindings_are_up_to_date() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../admin-ui/src/api/types.gen.ts");
    let expected = bindings();
    if std::env::var_os("UPDATE_TYPES").is_some() {
        std::fs::write(path, &expected).unwrap();
    }
    let actual = std::fs::read_to_string(path).unwrap_or_default();
    assert!(actual == expected, "admin-ui/src/api/types.gen.ts is stale: run with UPDATE_TYPES=1");
}
