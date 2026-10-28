//! App with the auth and admin providers, and seeded users.

use rustcms_admin::AdminProvider;
use rustcms_auth::{ADMIN_GUARD, Actor, AuthProvider, AuthState, RbacService};
use rustcms_core::models::user;
use rustcms_core::{AppBuilder, Config};
use rustcms_testing::{Factory, TestApp, TestDb, UserFactory};

use rustcms_admin::{Ctx, Section, SettingField, SettingsMeta, SettingsPage, TextInput, components};
use rustcms_core::ValidationErrors;
use rustcms_core::settings::{Secret, Settings};
use serde::{Deserialize, Serialize};

use super::{CategoryResource, NoteResource, PostResource, TagResource};

/// A settings group of every kind (`settings.general.*`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralSettings {
    pub site_name: String,
    pub motto: Option<String>,
    pub open: bool,
    pub class_size: i32,
    pub level: String,
    pub links: std::collections::BTreeMap<String, String>,
    pub starts: Option<chrono::NaiveDate>,
    pub smtp_password: Secret,
    pub api_key: Secret,
}

impl Default for GeneralSettings {
    fn default() -> Self {
        Self {
            site_name: "Academy".into(),
            motto: None,
            open: true,
            class_size: 30,
            level: "basic".into(),
            links: Default::default(),
            starts: None,
            smtp_password: Secret::default(),
            api_key: Secret::default(),
        }
    }
}

impl Settings for GeneralSettings {
    const GROUP: &'static str = "general";
}

#[async_trait::async_trait]
impl SettingsPage for GeneralSettings {
    fn meta() -> SettingsMeta {
        SettingsMeta::new("General").icon("settings")
    }

    fn fields() -> Vec<SettingField> {
        vec![
            SettingField::text("site_name").required().max_length(20),
            SettingField::text("motto").nullable(),
            SettingField::boolean("open"),
            SettingField::integer("class_size").help("Learners per class"),
            SettingField::select("level", [("basic", "Basic"), ("advanced", "Advanced")]),
            SettingField::key_value("links").max_items(3),
            SettingField::date("starts").nullable(),
            SettingField::secret("smtp_password").label("SMTP password"),
            SettingField::secret("api_key").audit_redact(),
        ]
    }

    fn form() -> rustcms_admin::Form {
        rustcms_admin::Form::new().schema(components![
            Section::make("Site")
                .schema(components![TextInput::make("site_name"), rustcms_admin::Textarea::make("motto")]),
            rustcms_admin::Toggle::make("open"),
            rustcms_admin::NumberInput::make("class_size"),
            rustcms_admin::SelectInput::make("level"),
            rustcms_admin::KeyValue::make("links"),
            rustcms_admin::DatePicker::make("starts"),
            rustcms_admin::SecretInput::make("smtp_password"),
            rustcms_admin::SecretInput::make("api_key"),
        ])
    }

    async fn validate(ctx: &Ctx, settings: &Self) -> Result<(), ValidationErrors> {
        // Runs inside the write (its transaction, the user's current grants).
        if ctx.txn.is_none() {
            return Err(ValidationErrors::field("site_name", "is invalid"));
        }
        // A careless message quoting a secret: the server redacts it.
        if settings.site_name == "leak" {
            let quoted = settings.smtp_password.expose().unwrap_or_default();
            return Err(ValidationErrors::field("site_name", format!("clashes with {quoted}")));
        }
        if settings.class_size < 1 {
            return Err(ValidationErrors::field("class_size", "is out of range"));
        }
        Ok(())
    }

    /// Notes `(before, after, secret set after, stored row seen)`; a site named
    /// `rollback` fails.
    async fn after_save(
        _state: &rustcms_core::AppState,
        ctx: &Ctx,
        txn: &sea_orm::DatabaseTransaction,
        before: &Self,
        after: &Self,
    ) -> rustcms_core::AppResult<()> {
        use sea_orm::EntityTrait;
        assert!(ctx.txn.is_some(), "the save's context");
        // In the save's transaction: the new values are written.
        let row = rustcms_core::models::setting::Entity::find_by_id("general".to_owned()).one(txn).await?;
        let seen = row.is_some_and(|r| r.value["site_name"] == serde_json::json!(after.site_name));
        SAVED_GENERAL.lock().unwrap().push((
            before.site_name.clone(),
            after.site_name.clone(),
            after.smtp_password.expose().is_some(),
            seen,
        ));
        if after.site_name == "leak-old" {
            // A careless message quoting the secret as it was: the server redacts it.
            let quoted = before.smtp_password.expose().unwrap_or_default();
            return Err(ValidationErrors::field("site_name", format!("was set with {quoted}")).into());
        }
        if after.site_name == "leak-bad" {
            // Not a validation error: its message is sent too, so it is redacted the same.
            let quoted = after.smtp_password.expose().unwrap_or_default();
            return Err(rustcms_core::AppError::BadRequest(format!("refused {quoted}")));
        }
        if after.site_name == "leak-new" {
            let quoted = after.smtp_password.expose().unwrap_or_default();
            return Err(ValidationErrors::field("site_name", format!("is now set with {quoted}")).into());
        }
        if after.site_name == "rollback" { Err(rustcms_core::AppError::internal("rollback")) } else { Ok(()) }
    }
}

/// Settings that reference files of the media library (`settings.brand.*`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BrandSettings {
    pub logo: Option<uuid::Uuid>,
    pub slides: Vec<uuid::Uuid>,
}

impl Settings for BrandSettings {
    const GROUP: &'static str = "brand";
}

#[async_trait::async_trait]
impl SettingsPage for BrandSettings {
    fn meta() -> SettingsMeta {
        SettingsMeta::new("Brand")
    }

    fn fields() -> Vec<SettingField> {
        vec![SettingField::media("logo").accept(&["image/*"]).max_size(10_000), SettingField::media_list("slides", 2)]
    }
}

/// What `GeneralSettings::after_save` saw.
#[allow(dead_code)]
pub static SAVED_GENERAL: std::sync::Mutex<Vec<(String, String, bool, bool)>> = std::sync::Mutex::new(Vec::new());

/// Vietnamese labels of the test resources (`[posts]` overrides for that resource).
pub const VI: &str = r#"
"Writing" = "Viết bài"
"Edit own posts" = "Sửa bài của mình"
"Post" = "Bài viết"
"Posts" = "Bài viết"
"Title" = "Tiêu đề"
"Category" = "Danh mục"
"Categories" = "Danh mục"
"General" = "Chung"
"SMTP password" = "Mật khẩu SMTP"

[posts]
"Content" = "Nội dung bài"
"Publish" = "Xuất bản"
"#;

/// The test dashboard: posts (a per-record policy, a scope), notes (soft deletes).
pub fn dashboard() -> rustcms_admin::dashboard::Dashboard {
    use rustcms_admin::dashboard::{Bucket, Dashboard, Widget};
    use rustcms_admin::resource::FilterOp;
    Dashboard::new()
        .widget(Widget::stat("posts_total", "posts").label("Posts"))
        .widget(
            Widget::stat("published_posts", "posts")
                .filter("published", FilterOp::Eq, serde_json::json!(true))
                .compare_last_days(30, "published_at"),
        )
        .widget(Widget::chart("posts_per_day", "posts", "published_at").per(Bucket::Day).last(7).span(2))
        .widget(Widget::recent("latest_posts", "posts").columns(&["published_at"]).sort("title").limit(5))
        .widget(Widget::stat("notes_total", "notes"))
}

/// Boots an app with `dashboard` instead (panics on a bad widget).
pub fn check_dashboard(dashboard: rustcms_admin::dashboard::Dashboard) {
    let config = rustcms_testing::test_config("sqlite::memory:");
    let mut app = AppBuilder::new(config);
    app.provider(AuthProvider::new());
    app.provider(
        AdminProvider::new()
            .resource::<CategoryResource>()
            .resource::<PostResource>()
            .resource::<TagResource>()
            .resource::<NoteResource>()
            .dashboard(dashboard),
    );
}

pub fn build(config: Config) -> AppBuilder {
    build_extended(config, |admin| admin)
}

/// [`build`] with the panel extended by `extend` (more resources).
pub fn build_extended(config: Config, extend: fn(AdminProvider) -> AdminProvider) -> AppBuilder {
    // Each app stores its uploads in its own temporary directory, with small limits.
    let root = std::env::temp_dir().join(format!("rustcms-media-{}", uuid::Uuid::now_v7()));
    let config = config
        .with("storage.root", root.to_string_lossy())
        .and_then(|c| c.with("storage.max_upload_bytes", 64 * 1024))
        .expect("storage config");
    let mut app = AppBuilder::new(config);
    app.provider(rustcms_core::storage::StorageProvider);
    app.provider(rustcms_core::mail::MailProvider);
    app.provider(
        AuthProvider::new()
            .permission(ADMIN_GUARD, "posts.update_own", "posts")
            .permission_area("writing", rustcms_auth::PermissionArea::new("Writing").sort(10))
            .permission_meta(
                ADMIN_GUARD,
                "posts.update_own",
                rustcms_auth::PermissionMeta::new()
                    .label("Edit own posts")
                    .description("Edit the posts you wrote")
                    .area("writing"),
            )
            .permission_preset(
                "writer",
                rustcms_auth::PermissionPreset::new("Writer").description("Writes posts").permissions([
                    "posts.view_any",
                    "posts.view",
                    "posts.create",
                    "users.view_any",
                ]),
            ),
    );
    app.provider(extend(
        AdminProvider::new()
            .brand("Test CMS")
            .extension_script("test-widgets", "window.rustcmsTest = 1;")
            .extension_style("test-widgets", ".x{color:red}")
            .media()
            .resource::<CategoryResource>()
            .resource::<PostResource>()
            .resource::<TagResource>()
            .resource::<NoteResource>()
            .settings::<GeneralSettings>()
            .settings::<BrandSettings>()
            .mail_settings()
            .dashboard(dashboard())
            .translations("vi", VI),
    ));
    app
}

pub struct Users {
    pub admin: user::Model,
    pub editor: user::Model,
    pub member: user::Model,
}

/// Admin (super admin), editor (posts + read categories), member (nothing).
pub async fn setup(db: TestDb) -> (TestApp, Users) {
    let config = rustcms_testing::test_config(db.url());
    setup_with(db, config).await
}

/// [`setup`] with `config`.
pub async fn setup_with(db: TestDb, config: Config) -> (TestApp, Users) {
    setup_built(db, config, build).await
}

/// [`setup_with`] for an app built by `make` (the panel of [`build`], plus more).
#[allow(dead_code)]
pub async fn setup_built(db: TestDb, config: Config, make: fn(Config) -> AppBuilder) -> (TestApp, Users) {
    db.migrate::<rustcms_migration::Migrator>().await;
    super::create_tables(db.conn()).await;
    let app = TestApp::with_config(make, config, db).await;
    let auth = app.state().service::<AuthState>().unwrap();
    let db = app.state().db();
    let rbac = RbacService::new(db, &auth.resolver);
    rbac.sync_permissions(&auth.permissions.all()).await.unwrap();

    let admin = UserFactory::new().name("Admin").create(db).await;
    let editor = UserFactory::new().name("Editor").create(db).await;
    let member = UserFactory::new().name("Member").create(db).await;
    let super_admin = rustcms_auth::entities::role::super_admin_id(db, ADMIN_GUARD).await.unwrap().unwrap();
    rbac.assign_role(Actor::System, admin.id, super_admin).await.unwrap();
    // Super admin of every guard (may replace other users' credentials).
    let api_super = rustcms_auth::entities::role::super_admin_id(db, rustcms_auth::API_GUARD).await.unwrap().unwrap();
    rbac.assign_role(Actor::System, admin.id, api_super).await.unwrap();
    let permissions: Vec<String> = [
        "posts.view_any",
        "posts.view",
        "posts.create",
        "posts.update",
        "posts.delete",
        "categories.view_any",
        "categories.view",
        "notes.view_any",
        "notes.view",
        "notes.create",
        "notes.delete",
        "notes.restore",
    ]
    .iter()
    .map(|p| (*p).to_owned())
    .collect();
    let role = rbac.create_role(Actor::System, ADMIN_GUARD, "editor", &permissions).await.unwrap();
    rbac.assign_role(Actor::System, editor.id, role.id).await.unwrap();
    (app, Users { admin, editor, member })
}
