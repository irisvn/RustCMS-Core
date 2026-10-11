//! M0 go/no-go: the generic engine over real SeaORM entities, on SQLite and PostgreSQL.
//!
//! PostgreSQL runs when `RUSTCMS_TEST_PG` holds an admin URL (see `justfile`);
//! each test gets its own database.

mod support;

use rustcms_admin::{Data, FilterOp, ListQuery};
use rustcms_core::AppError;
use rustcms_testing::{TestDb, db_test};
use serde_json::{Value, json};
use support::{Fixture, PrincipalExt, admin, editor};

fn data(value: Value) -> Data {
    value.as_object().expect("object").clone()
}

fn field_errors(err: AppError) -> Vec<(String, Vec<String>)> {
    match err {
        AppError::Validation(v) => v.errors.into_iter().collect(),
        other => panic!("expected validation error, got {other:?}"),
    }
}

db_test!(db_generated_key_and_list_controls, |db: TestDb| async move {
    let fx = Fixture::new(db).await;
    let ctx = fx.ctx(admin());
    let categories = fx.resource("categories");
    for (name, slug) in [("News", "news"), ("Art", "art"), ("Sport", "sport")] {
        let created = categories.create(&ctx, data(json!({"name": name, "slug": slug}))).await.unwrap();
        assert!(created.key.parse::<i32>().is_ok());
        assert_eq!(created.title.as_deref(), Some(name));
    }

    let query = ListQuery { sort: Some("-name".into()), per_page: Some(10_000), ..Default::default() };
    let page = categories.list(&ctx, &query).await.unwrap();
    assert_eq!(page.meta.per_page, 100, "per_page is capped");
    let names: Vec<_> = page.data.iter().map(|e| e.data["name"].clone()).collect();
    assert_eq!(names, vec![json!("Sport"), json!("News"), json!("Art")]);

    let query = ListQuery { search: Some("Spo".into()), ..Default::default() };
    assert_eq!(categories.list(&ctx, &query).await.unwrap().meta.total, 1);

    let query = ListQuery { sort: Some("id".into()), ..Default::default() };
    assert_eq!(field_errors(categories.list(&ctx, &query).await.unwrap_err())[0].0, "sort");
});

db_test!(unique_constraint_maps_to_field_error, |db: TestDb| async move {
    let fx = Fixture::new(db).await;
    let ctx = fx.ctx(admin());
    let categories = fx.resource("categories");
    categories.create(&ctx, data(json!({"name": "News", "slug": "news"}))).await.unwrap();
    let err = categories.create(&ctx, data(json!({"name": "Other", "slug": "news"}))).await.unwrap_err();
    assert_eq!(field_errors(err), vec![("slug".to_owned(), vec!["has already been taken".to_owned()])]);
});

db_test!(app_generated_key_hooks_and_wire_format, |db: TestDb| async move {
    let fx = Fixture::new(db).await;
    let ctx = fx.ctx(editor("7"));
    let category = fx.seed_category("News").await;
    let posts = fx.resource("posts");

    let post = posts
        .create(
            &ctx,
            data(json!({
                "title": "Hello", "category_id": category.parse::<i32>().unwrap(), "price": "10.50",
                "published_at": "2026-10-11T21:30:00+07:00"
            })),
        )
        .await
        .unwrap();
    assert!(uuid::Uuid::parse_str(&post.key).is_ok(), "app-generated uuid key");
    assert_eq!(post.data["author_id"], json!("7"), "i64 set by hook, string on the wire");
    assert_eq!(post.data["price"], json!("10.50"));
    assert_eq!(post.data["published_at"], json!("2026-10-11T14:30:00Z"));
    assert!(!post.data.contains_key("secret"), "undeclared columns are never exposed");
    assert_eq!(post.labels["category_id"], "News");
    assert!(post.abilities["update"]);

    let err =
        posts.create(&ctx, data(json!({"title": "x", "author_id": "1", "id": "x", "secret": "s"}))).await.unwrap_err();
    let fields: Vec<_> = field_errors(err).into_iter().map(|(f, _)| f).collect();
    assert_eq!(fields, vec!["author_id", "id", "secret"], "guarded, key and undeclared columns are rejected");

    let err = posts.create(&ctx, data(json!({"title": "x", "price": 10.5}))).await.unwrap_err();
    assert_eq!(field_errors(err)[0].0, "price", "decimal must be a string");
});

db_test!(field_write_permissions, |db: TestDb| async move {
    let fx = Fixture::new(db).await;
    let ctx = fx.ctx(editor("7"));
    let posts = fx.resource("posts");
    let post = posts.create(&ctx, data(json!({"title": "Draft"}))).await.unwrap();

    let err = posts.update(&ctx, &post.key, data(json!({"published": true}))).await.unwrap_err();
    assert_eq!(field_errors(err)[0].0, "published");

    let publisher = fx.ctx(editor("7").with(&["posts.publish"]));
    let updated = posts.update(&publisher, &post.key, data(json!({"published": true}))).await.unwrap();
    assert_eq!(updated.data["published"], json!(true));
    assert_eq!(updated.data["title"], json!("Draft"), "PATCH keeps other fields");
});

db_test!(scope_hides_other_owners_records, |db: TestDb| async move {
    let fx = Fixture::new(db).await;
    let posts = fx.resource("posts");
    let mine = posts.create(&fx.ctx(editor("7")), data(json!({"title": "Mine"}))).await.unwrap();
    let theirs = posts.create(&fx.ctx(editor("8")), data(json!({"title": "Theirs"}))).await.unwrap();

    let ctx = fx.ctx(editor("7"));
    assert_eq!(posts.list(&ctx, &ListQuery::default()).await.unwrap().meta.total, 1);
    assert!(matches!(posts.find(&ctx, &theirs.key).await, Err(AppError::NotFound)));
    assert!(matches!(posts.update(&ctx, &theirs.key, Data::new()).await, Err(AppError::NotFound)));
    assert!(matches!(posts.delete(&ctx, &theirs.key).await, Err(AppError::NotFound)));
    posts.delete(&ctx, &mine.key).await.unwrap();

    let admin_ctx = fx.ctx(admin());
    assert_eq!(posts.list(&admin_ctx, &ListQuery::default()).await.unwrap().meta.total, 1);
});

db_test!(relationships_respect_target_scope, |db: TestDb| async move {
    let fx = Fixture::new(db).await;
    let hidden = fx.seed_category("Hidden budget").await;
    let ctx = fx.ctx(editor("7"));
    let posts = fx.resource("posts");

    let err = posts
        .create(&ctx, data(json!({"title": "x", "category_id": hidden.parse::<i32>().unwrap()})))
        .await
        .unwrap_err();
    assert_eq!(field_errors(err), vec![("category_id".to_owned(), vec!["is invalid".to_owned()])]);

    // A record linked by someone allowed: the editor sees the key, not the hidden title.
    let admin_ctx = fx.ctx(admin());
    let post = posts
        .create(&admin_ctx, data(json!({"title": "x", "category_id": hidden.parse::<i32>().unwrap()})))
        .await
        .unwrap();
    assert_eq!(post.labels["category_id"], "Hidden budget");
    let as_owner = fx.ctx(editor(post.data["author_id"].as_str().unwrap()));
    assert_eq!(posts.find(&as_owner, &post.key).await.unwrap().labels["category_id"], format!("#{hidden}"));
});

db_test!(bulk_action_reports_each_record, |db: TestDb| async move {
    let fx = Fixture::new(db).await;
    let posts = fx.resource("posts");
    let ctx = fx.ctx(editor("7").with(&["posts.publish"]));
    let open = posts.create(&ctx, data(json!({"title": "Open"}))).await.unwrap();
    let locked = posts.create(&ctx, data(json!({"title": "locked: legal"}))).await.unwrap();
    let foreign = posts.create(&fx.ctx(editor("8")), data(json!({"title": "Other"}))).await.unwrap();

    let keys = vec![open.key.clone(), locked.key.clone(), foreign.key.clone(), "not-a-uuid".into()];
    let outcome = posts.run_action(&ctx, "publish", &keys, Data::new()).await.unwrap();
    assert_eq!(outcome.processed, vec![open.key.clone()]);
    assert_eq!(outcome.denied, vec![locked.key.clone()]);
    let mut missing = vec![foreign.key.clone(), "not-a-uuid".to_owned()];
    missing.sort();
    assert_eq!(outcome.missing, missing);

    assert_eq!(posts.find(&ctx, &open.key).await.unwrap().data["published"], json!(true));
    assert_eq!(posts.find(&ctx, &locked.key).await.unwrap().data["published"], json!(false));

    let too_many: Vec<String> = (0..501).map(|i| i.to_string()).collect();
    assert_eq!(field_errors(posts.run_action(&ctx, "publish", &too_many, Data::new()).await.unwrap_err())[0].0, "keys");
});

db_test!(natural_key_and_transactional_hooks, |db: TestDb| async move {
    let fx = Fixture::new(db).await;
    let ctx = fx.ctx(admin());
    let tags = fx.resource("tags");
    let tag = tags.create(&ctx, data(json!({"slug": "rust", "label": "Rust"}))).await.unwrap();
    assert_eq!(tag.key, "rust");

    assert_eq!(field_errors(tags.create(&ctx, data(json!({"label": "No key"}))).await.unwrap_err())[0].0, "slug");
    assert_eq!(field_errors(tags.update(&ctx, "rust", data(json!({"slug": "go"}))).await.unwrap_err())[0].0, "slug");

    // after_save fails → the insert rolls back.
    assert!(tags.create(&ctx, data(json!({"slug": "boom", "label": "boom"}))).await.is_err());
    assert!(matches!(tags.find(&ctx, "boom").await, Err(AppError::NotFound)));
});

db_test!(filters_are_whitelisted_and_bounded, |db: TestDb| async move {
    let fx = Fixture::new(db).await;
    let ctx = fx.ctx(admin());
    let posts = fx.resource("posts");
    posts.create(&ctx, data(json!({"title": "a"}))).await.unwrap();

    let filter = |field: &str, op, value| rustcms_admin::Filter { field: field.into(), op, value };
    let query = ListQuery { filters: vec![filter("published", FilterOp::Eq, json!(false))], ..Default::default() };
    assert_eq!(posts.list(&ctx, &query).await.unwrap().meta.total, 1);

    let query = ListQuery { filters: vec![filter("title", FilterOp::Eq, json!("a"))], ..Default::default() };
    assert_eq!(field_errors(posts.list(&ctx, &query).await.unwrap_err())[0].0, "title", "not filterable");

    let many: Vec<_> = (0..101).collect();
    let query = ListQuery { filters: vec![filter("category_id", FilterOp::In, json!(many))], ..Default::default() };
    assert_eq!(field_errors(posts.list(&ctx, &query).await.unwrap_err())[0].0, "category_id", "in-list bounded");

    let query = ListQuery { filters: vec![filter("category_id", FilterOp::Eq, json!("1"))], ..Default::default() };
    assert_eq!(field_errors(posts.list(&ctx, &query).await.unwrap_err())[0].0, "category_id", "typed values");
});

db_test!(deny_by_default, |db: TestDb| async move {
    let fx = Fixture::new(db).await;
    let nobody = fx.ctx(editor("9").without_all());
    let posts = fx.resource("posts");
    assert!(matches!(posts.list(&nobody, &ListQuery::default()).await, Err(AppError::Forbidden)));
    assert!(matches!(posts.create(&nobody, data(json!({"title": "x"}))).await, Err(AppError::Forbidden)));
    assert!(matches!(fx.resource("tags").list(&nobody, &ListQuery::default()).await, Err(AppError::Forbidden)));
});
