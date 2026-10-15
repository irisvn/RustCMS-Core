//! Customizing the panel: theme assets, custom pages and slots (templates
//! escaped and sanitized, scripts with data), who sees them, and the
//! guarantees of records read through the context. SQLite, PostgreSQL, MySQL.

mod support;

use axum::http::StatusCode;
use rustcms_admin::{
    AdminProvider, ColorScheme, Ctx, Field, KeyStrategy, PageView, PanelPage, PanelPageMeta, PanelSlot, Placement,
    Resource, ResourceMeta, SlotMeta, TemplateFile, Theme,
};
use rustcms_auth::{ADMIN_GUARD, Actor, AuthProvider, AuthState, RbacService};
use rustcms_core::AppResult;
use rustcms_core::models::user;
use rustcms_testing::{Factory, TestApp, TestDb, UserFactory, db_test};
use sea_orm::sea_query::Value as SqlValue;
use sea_orm::{ActiveModelTrait, ColumnTrait, QueryFilter, Select, Set};
use serde_json::{Value, json};

use support::{CategoryResource, NoteResource, PostResource, TagResource, category, post};

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01";

/// Lists categories (as the user may see them) with hostile markup around.
struct OverviewPage;

#[async_trait::async_trait]
impl PanelPage for OverviewPage {
    const SLUG: &'static str = "overview";

    fn meta() -> PanelPageMeta {
        PanelPageMeta::new("Overview").icon("layers").group("Reports").permission("categories.view_any")
    }

    fn view() -> PageView {
        PageView::Template(
            r#"<section class="rc-card"><h2>{{ t("Categories") }}</h2><ul>
{% for c in data.items %}<li title="{{ c.title }}"><a href="{{ admin_url('r/categories/records/' ~ c.key) }}">{{ c.title }}</a></li>{% endfor %}
</ul>
<a href="javascript:alert(1)" onclick="steal()">bad</a><img src="https://evil.test/x.png"><script>alert(2)</script>
<div style="display:none" id="location">hidden</div><a href="//evil.test">protocol-relative</a>
{% if can("categories.view_secret") %}<p>secret area</p>{% endif %}
</section>"#,
        )
    }

    async fn data(ctx: &Ctx) -> AppResult<Value> {
        let page = ctx.records("categories", &Default::default()).await?;
        Ok(json!({"items": page.data}))
    }
}

/// Posts as a ledger: the price is declared but read only with
/// `ledger.view_price`; `secret` is not declared; each user sees their own.
struct LedgerResource;

#[async_trait::async_trait]
impl Resource for LedgerResource {
    type Entity = post::Entity;
    const SLUG: &'static str = "ledger";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Ledger")
            .title("title")
            .key(KeyStrategy::AppGenerated(|| SqlValue::Uuid(Some(uuid::Uuid::now_v7()))))
    }

    fn fields() -> Vec<Field> {
        vec![
            Field::column(post::Column::Title).required(),
            Field::column(post::Column::Price).readable(|ctx| ctx.can("ledger.view_price")),
        ]
    }

    fn scope(query: Select<post::Entity>, ctx: &Ctx) -> Select<post::Entity> {
        query.filter(post::Column::AuthorId.eq(ctx.principal.id.parse::<i64>().unwrap_or(-1)))
    }
}

/// The ledger for a script: what reaches the browser is what the user may read.
struct ReportPage;

#[async_trait::async_trait]
impl PanelPage for ReportPage {
    const SLUG: &'static str = "report";

    fn meta() -> PanelPageMeta {
        PanelPageMeta::new("Report").permission("ledger.view_any")
    }

    fn view() -> PageView {
        PageView::Script("test/report")
    }

    async fn data(ctx: &Ctx) -> AppResult<Value> {
        let page = ctx.records("ledger", &Default::default()).await?;
        Ok(serde_json::to_value(page.data).unwrap())
    }
}

/// Only for holders of its own permission (declared for `permission:sync`).
struct ReservedPage;

#[async_trait::async_trait]
impl PanelPage for ReservedPage {
    const SLUG: &'static str = "reserved";

    fn meta() -> PanelPageMeta {
        PanelPageMeta::new("Reserved").declare_permission("reports.view")
    }

    fn view() -> PageView {
        PageView::Template("<p>{{ t(\"Reserved\") }}</p>")
    }

    async fn data(ctx: &Ctx) -> AppResult<Value> {
        assert!(ctx.can("reports.view"), "never called without the permission");
        Ok(Value::Null)
    }
}

struct CategoryAside;

#[async_trait::async_trait]
impl PanelSlot for CategoryAside {
    const NAME: &'static str = "category-aside";

    fn placement() -> Placement {
        Placement::RecordAside("categories")
    }

    fn meta() -> SlotMeta {
        SlotMeta::new().label("About").permission("categories.view")
    }

    fn view() -> PageView {
        PageView::Template("<p class=\"rc-muted\">{{ data.title }} ({{ data.key }})</p>")
    }

    async fn data(_: &Ctx, record: &rustcms_admin::Envelope) -> AppResult<Value> {
        Ok(json!({"title": record.title, "key": record.key}))
    }
}

struct SecretAside;

#[async_trait::async_trait]
impl PanelSlot for SecretAside {
    const NAME: &'static str = "secret-aside";

    fn placement() -> Placement {
        Placement::RecordAside("categories")
    }

    fn meta() -> SlotMeta {
        SlotMeta::new().permission("categories.view_secret")
    }

    fn view() -> PageView {
        PageView::Script("test/secret")
    }

    async fn data(_: &Ctx, _: &rustcms_admin::Envelope) -> AppResult<Value> {
        panic!("never called without the permission")
    }
}

fn build(config: rustcms_core::Config) -> rustcms_core::AppBuilder {
    let mut app = rustcms_core::AppBuilder::new(config);
    app.provider(AuthProvider::new());
    app.provider(
        AdminProvider::new()
            .theme(
                Theme::new()
                    .primary("#2563eb")
                    .radius("0.75rem")
                    .font_family("Inter, system-ui, sans-serif")
                    .logo(PNG, "image/png")
                    .dark(|d| d.primary("#60a5fa"))
                    .color_scheme(ColorScheme::Dark),
            )
            .resource::<CategoryResource>()
            .resource::<PostResource>()
            .resource::<TagResource>()
            .resource::<NoteResource>()
            .resource::<LedgerResource>()
            .page::<OverviewPage>()
            .page::<ReportPage>()
            .page::<ReservedPage>()
            .slot::<CategoryAside>()
            .slot::<SecretAside>()
            .translations(
                "vi",
                "\"Categories\" = \"Danh mục\"\n\"About\" = \"Giới thiệu\"\n\"Overview\" = \"Tổng quan\"",
            ),
    );
    app
}

struct Users {
    admin: user::Model,
    editor: user::Model,
    member: user::Model,
}

async fn setup(db: TestDb) -> (TestApp, Users) {
    db.migrate::<rustcms_migration::Migrator>().await;
    support::create_tables(db.conn()).await;
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
    let permissions: Vec<String> =
        ["categories.view_any", "categories.view", "posts.view_any", "ledger.view_any", "ledger.view"]
            .iter()
            .map(|p| (*p).to_owned())
            .collect();
    let role = rbac.create_role(Actor::System, ADMIN_GUARD, "editor", &permissions).await.unwrap();
    rbac.assign_role(Actor::System, editor.id, role.id).await.unwrap();
    (app, Users { admin, editor, member })
}

async fn category(app: &TestApp, name: &str) -> i32 {
    let slug = format!("c-{}", uuid::Uuid::now_v7());
    category::ActiveModel { name: Set(name.into()), slug: Set(slug), ..Default::default() }
        .insert(app.state().db())
        .await
        .unwrap()
        .id
}

async fn pages(app: &TestApp, client: &rustcms_testing::TestClient<'_>) -> Value {
    client.get(&app.admin_api("panel")).await.json()["pages"].clone()
}

db_test!(pages_follow_permissions_and_never_probe, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    let editor = app.acting_as(&users.editor).await;
    let slugs: Vec<Value> = pages(&app, &editor).await.as_array().unwrap().iter().map(|p| p["slug"].clone()).collect();
    assert_eq!(slugs, [json!("overview"), json!("report")], "only pages the editor may open");
    // A page the user may not open, or does not exist: 403 / 404 once they may open some.
    editor.get(&app.admin_api("pages/reserved")).await.assert_status(StatusCode::FORBIDDEN);
    editor.get(&app.admin_api("pages/nope")).await.assert_status(StatusCode::NOT_FOUND);
    // Without access to any page, every slug is the same 403 (no probing).
    let member = app.acting_as(&users.member).await;
    assert_eq!(pages(&app, &member).await, json!([]));
    for slug in ["overview", "reserved", "nope"] {
        member.get(&app.admin_api(&format!("pages/{slug}"))).await.assert_status(StatusCode::FORBIDDEN);
    }
    // Declared for permission:sync; the super admin opens it.
    let auth = app.state().service::<AuthState>().unwrap();
    assert!(auth.permissions.all().values().flatten().any(|g| g == "pages"));
    let admin = app.acting_as(&users.admin).await;
    let reserved = admin.get(&app.admin_api("pages/reserved")).await;
    reserved.assert_status(StatusCode::OK);
    assert_eq!(reserved.json()["content"], json!({"kind": "html", "html": "<p>Reserved</p>"}));
});

db_test!(templates_escape_data_and_sanitize_markup, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    category(&app, "<script>alert(1)</script>\" onmouseover=\"x").await;
    category(&app, "Hidden drafts").await;
    let editor = app.acting_as(&users.editor).await;
    let page = editor.get(&app.admin_api("pages/overview")).await;
    page.assert_status(StatusCode::OK);
    let body = page.json();
    assert_eq!(body["title"], json!("Overview"));
    let html = body["content"]["html"].as_str().unwrap();
    // As attributes (values quote `"` as `&quot;`, so `name="` is a real
    // attribute): no handler, style, id, script URL or foreign source.
    let tags: Vec<&str> = html.split('<').skip(1).filter_map(|t| t.split('>').next()).collect();
    for tag in &tags {
        for absent in [
            " onclick=\"",
            " onmouseover=\"",
            " style=\"",
            " id=\"",
            "href=\"javascript",
            "href=\"//",
            "src=\"https://",
        ] {
            assert!(!tag.contains(absent), "{absent} in <{tag}>");
        }
    }
    for absent in ["<script", "secret area", "Hidden drafts"] {
        assert!(!html.contains(absent), "{absent} in {html}");
    }
    assert!(html.contains("&lt;script&gt;"), "data is escaped: {html}");
    assert!(html.contains("href=\"/admin/r/categories/records/"), "admin_url builds panel links: {html}");
    assert!(html.contains("<h2>Categories</h2>"));
    // In the user's language.
    let vi = app.acting_as(&users.editor).await.languages("vi");
    let html = vi.get(&app.admin_api("pages/overview")).await.json()["content"]["html"].as_str().unwrap().to_owned();
    assert!(html.contains("<h2>Danh mục</h2>"), "{html}");
});

db_test!(scripts_get_what_the_user_may_read, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    // A post of the editor, and one of the admin (outside the editor's scope).
    for (title, author) in [("Mine", users.editor.id), ("Theirs", users.admin.id)] {
        post::ActiveModel {
            id: Set(uuid::Uuid::now_v7()),
            title: Set(title.into()),
            author_id: Set(author),
            price: Set(Some(rust_decimal::Decimal::new(4242, 2))),
            secret: Set("undeclared-secret".into()),
            ..Default::default()
        }
        .insert(app.state().db())
        .await
        .unwrap();
    }
    let editor = app.acting_as(&users.editor).await;
    let report = editor.get(&app.admin_api("pages/report")).await;
    report.assert_status(StatusCode::OK);
    let content = &report.json()["content"];
    assert_eq!(content["kind"], json!("script"));
    assert_eq!(content["script"], json!("test/report"));
    let rows = content["data"].as_array().unwrap();
    assert_eq!(rows.len(), 1, "scope applies: {rows:?}");
    assert_eq!(rows[0]["title"], json!("Mine"));
    assert_eq!(rows[0]["data"]["title"], json!("Mine"));
    let text = content["data"].to_string();
    for absent in ["Theirs", "42.42", "price", "undeclared-secret", "\"secret\""] {
        assert!(!text.contains(absent), "{absent} reached the script: {text}");
    }
});

db_test!(slots_follow_the_record_and_their_permissions, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    let visible = category(&app, "Visible").await;
    let hidden = category(&app, "Hidden one").await;
    let editor = app.acting_as(&users.editor).await;
    let slots = |key: String| app.admin_api(&format!("slots?resource=categories&key={key}"));
    let shown = editor.get(&slots(visible.to_string())).await;
    shown.assert_status(StatusCode::OK);
    let data = shown.json()["data"].clone();
    assert_eq!(data.as_array().unwrap().len(), 1, "the secret slot is left out: {data}");
    assert_eq!(data[0]["name"], json!("category-aside"));
    assert_eq!(data[0]["label"], json!("About"));
    assert_eq!(data[0]["content"]["html"], json!(format!("<p class=\"rc-muted\">Visible ({visible})</p>")));
    // As the record page: out of scope or missing, 404; parameters checked.
    editor.get(&slots(hidden.to_string())).await.assert_status(StatusCode::NOT_FOUND);
    editor.get(&slots("999999".into())).await.assert_status(StatusCode::NOT_FOUND);
    editor.get(&app.admin_api("slots?resource=nope&key=1")).await.assert_status(StatusCode::NOT_FOUND);
    editor.get(&app.admin_api("slots?resource=categories")).await.assert_validation_error("key");
    editor.get(&app.admin_api("slots?resource=categories&key=1&x=1")).await.assert_validation_error("x");
    let member = app.acting_as(&users.member).await;
    member.get(&slots(visible.to_string())).await.assert_status(StatusCode::FORBIDDEN);
    // A resource that admits by permission does so before parameter errors (no probing).
    for query in ["resource=ledger&key=1&x=1", "resource=ledger", "resource=ledger&key=1&key=2"] {
        member.get(&app.admin_api(&format!("slots?{query}"))).await.assert_status(StatusCode::FORBIDDEN);
    }
});

db_test!(the_theme_is_served_as_checked_assets, |db: TestDb| async move {
    let (app, _users) = setup(db).await;
    let index = app.get("/admin").await;
    index.assert_status(StatusCode::OK);
    let html = index.text().replace("&quot;", "\"");
    assert!(html.contains("\"colorScheme\":\"dark\""), "{html}");
    let logo = html.split("\"logo\":\"").nth(1).unwrap().split('"').next().unwrap().to_owned();
    assert!(logo.starts_with("assets/ext/logo.") && logo.ends_with(".png"), "{logo}");
    let theme = html.split("assets/ext/theme.").nth(1).unwrap().split('"').next().unwrap().to_owned();
    let css = app.get(&format!("/admin/assets/ext/theme.{theme}")).await;
    css.assert_status(StatusCode::OK);
    assert!(
        css.text().contains("--primary:#2563eb;") && css.text().contains(":root[data-scheme=dark]{--primary:#60a5fa;}")
    );
    let image = app.get(&format!("/admin/{logo}")).await;
    image.assert_status(StatusCode::OK);
    assert_eq!(image.headers["content-type"], "image/png");
    assert_eq!(image.headers["x-content-type-options"], "nosniff");
    assert_eq!(image.body.as_ref(), PNG);
});

/// Drawn from a template file: embedded, and read again in local development.
struct DevPage;

fn dev_template() -> &'static str {
    static PATH: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    PATH.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("rustcms-views-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("dev.html").to_string_lossy().into_owned()
    })
}

#[async_trait::async_trait]
impl PanelPage for DevPage {
    const SLUG: &'static str = "dev";

    fn meta() -> PanelPageMeta {
        PanelPageMeta::new("Dev").permission("categories.view_any")
    }

    fn view() -> PageView {
        PageView::File(TemplateFile::__new("<p>embedded</p>", dev_template(), true))
    }

    async fn data(_: &Ctx) -> AppResult<Value> {
        Ok(json!({"n": 3}))
    }
}

/// Tests (and backends) using the template file take turns.
static DEV_FILE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn build_dev(mut config: rustcms_core::Config, env: rustcms_core::Environment) -> rustcms_core::AppBuilder {
    config.app.env = env;
    let mut app = rustcms_core::AppBuilder::new(config);
    app.provider(AuthProvider::new());
    app.provider(AdminProvider::new().page::<DevPage>());
    app
}

db_test!(template_files_are_read_again_in_local_development, |db: TestDb| async move {
    let _file = DEV_FILE.lock().await;
    db.migrate::<rustcms_migration::Migrator>().await;
    let app = TestApp::new(|c| build_dev(c, rustcms_core::Environment::Local), db).await;
    let db = app.state().db();
    let admin = UserFactory::new().create(db).await;
    let super_admin = rustcms_auth::entities::role::super_admin_id(db, ADMIN_GUARD).await.unwrap().unwrap();
    let auth = app.state().service::<AuthState>().unwrap();
    RbacService::new(db, &auth.resolver).assign_role(Actor::System, admin.id, super_admin).await.unwrap();
    let client = app.acting_as(&admin).await;
    let html = |page: rustcms_testing::TestResponse| page.json()["content"]["html"].clone();
    let path = dev_template();
    std::fs::write(path, "<p>first</p>").unwrap();
    assert_eq!(html(client.get(&app.admin_api("pages/dev")).await), json!("<p>first</p>"));
    // Edited: shown on the next request, rules as at boot.
    std::fs::write(path, "<p>second {{ data.n }}<script>x</script></p>").unwrap();
    assert_eq!(html(client.get(&app.admin_api("pages/dev")).await), json!("<p>second 3</p>"));
    std::fs::write(path, "{%autoescape false%}{{ data.n }}{% endautoescape %}").unwrap();
    client.get(&app.admin_api("pages/dev")).await.assert_status(StatusCode::INTERNAL_SERVER_ERROR);
    std::fs::write(path, "{{ unclosed").unwrap();
    client.get(&app.admin_api("pages/dev")).await.assert_status(StatusCode::INTERNAL_SERVER_ERROR);
    // Gone: the embedded copy.
    std::fs::remove_file(path).unwrap();
    assert_eq!(html(client.get(&app.admin_api("pages/dev")).await), json!("<p>embedded</p>"));
});

db_test!(template_files_stay_embedded_outside_local_development, |db: TestDb| async move {
    let _file = DEV_FILE.lock().await;
    db.migrate::<rustcms_migration::Migrator>().await;
    let app = TestApp::new(|c| build_dev(c, rustcms_core::Environment::Testing), db).await;
    let db = app.state().db();
    let admin = UserFactory::new().create(db).await;
    let super_admin = rustcms_auth::entities::role::super_admin_id(db, ADMIN_GUARD).await.unwrap().unwrap();
    let auth = app.state().service::<AuthState>().unwrap();
    RbacService::new(db, &auth.resolver).assign_role(Actor::System, admin.id, super_admin).await.unwrap();
    let client = app.acting_as(&admin).await;
    std::fs::write(dev_template(), "<p>edited</p>").unwrap();
    let page = client.get(&app.admin_api("pages/dev")).await;
    assert_eq!(page.json()["content"]["html"], json!("<p>embedded</p>"));
    std::fs::remove_file(dev_template()).unwrap();
});

fn all_resources() -> AdminProvider {
    AdminProvider::new()
        .resource::<CategoryResource>()
        .resource::<PostResource>()
        .resource::<TagResource>()
        .resource::<NoteResource>()
}

#[test]
fn misdeclared_pages_and_slots_stop_the_boot() {
    struct NoPermission;
    #[async_trait::async_trait]
    impl PanelPage for NoPermission {
        const SLUG: &'static str = "open";
        fn meta() -> PanelPageMeta {
            PanelPageMeta::new("Open")
        }
        fn view() -> PageView {
            PageView::Template("x")
        }
    }
    struct BadScript;
    #[async_trait::async_trait]
    impl PanelPage for BadScript {
        const SLUG: &'static str = "bad-script";
        fn meta() -> PanelPageMeta {
            PanelPageMeta::new("Bad").permission("x.view")
        }
        fn view() -> PageView {
            PageView::Script("not a name")
        }
    }
    struct BadTemplate;
    #[async_trait::async_trait]
    impl PanelPage for BadTemplate {
        const SLUG: &'static str = "Bad_Slug";
        fn meta() -> PanelPageMeta {
            PanelPageMeta::new("Bad").permission("x.view")
        }
        fn view() -> PageView {
            PageView::Template("{{ unclosed")
        }
    }
    struct Elsewhere;
    #[async_trait::async_trait]
    impl PanelSlot for Elsewhere {
        const NAME: &'static str = "elsewhere";
        fn placement() -> Placement {
            Placement::RecordAside("nope")
        }
        fn meta() -> SlotMeta {
            SlotMeta::new().permission("x.view")
        }
        fn view() -> PageView {
            PageView::Template("x")
        }
        async fn data(_: &Ctx, _: &rustcms_admin::Envelope) -> AppResult<Value> {
            Ok(Value::Null)
        }
    }
    let boot = |admin: fn() -> AdminProvider| {
        std::panic::catch_unwind(move || {
            let mut app = rustcms_core::AppBuilder::new(rustcms_testing::test_config("sqlite::memory:"));
            app.provider(AuthProvider::new());
            app.provider(admin());
        })
        .is_err()
    };
    assert!(boot(|| AdminProvider::new().page::<NoPermission>()));
    assert!(boot(|| AdminProvider::new().page::<BadScript>()));
    assert!(boot(|| AdminProvider::new().page::<BadTemplate>()));
    assert!(boot(|| AdminProvider::new().slot::<Elsewhere>()));
    assert!(boot(|| AdminProvider::new().page::<OverviewPage>().page::<OverviewPage>()));
    assert!(!boot(|| all_resources().page::<OverviewPage>()));
}

#[test]
fn template_strings_are_listed_for_translation() {
    let mut app = rustcms_core::AppBuilder::new(rustcms_testing::test_config("sqlite::memory:"));
    app.provider(AuthProvider::new());
    app.provider(all_resources().page::<OverviewPage>().page::<ReservedPage>());
    let panel = app.service::<rustcms_admin::AdminPanel>().unwrap();
    let missing = panel.missing_translations("vi", &[]);
    for label in ["Overview", "Reports", "Categories", "Reserved"] {
        assert!(missing.contains(&label.to_owned()), "{label}: {missing:?}");
    }
}
