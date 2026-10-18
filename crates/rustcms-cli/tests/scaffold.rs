//! `rustcms new` + generators produce an app that compiles and passes its tests.
//!
//! Slow (compiles a fresh app): `cargo test -p rustcms-cli -- --ignored` (`just test-cli`).

use std::path::Path;
use std::process::Command;

use assert_cmd::prelude::*;

fn rustcms(dir: &Path) -> Command {
    let mut cmd = Command::cargo_bin("rustcms").unwrap();
    cmd.current_dir(dir);
    cmd
}

#[test]
#[ignore = "compiles a generated application"]
fn generated_app_builds_and_passes_its_tests() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    rustcms(tmp.path()).args(["new", "blog", "--rustcms-path"]).arg(repo).assert().success();
    let app = tmp.path().join("blog");

    rustcms(&app).args(["make:model", "BlogPost", "-m"]).assert().success();
    rustcms(&app.join("src")).args(["make:migration", "add_slug_to_blog_posts"]).assert().success();
    // The blank migration is a `todo!()` for the developer; replace it so the app migrates.
    let blank = std::fs::read_dir(app.join("src/database/migrations"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.to_string_lossy().ends_with("_add_slug_to_blog_posts.rs"))
        .unwrap();
    let contents = std::fs::read_to_string(&blank)
        .unwrap()
        .replace("todo!(\"add_slug_to_blog_posts: write the migration\")", "Ok(())")
        .replace("todo!(\"add_slug_to_blog_posts: undo the migration\")", "Ok(())");
    std::fs::write(&blank, contents).unwrap();

    // Columns for the generated resource (the schema endpoint does not read the table).
    let model = app.join("src/models/blog_post.rs");
    let source = std::fs::read_to_string(&model).unwrap().replace(
        "    pub id: i64,\n",
        "    pub id: i64,\n    pub title: String,\n    pub body: Option<String>,\n    pub published: bool,\n",
    );
    std::fs::write(&model, source).unwrap();
    // The table gets the same columns, so the generated API can read rows.
    let create = std::fs::read_dir(app.join("src/database/migrations"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.to_string_lossy().ends_with("_create_blog_posts_table.rs"))
        .unwrap();
    let migration = std::fs::read_to_string(&create).unwrap().replacen(
        ".col(ColumnDef::new(BlogPosts::CreatedAt)",
        ".col(ColumnDef::new(Alias::new(\"title\")).string().not_null())\n                .col(ColumnDef::new(Alias::new(\"body\")).text().null())\n                .col(ColumnDef::new(Alias::new(\"published\")).boolean().not_null())\n                .col(ColumnDef::new(BlogPosts::CreatedAt)",
        1,
    );
    assert!(migration.contains("Alias::new(\"title\")"), "{migration}");
    std::fs::write(&create, migration).unwrap();
    rustcms(&app).args(["make:resource", "BlogPost", "--generate"]).assert().success();
    rustcms(&app).args(["make:api", "BlogPost"]).assert().success();
    // An unusual model: the generated API must still compile (no table needed).
    rustcms(&app).args(["make:model", "StockItem"]).assert().success();
    std::fs::write(
        app.join("src/models/stock_item.rs"),
        r##"use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(
    table_name = "stock-items"
)]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false, enum_name = "Code")]
    pub item_code: u32,
    pub r#type: String,
    #[sea_orm(column_type = "Decimal(Some((10, 2)))")]
    pub price: Decimal,
    pub seen_at: Option<DateTimeWithTimeZone>,
    pub data: Option<Json>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
"##,
    )
    .unwrap();
    rustcms(&app).args(["make:api", "StockItem"]).assert().success();
    rustcms(&app).args(["make:policy", "BlogPostPolicy"]).assert().success();
    let provider = std::fs::read_to_string(app.join("src/providers/admin_panel_provider.rs")).unwrap();
    assert!(
        provider.contains(".resource::<crate::admin::resources::blog_post_resource::BlogPostResource>()"),
        "{provider}"
    );
    // Every other generator, as a new app would use them.
    for args in [
        &["make:service", "PublishingService"][..],
        &["make:job", "NotifyAuthor"],
        &["make:seeder", "BlogPostSeeder"],
        &["make:factory", "BlogPost"],
        &["make:settings", "BlogSettings"],
        &["make:command", "blog:report"],
        &["make:test", "AdminResource"],
        &["make:page", "ReportPage"],
        &["make:page", "ChartPage", "--script"],
        &["make:page", "StatsPage", "--react"],
    ] {
        rustcms(&app).args(args).assert().success();
    }
    // The React page builds with the SDK the CLI wrote (needs npm; CI installs Node).
    if Command::new("npm").arg("--version").output().is_ok_and(|o| o.status.success()) {
        let admin = app.join("resources/admin");
        let npm = |args: &[&str]| {
            let status = Command::new("npm").current_dir(&admin).args(args).status().unwrap();
            assert!(status.success(), "npm {args:?} failed");
        };
        npm(&["install", "--no-audit", "--no-fund"]);
        std::fs::remove_dir_all(admin.join("node_modules")).unwrap();
        npm(&["ci", "--no-audit", "--no-fund"]);
        npm(&["run", "typecheck"]);
        npm(&["run", "build"]);
        std::fs::remove_dir_all(admin.join("node_modules")).unwrap();
        let built = std::fs::read_to_string(admin.join("js/blog/stats.js")).unwrap();
        assert!(built.contains("\"blog/stats\""), "built page:\n{built}");
    }
    // A generated test module, replaced by these checks.
    std::fs::write(
        app.join("tests/feature/admin_resource.rs"),
        r#"use blog::bootstrap;
use blog::database::migrations::Migrator;
use rustcms::axum::http::StatusCode;
use rustcms_testing::{Factory, TestApp, TestDb, UserFactory, db_test};
use serde_json::json;

db_test!(generated_resource_is_served, migrate = Migrator, |db: TestDb| async move {
    let app = TestApp::new(bootstrap::build, db).await;
    let user = UserFactory::new().create(app.state().db()).await;
    app.make_super_admin(&user).await;
    let schema = app.acting_as(&user).await.get("/admin/api/resources/blog_posts/schema?operation=create").await;
    schema.assert_status(StatusCode::OK).assert_json("/form/components/0/name", json!("title"));
});

db_test!(generated_api_is_served, migrate = Migrator, |db: TestDb| async move {
    use sea_orm::{ActiveModelTrait, Set};
    let app = TestApp::new(bootstrap::build, db).await;
    app.get("/api/v1/blog_posts").await.assert_status(StatusCode::UNAUTHORIZED);
    let user = UserFactory::new().create(app.state().db()).await;
    app.make_super_admin(&user).await;
    let token = app.with_token(&user, &["*"]).await;
    let empty = token.get("/api/v1/blog_posts").await;
    empty.assert_status(StatusCode::OK).assert_json("/meta/total", json!(0)).assert_json("/meta/lastPage", json!(1));
    token.get("/api/v1/blog_posts/1").await.assert_status(StatusCode::NOT_FOUND);
    let now = chrono_now();
    let post = blog::models::blog_post::ActiveModel {
        title: Set("Hello".into()),
        body: Set(None),
        published: Set(true),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(app.state().db())
    .await
    .unwrap();
    let path = format!("/api/v1/blog_posts/{}", post.id);
    token.get(&path).await.assert_status(StatusCode::OK).assert_json("/title", json!("Hello")).assert_json("/id", json!(post.id.to_string()));
    token.get("/api/v1/blog_posts?page=0").await.assert_validation_error("page");
    // Past the last page: no items, same totals.
    let past = token.get("/api/v1/blog_posts?page=5").await;
    past.assert_status(StatusCode::OK).assert_json("/data", json!([])).assert_json("/meta/total", json!(1));
    past.assert_json("/meta/lastPage", json!(1)).assert_json("/meta/page", json!(5));
});

db_test!(generated_settings_and_seeders_run, migrate = Migrator, |db: TestDb| async move {
    use rustcms::database::Seeder;
    let app = TestApp::new(bootstrap::build, db).await;
    blog::database::seeders::DatabaseSeeder.run(app.state()).await.unwrap();
    let user = UserFactory::new().create(app.state().db()).await;
    app.make_super_admin(&user).await;
    let page = app.acting_as(&user).await.get("/admin/api/settings/blog").await;
    page.assert_status(StatusCode::OK);
    // Generated pages: a template, and a script.
    let report = app.acting_as(&user).await.get("/admin/api/pages/report").await;
    report.assert_status(StatusCode::OK).assert_json("/content/kind", json!("html"));
    assert!(report.json()["content"]["html"].as_str().unwrap().contains("<h2 class=\"rc-title\">Report</h2>"));
    let chart = app.acting_as(&user).await.get("/admin/api/pages/chart").await;
    chart.assert_status(StatusCode::OK).assert_json("/content/script", json!("blog/chart"));
    let stats = app.acting_as(&user).await.get("/admin/api/pages/stats/sub/path").await;
    stats.assert_status(StatusCode::OK).assert_json("/content/script", json!("blog/stats"));
});

fn chrono_now() -> sea_orm::prelude::DateTimeUtc {
    sea_orm::prelude::DateTimeUtc::from(std::time::SystemTime::now())
}
"#,
    )
    .unwrap();

    let env = std::fs::read_to_string(app.join(".env")).unwrap();
    assert!(env.lines().any(|l| l.starts_with("APP_KEY=base64:")), "fresh key in .env");

    let target = repo.join("target/scaffold-test");
    let status = Command::new(env!("CARGO"))
        .current_dir(&app)
        .env("CARGO_TARGET_DIR", &target)
        .args(["test", "--quiet"])
        .status()
        .unwrap();
    assert!(status.success(), "generated app tests failed");
    // The feature suite is one binary that holds every generated module.
    let listed = Command::new(env!("CARGO"))
        .current_dir(&app)
        .env("CARGO_TARGET_DIR", &target)
        .args(["test", "--quiet", "--test", "feature", "--", "--list"])
        .output()
        .unwrap();
    let listed = String::from_utf8_lossy(&listed.stdout);
    for test in [
        "health::health_and_ping",
        "admin_resource::generated_api_is_served",
        "admin_resource::generated_settings_and_seeders_run",
    ] {
        assert!(listed.contains(test), "{test} not in the feature suite:\n{listed}");
    }
    rustcms(&app).arg("migrate").env("CARGO_TARGET_DIR", &target).assert().success();
    rustcms(&app).arg("db:seed").env("CARGO_TARGET_DIR", &target).assert().success();
    rustcms(&app).args(["db:seed", "--class", "blog_post"]).env("CARGO_TARGET_DIR", &target).assert().success();
    rustcms(&app)
        .args(["db:seed", "--class", "nope"])
        .env("CARGO_TARGET_DIR", &target)
        .assert()
        .failure()
        .stderr(predicates::str::contains("registered: database, blog_post"));
    rustcms(&app).arg("blog:report").env("CARGO_TARGET_DIR", &target).assert().success();
    // Production needs --force (checked before anything runs); a fresh start seeds too.
    rustcms(&app)
        .arg("db:seed")
        .env("CARGO_TARGET_DIR", &target)
        .env("APP_ENV", "production")
        .assert()
        .failure()
        .stderr(predicates::str::contains("refusing to seed in production"));
    rustcms(&app)
        .args(["migrate:fresh", "--seed"])
        .env("CARGO_TARGET_DIR", &target)
        .assert()
        .success()
        .stdout(predicates::str::contains("Database seeded."));

    rustcms(&app)
        .arg("migrate:status")
        .env("CARGO_TARGET_DIR", &target)
        .assert()
        .success()
        .stdout(predicates::str::contains("create_blog_posts_table"));
}

#[test]
fn outside_a_project_app_commands_fail_with_a_hint() {
    let tmp = tempfile::tempdir().unwrap();
    rustcms(tmp.path()).arg("migrate").assert().failure().stderr(predicates::str::contains("rustcms new"));
}
