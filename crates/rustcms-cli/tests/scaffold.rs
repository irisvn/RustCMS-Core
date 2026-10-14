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
    let blank = std::fs::read_dir(app.join("src/migrations"))
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
    let create = std::fs::read_dir(app.join("src/migrations"))
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
    let app_rs = std::fs::read_to_string(app.join("src/app.rs")).unwrap();
    assert!(app_rs.contains(".resource::<crate::admin::blog_post::BlogPostResource>()"), "{app_rs}");
    std::fs::write(
        app.join("tests/admin_resource.rs"),
        r#"use blog::{app, migrations::Migrator};
use rustcms::axum::http::StatusCode;
use rustcms_testing::{Factory, TestApp, TestDb, UserFactory, db_test};
use serde_json::json;

db_test!(generated_resource_is_served, migrate = Migrator, |db: TestDb| async move {
    let app = TestApp::new(app::build, db).await;
    let user = UserFactory::new().create(app.state().db()).await;
    app.make_super_admin(&user).await;
    let schema = app.acting_as(&user).await.get("/admin/api/resources/blog_posts/schema?operation=create").await;
    schema.assert_status(StatusCode::OK).assert_json("/form/components/0/name", json!("title"));
});

db_test!(generated_api_is_served, migrate = Migrator, |db: TestDb| async move {
    use sea_orm::{ActiveModelTrait, Set};
    let app = TestApp::new(app::build, db).await;
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

    rustcms(&app).arg("migrate").env("CARGO_TARGET_DIR", &target).assert().success();
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
