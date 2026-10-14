//! App with the auth and admin providers, and seeded users.

use rustcms_admin::AdminProvider;
use rustcms_auth::{ADMIN_GUARD, Actor, AuthProvider, AuthState, RbacService};
use rustcms_core::models::user;
use rustcms_core::{AppBuilder, Config};
use rustcms_testing::{Factory, TestApp, TestDb, UserFactory};

use super::{CategoryResource, NoteResource, PostResource, TagResource};

/// Vietnamese labels of the test resources (`[posts]` overrides for that resource).
pub const VI: &str = r#"
"Post" = "Bài viết"
"Posts" = "Bài viết"
"Title" = "Tiêu đề"
"Category" = "Danh mục"
"Categories" = "Danh mục"

[posts]
"Content" = "Nội dung bài"
"Publish" = "Xuất bản"
"#;

pub fn build(config: Config) -> AppBuilder {
    // Each app stores its uploads in its own temporary directory, with small limits.
    let root = std::env::temp_dir().join(format!("rustcms-media-{}", uuid::Uuid::now_v7()));
    let config = config
        .with("storage.root", root.to_string_lossy())
        .and_then(|c| c.with("storage.max_upload_bytes", 64 * 1024))
        .expect("storage config");
    let mut app = AppBuilder::new(config);
    app.provider(rustcms_core::storage::StorageProvider);
    app.provider(AuthProvider::new().permission(ADMIN_GUARD, "posts.update_own", "posts"));
    app.provider(
        AdminProvider::new()
            .brand("Test CMS")
            .extension_script("test-widgets", "window.rustcmsTest = 1;")
            .extension_style("test-widgets", ".x{color:red}")
            .media()
            .resource::<CategoryResource>()
            .resource::<PostResource>()
            .resource::<TagResource>()
            .resource::<NoteResource>()
            .translations("vi", VI),
    );
    app
}

pub struct Users {
    pub admin: user::Model,
    pub editor: user::Model,
    pub member: user::Model,
}

/// Admin (super admin), editor (posts + read categories), member (nothing).
pub async fn setup(db: TestDb) -> (TestApp, Users) {
    db.migrate::<rustcms_migration::Migrator>().await;
    super::create_tables(db.conn()).await;
    let app = TestApp::new(build, db).await;
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
