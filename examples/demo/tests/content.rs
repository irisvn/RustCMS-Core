use demo::{app, migrations::Migrator};
use rustcms_testing::{TestApp, TestDb, db_test};
use sea_orm::{ActiveModelTrait, EntityTrait, Set};

use demo::models::{category, post};

db_test!(deleting_a_category_clears_it_on_posts, migrate = Migrator, |db: TestDb| async move {
    let app = TestApp::new(app::build, db).await;
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
    // Back to before the categories migration (categories, deleted_at, media, color):
    // its rebuild (SQLite, PostgreSQL) or in-place change (MySQL) is undone.
    Migrator::down(db.conn(), Some(4)).await.unwrap();
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
    let app = TestApp::new(app::build, db).await;
    let state = app.state();
    // A previous run stopped after the admin and one category.
    rustcms_testing::UserFactory::new().email("admin@example.test").create(state.db()).await;
    category::ActiveModel { name: Set("News".into()), slug: Set("news".into()), ..Default::default() }
        .insert(state.db())
        .await
        .unwrap();
    demo::seed::seed(state).await.unwrap();
    demo::seed::seed(state).await.unwrap();
    assert_eq!(category::Entity::find().count(state.db()).await.unwrap(), 3);
    assert_eq!(post::Entity::find().count(state.db()).await.unwrap(), 30);
});
