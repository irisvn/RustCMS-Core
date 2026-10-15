use demo::bootstrap;
use demo::database::migrations::Migrator;
use rustcms_testing::{TestApp, TestDb, db_test};
use sea_orm::{ActiveModelTrait, EntityTrait, Set};

use demo::models::{category, post};

db_test!(deleting_a_category_clears_it_on_posts, migrate = Migrator, |db: TestDb| async move {
    let app = TestApp::new(bootstrap::build, db).await;
    let db = app.state().db();
    let news = category::ActiveModel { name: Set("News".into()), slug: Set("news".into()), ..Default::default() }
        .insert(db)
        .await
        .unwrap();
    let now = chrono::Utc::now();
    let post = post::ActiveModel {
        title: Set("Hello".into()),
        category_id: Set(Some(news.id)),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap();
    category::Entity::delete_by_id(news.id).exec(db).await.unwrap();
    let post = post::Entity::find_by_id(post.id).one(db).await.unwrap().unwrap();
    assert_eq!(post.category_id, None, "no dangling reference");
});

db_test!(migrations_roll_back_and_forward_keeping_ids, migrate = Migrator, |db: TestDb| async move {
    use sea_orm::ConnectionTrait;
    use sea_orm_migration::MigratorTrait;
    let insert = "INSERT INTO posts (created_at, updated_at) VALUES (CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)";
    let max_id = async || {
        let row = db
            .conn()
            .query_one_raw(sea_orm::Statement::from_string(
                db.conn().get_database_backend(),
                "SELECT MAX(id) AS n FROM posts",
            ))
            .await
            .unwrap()
            .unwrap();
        row.try_get::<Option<i64>>("", "n").unwrap()
    };
    // Ids 1 and 2, then 2 is deleted: 2 must never be issued again.
    db.conn().execute_unprepared(insert).await.unwrap();
    db.conn().execute_unprepared(insert).await.unwrap();
    db.conn().execute_unprepared("DELETE FROM posts WHERE id = 2").await.unwrap();
    // Back to before the categories migration (categories, deleted_at, media, color,
    // attributes, tags): its rebuild (SQLite, PostgreSQL) or in-place change (MySQL) is undone.
    Migrator::down(db.conn(), Some(6)).await.unwrap();
    assert!(!sea_orm_migration::SchemaManager::new(db.conn()).has_table("categories").await.unwrap());
    db.conn().execute_unprepared(insert).await.unwrap();
    assert_eq!(max_id().await, Some(3), "rollback keeps the id high-water mark");
    db.conn().execute_unprepared("DELETE FROM posts WHERE id = 3").await.unwrap();
    Migrator::up(db.conn(), None).await.unwrap();
    db.conn().execute_unprepared(insert).await.unwrap();
    assert_eq!(max_id().await, Some(4), "upgrade keeps the id high-water mark");
});

db_test!(seeding_completes_after_an_interrupted_run, migrate = Migrator, |db: TestDb| async move {
    use rustcms_testing::Factory;
    use sea_orm::{ActiveModelTrait, PaginatorTrait, Set};
    let app = TestApp::new(bootstrap::build, db).await;
    let state = app.state();
    // A previous run stopped after the admin and one category.
    rustcms_testing::UserFactory::new().email("admin@example.test").create(state.db()).await;
    category::ActiveModel { name: Set("News".into()), slug: Set("news".into()), ..Default::default() }
        .insert(state.db())
        .await
        .unwrap();
    crate::support::seed(state).await;
    crate::support::seed(state).await;
    assert_eq!(category::Entity::find().count(state.db()).await.unwrap(), 3);
    assert_eq!(post::Entity::find().count(state.db()).await.unwrap(), 30);
});

db_test!(posts_keep_key_value_attributes, migrate = Migrator, |db: TestDb| async move {
    use rustcms::axum::http::StatusCode;
    use sea_orm::{ColumnTrait, QueryFilter};
    use serde_json::json;
    let app = TestApp::new(bootstrap::build, db).await;
    crate::support::seed(app.state()).await;
    let admin = rustcms::models::user::Entity::find()
        .filter(rustcms::models::user::Column::Email.eq("admin@example.test"))
        .one(app.state().db())
        .await
        .unwrap()
        .unwrap();
    let panel = app.acting_as(&admin).await;
    let records = app.admin_api("resources/posts/records");

    // The form has the three tabs, the last one with the key/value input.
    let schema = panel.get(&app.admin_api("resources/posts/schema")).await.json().to_string();
    for needle in ["\"Content\"", "\"Publishing\"", "\"Attributes\"", "key_value"] {
        assert!(schema.contains(needle), "{needle} missing from the posts schema");
    }

    let attributes = json!({"reading_time": "5 min", "audience": "everyone"});
    let created = panel.post_json(&records, &json!({"title": "With attributes", "attributes": attributes})).await;
    created.assert_status(StatusCode::CREATED);
    let id = created.json()["data"]["key"].as_str().unwrap().to_owned();
    let stored = post::Entity::find_by_id(id.parse::<i64>().unwrap()).one(app.state().db()).await.unwrap().unwrap();
    assert_eq!(stored.attributes, Some(attributes));

    // Bounds hold on every write: values are text, at most 20 pairs of 200 characters.
    let too_many: serde_json::Map<_, _> = (0..21).map(|i| (format!("k{i}"), json!("v"))).collect();
    for bad in [json!({"n": 1}), json!(too_many), json!({"long": "x".repeat(201)}), json!(["a"])] {
        panel
            .post_json(&records, &json!({"title": "Bad", "attributes": bad}))
            .await
            .assert_validation_error("attributes");
    }
    // Seeded posts have attributes too.
    let seeded = post::Entity::find().one(app.state().db()).await.unwrap().unwrap();
    assert!(seeded.attributes.is_some());
});

db_test!(every_label_speaks_vietnamese, migrate = Migrator, |db: TestDb| async move {
    let app = TestApp::new(bootstrap::build, db).await;
    let state = app.state();
    let panel = state.service::<rustcms::admin::AdminPanel>().unwrap();
    let auth = state.service::<rustcms::auth::AuthState>().unwrap();
    let groups: Vec<String> = auth.permissions.all().into_values().flatten().collect();
    assert_eq!(panel.missing_translations("vi", &groups), Vec::<String>::new());
});

#[test]
fn migration_names_never_change() {
    use sea_orm_migration::MigratorTrait;
    // Recorded in `seaql_migrations` of every existing database: moving the
    // files (0.3 layout) must keep these names.
    let names: Vec<String> = Migrator::migrations().iter().map(|m| m.name().to_owned()).collect();
    assert!(
        names.ends_with(&[
            "m20261011_155203_create_posts_table".to_owned(),
            "m20261012_000001_create_categories_and_post_content".to_owned(),
            "m20261013_000001_add_deleted_at_to_posts".to_owned(),
            "m20261014_000001_add_media_to_posts".to_owned(),
            "m20261014_000002_add_color_to_categories".to_owned(),
            "m20261015_000001_add_attributes_to_posts".to_owned(),
            "m20261022_000001_create_tags".to_owned(),
        ]),
        "{names:?}"
    );
}
