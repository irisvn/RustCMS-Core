//! M0 go/no-go: the generic engine over real SeaORM entities, on SQLite and PostgreSQL.
//!
//! PostgreSQL runs when `RUSTCMS_TEST_PG` holds an admin URL (see `justfile`);
//! each test gets its own database.

mod support;

use std::sync::Arc;

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

db_test!(limits_apply_to_values_written_by_hooks, |db: TestDb| async move {
    let fx = Fixture::new(db).await;
    let ctx = fx.ctx(editor("7"));
    let posts = fx.resource("posts");
    let err = posts.create(&ctx, data(json!({"title": "expand"}))).await.unwrap_err();
    assert_eq!(field_errors(err)[0].0, "title");
    assert_eq!(posts.list(&ctx, &ListQuery::default()).await.unwrap().meta.total, 0, "nothing written");
});

db_test!(search_matches_non_ascii_and_survives_joined_scopes, |db: TestDb| async move {
    let fx = Fixture::new(db).await;
    let ctx = fx.ctx(admin());
    let categories = fx.resource("categories");
    for (name, slug) in [("Đức Anh", "duc"), ("Other", "other")] {
        categories.create(&ctx, data(json!({"name": name, "slug": slug}))).await.unwrap();
    }
    let search = |s: &str| ListQuery { search: Some(s.into()), ..Default::default() };
    assert_eq!(
        categories.list(&ctx, &search("Đức")).await.unwrap().meta.total,
        1,
        "exact non-ASCII text matches itself"
    );
    assert_eq!(
        categories.list(&ctx, &search("đức anh")).await.unwrap().meta.total,
        u64::from(fx.db.backend() != rustcms_testing::Backend::Sqlite),
        "lower-case non-ASCII matches on PostgreSQL and MySQL (Unicode folding); SQLite folds ASCII only"
    );
    assert_eq!(
        categories.list(&ctx, &search("OTHER")).await.unwrap().meta.total,
        1,
        "ASCII case-insensitive everywhere"
    );
    assert_eq!(categories.lookup(&ctx, Some("Đức"), &[]).await.unwrap().len(), 1, "options search too");

    // A scope joining a table with the same column name: search stays unambiguous.
    let tags = fx.resource("tags");
    tags.create(&ctx, data(json!({"slug": "duc", "label": "Rust"}))).await.unwrap();
    let joined = fx.ctx(editor("7").with(&["tags.joined"]));
    assert_eq!(tags.list(&joined, &search("duc")).await.unwrap().meta.total, 1);
});

db_test!(options_skip_policy_denied_candidates, |db: TestDb| async move {
    let fx = Fixture::new(db).await;
    let admin_ctx = fx.ctx(admin());
    let categories = fx.resource("categories");
    // 55 denied candidates sort before the one the editor may see.
    for i in 0..55 {
        categories
            .create(&admin_ctx, data(json!({"name": format!("A{i:02}"), "slug": format!("secret-{i}")})))
            .await
            .unwrap();
    }
    categories.create(&admin_ctx, data(json!({"name": "B visible", "slug": "visible"}))).await.unwrap();
    let editor = fx.ctx(editor("7"));
    let titles: Vec<String> =
        categories.lookup(&editor, None, &[]).await.unwrap().into_iter().map(|(_, t)| t).collect();
    assert_eq!(titles, ["B visible"]);
    assert_eq!(categories.lookup(&admin_ctx, None, &[]).await.unwrap().len(), 50, "still capped at 50");
});

struct ReservedActionName;

impl rustcms_admin::Resource for ReservedActionName {
    type Entity = support::tag::Entity;
    const SLUG: &'static str = "reserved";

    fn meta() -> rustcms_admin::ResourceMeta {
        rustcms_admin::ResourceMeta::new("Reserved").key(rustcms_admin::KeyStrategy::Natural)
    }

    fn fields() -> Vec<rustcms_admin::Field> {
        vec![rustcms_admin::Field::make("slug"), rustcms_admin::Field::make("label")]
    }

    fn actions() -> Vec<rustcms_admin::Action<support::tag::Entity>> {
        vec![
            rustcms_admin::Action::<support::tag::Entity>::new("view", |_, _, _, _| Box::pin(async { Ok(()) }))
                .ability("update"),
        ]
    }
}

#[test]
#[should_panic(expected = "reserved for CRUD abilities")]
fn action_names_cannot_shadow_crud_abilities() {
    let _ = rustcms_admin::Registry::new().register::<ReservedActionName>();
}

db_test!(options_report_an_exhausted_scan_budget, |db: TestDb| async move {
    use sea_orm::EntityTrait;
    let fx = Fixture::new(db).await;
    let rows: Vec<_> = (0..1001)
        .map(|i| support::category::ActiveModel {
            name: sea_orm::Set(format!("A{i:04}")),
            slug: sea_orm::Set(format!("secret-{i}")),
            ..Default::default()
        })
        .collect();
    for chunk in rows.chunks(200) {
        support::category::Entity::insert_many(chunk.to_vec()).exec(fx.db.conn()).await.unwrap();
    }
    let admin_ctx = fx.ctx(admin());
    fx.resource("categories").create(&admin_ctx, data(json!({"name": "B visible", "slug": "visible"}))).await.unwrap();
    let err = fx.resource("categories").lookup(&fx.ctx(editor("7")), None, &[]).await.unwrap_err();
    assert_eq!(field_errors(err)[0].0, "search", "an incomplete scan is an error, not an empty list");
});

/// Over `tags`, sortable by label; its policy edits the sort order mid-scan
/// (once per principal) to simulate a concurrent writer.
struct Shuffle;

static SHUFFLED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

struct ShufflePolicy;

#[async_trait::async_trait]
impl rustcms_admin::Policy for ShufflePolicy {
    async fn allows(&self, ctx: &rustcms_admin::Ctx, ability: &str, record: Option<&Data>) -> bool {
        use sea_orm::ConnectionTrait;
        let first = record.is_some_and(|r| r["slug"] == json!("t000"));
        let fire = ability == "view" && first && {
            let mut done = SHUFFLED.lock().unwrap();
            !done.contains(&ctx.principal.id) && {
                done.push(ctx.principal.id.clone());
                true
            }
        };
        if fire {
            ctx.db.execute_unprepared("UPDATE tags SET label = 'zzz' WHERE slug = 't001'").await.unwrap();
        }
        true
    }
}

impl rustcms_admin::Resource for Shuffle {
    type Entity = support::tag::Entity;
    const SLUG: &'static str = "shuffle";

    fn meta() -> rustcms_admin::ResourceMeta {
        rustcms_admin::ResourceMeta::new("Shuffle").key(rustcms_admin::KeyStrategy::Natural)
    }

    fn fields() -> Vec<rustcms_admin::Field> {
        vec![rustcms_admin::Field::make("slug"), rustcms_admin::Field::make("label").sortable()]
    }

    fn policy() -> Arc<dyn rustcms_admin::Policy> {
        Arc::new(ShufflePolicy)
    }
}

db_test!(record_checked_lists_read_one_snapshot, |db: TestDb| async move {
    use sea_orm::EntityTrait;
    let fx = Fixture::new(db).await;
    let rows: Vec<_> = (0..600)
        .map(|i| support::tag::ActiveModel {
            slug: sea_orm::Set(format!("t{i:03}")),
            label: sea_orm::Set(format!("l{i:03}")),
        })
        .collect();
    for chunk in rows.chunks(200) {
        support::tag::Entity::insert_many(chunk.to_vec()).exec(fx.db.conn()).await.unwrap();
    }
    let registry = rustcms_admin::Registry::new().register::<Shuffle>().finish();
    let principal =
        rustcms_core::Principal { id: uuid::Uuid::new_v4().to_string(), super_admin: true, ..Default::default() };
    let ctx = rustcms_admin::Ctx::new(fx.db.conn().clone(), principal, registry.clone());
    let query = ListQuery { sort: Some("label".into()), per_page: Some(100), page: Some(6), ..Default::default() };
    let page = registry.get("shuffle").unwrap().list(&ctx, &query).await.unwrap();
    let keys: Vec<String> = page.data.iter().map(|e| e.key.clone()).collect();
    let expected: Vec<String> = (500..600).map(|i| format!("t{i:03}")).collect();
    assert_eq!(keys, expected, "no duplicate or skipped rows when sorting changes mid-scan");
    assert_eq!(page.meta.total, 600);
});

/// Categories titled by their integer id.
struct Numbered;

impl rustcms_admin::Resource for Numbered {
    type Entity = support::category::Entity;
    const SLUG: &'static str = "numbered";

    fn meta() -> rustcms_admin::ResourceMeta {
        rustcms_admin::ResourceMeta::new("Numbered").title("id")
    }

    fn fields() -> Vec<rustcms_admin::Field> {
        vec![rustcms_admin::Field::make("id"), rustcms_admin::Field::make("name"), rustcms_admin::Field::make("slug")]
    }
}

db_test!(options_search_non_text_titles, |db: TestDb| async move {
    let fx = Fixture::new(db).await;
    let admin_ctx = fx.ctx(admin());
    for i in 0..12 {
        fx.resource("categories")
            .create(&admin_ctx, data(json!({"name": format!("c{i}"), "slug": format!("c{i}")})))
            .await
            .unwrap();
    }
    let registry = rustcms_admin::Registry::new().register::<Numbered>().finish();
    let ctx = rustcms_admin::Ctx::new(fx.db.conn().clone(), admin(), registry.clone());
    let found = registry.get("numbered").unwrap().lookup(&ctx, Some("1"), &[]).await.unwrap();
    let titles: Vec<String> = found.into_iter().map(|(_, t)| t).collect();
    assert!(
        titles.contains(&"1".to_owned()) && titles.contains(&"10".to_owned()) && titles.contains(&"11".to_owned()),
        "{titles:?}"
    );
});

db_test!(record_checked_lists_work_with_a_one_connection_pool, |db: TestDb| async move {
    let fx = Fixture::new(db).await;
    let category = fx.seed_category("News").await.parse::<i32>().unwrap();
    let posts = fx.resource("posts");
    let writer = fx.ctx(editor("7"));
    for i in 0..3 {
        posts.create(&writer, data(json!({"title": format!("p{i}"), "category_id": category}))).await.unwrap();
    }
    // PostPolicy is record-dependent: the list scans in a snapshot, then loads category labels.
    let config =
        rustcms_core::config::DatabaseConfig { url: fx.db.url().to_owned(), max_connections: 1, log_statements: false };
    let single = rustcms_core::db::connect(&config).await.unwrap();
    let ctx = rustcms_admin::Ctx { db: single, ..fx.ctx(editor("7")) };
    let page = tokio::time::timeout(std::time::Duration::from_secs(10), posts.list(&ctx, &ListQuery::default()))
        .await
        .expect("no pool deadlock")
        .unwrap();
    assert_eq!(page.meta.total, 3);
    assert!(page.data.iter().all(|e| e.labels["category_id"] == "News"));
    let options = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        fx.resource("categories").lookup(&ctx, Some("New"), &[]),
    )
    .await
    .expect("no pool deadlock")
    .unwrap();
    assert_eq!(options.len(), 1);
});

/// Tags whose policy queries the database for every record.
struct Queried;

struct QueryingPolicy;

#[async_trait::async_trait]
impl rustcms_admin::Policy for QueryingPolicy {
    async fn allows(&self, ctx: &rustcms_admin::Ctx, _ability: &str, _record: Option<&Data>) -> bool {
        use sea_orm::ConnectionTrait;
        ctx.conn().execute_unprepared("SELECT 1").await.is_ok()
    }
}

impl rustcms_admin::Resource for Queried {
    type Entity = support::tag::Entity;
    const SLUG: &'static str = "queried";

    fn meta() -> rustcms_admin::ResourceMeta {
        rustcms_admin::ResourceMeta::new("Queried").title("label").key(rustcms_admin::KeyStrategy::Natural)
    }

    fn fields() -> Vec<rustcms_admin::Field> {
        vec![rustcms_admin::Field::make("slug"), rustcms_admin::Field::make("label").searchable()]
    }

    fn policy() -> Arc<dyn rustcms_admin::Policy> {
        Arc::new(QueryingPolicy)
    }
}

db_test!(database_backed_policies_work_with_a_one_connection_pool, |db: TestDb| async move {
    let fx = Fixture::new(db).await;
    fx.resource("tags").create(&fx.ctx(admin()), data(json!({"slug": "rust", "label": "Rust"}))).await.unwrap();
    let config =
        rustcms_core::config::DatabaseConfig { url: fx.db.url().to_owned(), max_connections: 1, log_statements: false };
    let single = rustcms_core::db::connect(&config).await.unwrap();
    let registry = rustcms_admin::Registry::new().register::<Queried>().finish();
    let ctx = rustcms_admin::Ctx::new(single, admin(), registry.clone());
    let queried = registry.get("queried").unwrap();
    let limit = std::time::Duration::from_secs(10);
    let page = tokio::time::timeout(limit, queried.list(&ctx, &ListQuery::default()))
        .await
        .expect("list: no pool deadlock")
        .unwrap();
    assert_eq!(page.meta.total, 1);
    let options = tokio::time::timeout(limit, queried.lookup(&ctx, Some("ru"), &[]))
        .await
        .expect("lookup: no pool deadlock")
        .unwrap();
    assert_eq!(options.len(), 1);
});

db_test!(record_checked_lists_hold_only_keys_and_a_chunk, |db: TestDb| async move {
    use sea_orm::EntityTrait;
    let fx = Fixture::new(db).await;
    // 300 large rows; the list keeps keys for all of them but loads 100 rows at a time.
    let big = "x".repeat(60 * 1024);
    let rows: Vec<_> = (0..300)
        .map(|i| support::tag::ActiveModel {
            slug: sea_orm::Set(format!("t{i:03}")),
            label: sea_orm::Set(format!("{i:03}{big}")),
        })
        .collect();
    for chunk in rows.chunks(50) {
        support::tag::Entity::insert_many(chunk.to_vec()).exec(fx.db.conn()).await.unwrap();
    }
    let registry = rustcms_admin::Registry::new().register::<Queried>().finish();
    let ctx = rustcms_admin::Ctx::new(fx.db.conn().clone(), admin(), registry.clone());
    let query = ListQuery { per_page: Some(1), page: Some(250), ..Default::default() };
    let page = registry.get("queried").unwrap().list(&ctx, &query).await.unwrap();
    assert_eq!(page.meta.total, 300);
    assert_eq!(page.data.len(), 1);
    assert_eq!(page.data[0].key, "t249");
});

/// Tags joined to categories with DISTINCT, record-checked (database-backed policy).
struct DistinctJoined;

impl rustcms_admin::Resource for DistinctJoined {
    type Entity = support::tag::Entity;
    const SLUG: &'static str = "distinct";

    fn meta() -> rustcms_admin::ResourceMeta {
        rustcms_admin::ResourceMeta::new("Distinct").title("label").key(rustcms_admin::KeyStrategy::Natural)
    }

    fn fields() -> Vec<rustcms_admin::Field> {
        vec![rustcms_admin::Field::make("slug"), rustcms_admin::Field::make("label").sortable().searchable()]
    }

    fn policy() -> Arc<dyn rustcms_admin::Policy> {
        Arc::new(QueryingPolicy)
    }

    fn scope(
        query: sea_orm::Select<support::tag::Entity>,
        _ctx: &rustcms_admin::Ctx,
    ) -> sea_orm::Select<support::tag::Entity> {
        use sea_orm::QuerySelect;
        let relation: sea_orm::RelationDef =
            <support::tag::Entity as sea_orm::EntityTrait>::belongs_to(support::category::Entity)
                .from(support::tag::Column::Slug)
                .to(support::category::Column::Slug)
                .into();
        query.join(sea_orm::JoinType::LeftJoin, relation).distinct()
    }
}

db_test!(distinct_joined_scopes_list_and_look_up, |db: TestDb| async move {
    let fx = Fixture::new(db).await;
    let admin_ctx = fx.ctx(admin());
    for (slug, label) in [("b", "Beta"), ("a", "Alpha")] {
        fx.resource("tags").create(&admin_ctx, data(json!({"slug": slug, "label": label}))).await.unwrap();
    }
    let registry = rustcms_admin::Registry::new().register::<DistinctJoined>().finish();
    let ctx = rustcms_admin::Ctx::new(fx.db.conn().clone(), admin(), registry.clone());
    let resource = registry.get("distinct").unwrap();
    let query = ListQuery { sort: Some("-label".into()), ..Default::default() };
    let keys: Vec<String> = resource.list(&ctx, &query).await.unwrap().data.into_iter().map(|e| e.key).collect();
    assert_eq!(keys, ["b", "a"]);
    let options: Vec<String> =
        resource.lookup(&ctx, Some("a"), &[]).await.unwrap().into_iter().map(|(_, t)| t).collect();
    assert_eq!(options, ["Alpha", "Beta"]);
});

/// Tags whose policy, when checking `t000`, renames `t120` (as a concurrent
/// writer would between the key scan and the second chunk). With
/// `tags.hide_first`, the first 100 tags are denied.
struct Mutating;

static MUTATED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

struct MutatingPolicy;

#[async_trait::async_trait]
impl rustcms_admin::Policy for MutatingPolicy {
    async fn allows(&self, ctx: &rustcms_admin::Ctx, _ability: &str, record: Option<&Data>) -> bool {
        use sea_orm::ConnectionTrait;
        let slug = record.and_then(|r| r["slug"].as_str()).unwrap_or_default().to_owned();
        let fire = slug == "t000" && {
            let mut done = MUTATED.lock().unwrap();
            !done.contains(&ctx.principal.id) && {
                done.push(ctx.principal.id.clone());
                true
            }
        };
        if fire {
            ctx.db.execute_unprepared("UPDATE tags SET label = 'gone' WHERE slug = 't120'").await.unwrap();
        }
        let hide_first = ctx.principal.permissions.contains("tags.hide_first");
        !(record.is_some() && hide_first && slug.as_str() < "t100")
    }
}

impl rustcms_admin::Resource for Mutating {
    type Entity = support::tag::Entity;
    const SLUG: &'static str = "mutating";

    fn meta() -> rustcms_admin::ResourceMeta {
        rustcms_admin::ResourceMeta::new("Mutating").title("label").key(rustcms_admin::KeyStrategy::Natural)
    }

    fn fields() -> Vec<rustcms_admin::Field> {
        vec![rustcms_admin::Field::make("slug").sortable(), rustcms_admin::Field::make("label").searchable()]
    }

    fn policy() -> Arc<dyn rustcms_admin::Policy> {
        Arc::new(MutatingPolicy)
    }
}

async fn seed_keep_tags(fx: &Fixture) {
    use sea_orm::EntityTrait;
    let rows: Vec<_> = (0..150)
        .map(|i| support::tag::ActiveModel {
            slug: sea_orm::Set(format!("t{i:03}")),
            label: sea_orm::Set(format!("keep-{i:03}")),
        })
        .collect();
    support::tag::Entity::insert_many(rows).exec(fx.db.conn()).await.unwrap();
}

db_test!(reloaded_candidates_must_still_match_the_query, |db: TestDb| async move {
    let fx = Fixture::new(db).await;
    seed_keep_tags(&fx).await;
    let registry = rustcms_admin::Registry::new().register::<Mutating>().finish();
    let principal =
        || rustcms_core::Principal { id: uuid::Uuid::new_v4().to_string(), super_admin: true, ..Default::default() };
    let resource = registry.get("mutating").unwrap();

    // List: t120 stops matching "keep" while the first chunk is checked.
    let ctx = rustcms_admin::Ctx::new(fx.db.conn().clone(), principal(), registry.clone());
    let query = ListQuery {
        search: Some("keep".into()),
        sort: Some("slug".into()),
        per_page: Some(100),
        page: Some(2),
        ..Default::default()
    };
    let page = resource.list(&ctx, &query).await.unwrap();
    assert!(page.data.iter().all(|e| e.key != "t120"), "non-matching record returned");
    assert_eq!(page.meta.total, 149, "non-matching record counted");

    // Options: the first 100 are denied, so the lookup reaches the renamed record.
    sea_orm::ConnectionTrait::execute_unprepared(
        fx.db.conn(),
        "UPDATE tags SET label = 'keep-120' WHERE slug = 't120'",
    )
    .await
    .unwrap();
    let mut hidden = principal();
    hidden.super_admin = false;
    hidden.permissions = ["mutating.view_any".to_owned(), "tags.hide_first".to_owned()].into();
    let ctx = rustcms_admin::Ctx::new(fx.db.conn().clone(), hidden, registry.clone());
    let titles: Vec<String> =
        resource.lookup(&ctx, Some("keep"), &[]).await.unwrap().into_iter().map(|(_, t)| t).collect();
    assert!(!titles.iter().any(|t| t == "gone"), "renamed option no longer matches the search");
    assert!(!titles.is_empty(), "the lookup reached the second chunk");
});

/// Posts titled by their UUID key.
struct UuidTitled;

impl rustcms_admin::Resource for UuidTitled {
    type Entity = support::post::Entity;
    const SLUG: &'static str = "uuid_titled";

    fn meta() -> rustcms_admin::ResourceMeta {
        rustcms_admin::ResourceMeta::new("Uuid").title("id")
    }

    fn fields() -> Vec<rustcms_admin::Field> {
        vec![rustcms_admin::Field::make("id").searchable(), rustcms_admin::Field::make("title")]
    }
}

db_test!(uuid_titles_and_fields_are_searchable_as_displayed, |db: TestDb| async move {
    let fx = Fixture::new(db).await;
    let admin_ctx = fx.ctx(admin());
    let mut keys = Vec::new();
    for title in ["a", "b"] {
        keys.push(fx.resource("posts").create(&admin_ctx, data(json!({"title": title}))).await.unwrap().key);
    }
    let registry = rustcms_admin::Registry::new().register::<UuidTitled>().finish();
    let ctx = rustcms_admin::Ctx::new(fx.db.conn().clone(), admin(), registry.clone());
    let resource = registry.get("uuid_titled").unwrap();
    let found = resource.lookup(&ctx, Some(&keys[0]), &[]).await.unwrap();
    assert_eq!(found, [(keys[0].clone(), keys[0].clone())], "exact displayed UUID");
    let upper = resource.lookup(&ctx, Some(&keys[1][..13].to_uppercase()), &[]).await.unwrap();
    assert!(upper.iter().any(|(k, _)| k == &keys[1]), "prefix, any case");
    let query = ListQuery { search: Some(keys[1].clone()), ..Default::default() };
    assert_eq!(resource.list(&ctx, &query).await.unwrap().meta.total, 1, "searchable UUID field");
});

/// Like `DistinctJoined`, with the scope itself ordering by label (descending).
struct DistinctOrdered;

impl rustcms_admin::Resource for DistinctOrdered {
    type Entity = support::tag::Entity;
    const SLUG: &'static str = "distinct_ordered";

    fn meta() -> rustcms_admin::ResourceMeta {
        rustcms_admin::ResourceMeta::new("Ordered").title("slug").key(rustcms_admin::KeyStrategy::Natural)
    }

    fn fields() -> Vec<rustcms_admin::Field> {
        vec![rustcms_admin::Field::make("slug").searchable(), rustcms_admin::Field::make("label")]
    }

    fn policy() -> Arc<dyn rustcms_admin::Policy> {
        Arc::new(QueryingPolicy)
    }

    fn scope(
        query: sea_orm::Select<support::tag::Entity>,
        ctx: &rustcms_admin::Ctx,
    ) -> sea_orm::Select<support::tag::Entity> {
        use sea_orm::QueryOrder;
        <DistinctJoined as rustcms_admin::Resource>::scope(query, ctx).order_by_desc(support::tag::Column::Label)
    }
}

db_test!(distinct_scopes_with_their_own_ordering, |db: TestDb| async move {
    let fx = Fixture::new(db).await;
    let admin_ctx = fx.ctx(admin());
    for (slug, label) in [("x1", "Alpha"), ("x2", "Beta")] {
        fx.resource("tags").create(&admin_ctx, data(json!({"slug": slug, "label": label}))).await.unwrap();
    }
    let registry = rustcms_admin::Registry::new().register::<DistinctOrdered>().finish();
    let ctx = rustcms_admin::Ctx::new(fx.db.conn().clone(), admin(), registry.clone());
    let resource = registry.get("distinct_ordered").unwrap();
    let keys: Vec<String> =
        resource.list(&ctx, &ListQuery::default()).await.unwrap().data.into_iter().map(|e| e.key).collect();
    assert_eq!(keys, ["x2", "x1"], "scope ordering kept");
    let options: Vec<String> =
        resource.lookup(&ctx, Some("x"), &[]).await.unwrap().into_iter().map(|(k, _)| k).collect();
    assert_eq!(options.len(), 2);
});

/// Tags whose label can be replaced but never read back.
struct WriteOnly;

impl rustcms_admin::Resource for WriteOnly {
    type Entity = support::tag::Entity;
    const SLUG: &'static str = "write_only";

    fn meta() -> rustcms_admin::ResourceMeta {
        rustcms_admin::ResourceMeta::new("Secret").key(rustcms_admin::KeyStrategy::Natural)
    }

    fn fields() -> Vec<rustcms_admin::Field> {
        vec![rustcms_admin::Field::make("slug"), rustcms_admin::Field::make("label").required().readable(|_| false)]
    }
}

db_test!(write_only_fields_stay_editable_without_their_value, |db: TestDb| async move {
    let fx = Fixture::new(db).await;
    fx.resource("tags").create(&fx.ctx(admin()), data(json!({"slug": "s", "label": "old secret"}))).await.unwrap();
    let registry = rustcms_admin::Registry::new().register::<WriteOnly>().finish();
    let ctx = rustcms_admin::Ctx::new(fx.db.conn().clone(), admin(), registry.clone());
    let resource = registry.get("write_only").unwrap();

    let schema =
        serde_json::to_value(resource.schema(&ctx, rustcms_admin::SchemaOperation::Edit, Some("s")).await.unwrap())
            .unwrap();
    let label = schema["form"]["components"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "label")
        .cloned()
        .expect("label offered");
    assert_eq!((label["disabled"].clone(), label["writeOnly"].clone()), (json!(false), json!(true)));
    assert!(label["rules"].as_array().unwrap().is_empty(), "empty keeps the stored value");

    let updated = resource.update(&ctx, "s", data(json!({"label": "new secret"}))).await.unwrap();
    assert!(!updated.data.contains_key("label"), "value never sent back");
    let stored = <support::tag::Entity as sea_orm::EntityTrait>::find_by_id("s".to_owned())
        .one(fx.db.conn())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.label, "new secret");
});

/// Categories whose scope breaks (invalid SQL) for principals listed in `BROKEN`.
struct FlakyTarget;

static BROKEN: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

impl rustcms_admin::Resource for FlakyTarget {
    type Entity = support::category::Entity;
    const SLUG: &'static str = "flaky_target";

    fn meta() -> rustcms_admin::ResourceMeta {
        rustcms_admin::ResourceMeta::new("Flaky").title("name")
    }

    fn fields() -> Vec<rustcms_admin::Field> {
        vec![rustcms_admin::Field::make("name"), rustcms_admin::Field::make("slug")]
    }

    fn scope(
        query: sea_orm::Select<support::category::Entity>,
        ctx: &rustcms_admin::Ctx,
    ) -> sea_orm::Select<support::category::Entity> {
        use sea_orm::QueryFilter;
        if BROKEN.lock().unwrap().contains(&ctx.principal.id) {
            query.filter(sea_orm::sea_query::Expr::cust("no_such_column = 1"))
        } else {
            query
        }
    }
}

/// Posts pointing at `FlakyTarget`; `after_save` breaks the target for this principal,
/// so the label lookup after commit fails.
struct BreaksLabels;

#[async_trait::async_trait]
impl rustcms_admin::Resource for BreaksLabels {
    type Entity = support::post::Entity;
    const SLUG: &'static str = "breaks_labels";

    fn meta() -> rustcms_admin::ResourceMeta {
        rustcms_admin::ResourceMeta::new("Breaks").key(rustcms_admin::KeyStrategy::AppGenerated(|| {
            sea_orm::sea_query::Value::Uuid(Some(uuid::Uuid::now_v7()))
        }))
    }

    fn fields() -> Vec<rustcms_admin::Field> {
        vec![rustcms_admin::Field::make("title"), rustcms_admin::Field::make("category_id").belongs_to("flaky_target")]
    }

    async fn mutate_before_create(
        ctx: &rustcms_admin::Ctx,
        _txn: &sea_orm::DatabaseTransaction,
        mut data: Data,
    ) -> rustcms_core::AppResult<Data> {
        data.insert("author_id".into(), json!(ctx.principal.id.clone()));
        Ok(data)
    }

    async fn after_save(
        ctx: &rustcms_admin::Ctx,
        _txn: &sea_orm::DatabaseTransaction,
        _record: &support::post::Model,
    ) -> rustcms_core::AppResult<()> {
        BROKEN.lock().unwrap().push(ctx.principal.id.clone());
        Ok(())
    }
}

db_test!(committed_writes_never_report_failure, |db: TestDb| async move {
    use sea_orm::{EntityTrait, PaginatorTrait};
    let fx = Fixture::new(db).await;
    let category = fx.seed_category("News").await.parse::<i32>().unwrap();
    let registry = rustcms_admin::Registry::new().register::<FlakyTarget>().register::<BreaksLabels>().finish();
    let principal = rustcms_core::Principal { id: "424242".into(), super_admin: true, ..Default::default() };
    let ctx = rustcms_admin::Ctx::new(fx.db.conn().clone(), principal, registry.clone());
    let created =
        registry.get("breaks_labels").unwrap().create(&ctx, data(json!({"title": "t", "category_id": category}))).await;
    let created = created.expect("a committed create is reported as a success");
    assert!(uuid::Uuid::parse_str(&created.key).is_ok(), "the client gets the key, so it does not retry");
    assert_eq!(support::post::Entity::find().count(fx.db.conn()).await.unwrap(), 1);
});

/// Posts whose policy stalls on every check after a write (a slow dependency).
struct Stalling;

struct StallingPolicy;

#[async_trait::async_trait]
impl rustcms_admin::Policy for StallingPolicy {
    async fn allows(&self, _ctx: &rustcms_admin::Ctx, ability: &str, record: Option<&Data>) -> bool {
        if ability == "view" && record.is_some() {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        }
        true
    }
}

impl rustcms_admin::Resource for Stalling {
    type Entity = support::tag::Entity;
    const SLUG: &'static str = "stalling";

    fn meta() -> rustcms_admin::ResourceMeta {
        rustcms_admin::ResourceMeta::new("Stalling").key(rustcms_admin::KeyStrategy::Natural)
    }

    fn fields() -> Vec<rustcms_admin::Field> {
        vec![rustcms_admin::Field::make("slug"), rustcms_admin::Field::make("label")]
    }

    fn policy() -> Arc<dyn rustcms_admin::Policy> {
        Arc::new(StallingPolicy)
    }
}

db_test!(slow_post_commit_hydration_still_returns_the_key, |db: TestDb| async move {
    use sea_orm::{EntityTrait, PaginatorTrait};
    let fx = Fixture::new(db).await;
    let registry = rustcms_admin::Registry::new().register::<Stalling>().finish();
    let ctx = rustcms_admin::Ctx::new(fx.db.conn().clone(), admin(), registry.clone());
    let started = std::time::Instant::now();
    let created =
        registry.get("stalling").unwrap().create(&ctx, data(json!({"slug": "s", "label": "l"}))).await.unwrap();
    assert!(started.elapsed() < std::time::Duration::from_secs(5), "answered within the hydration bound");
    assert_eq!(created.key, "s");
    assert!(created.data.is_empty());
    assert_eq!(support::tag::Entity::find().count(fx.db.conn()).await.unwrap(), 1);
});

db_test!(writes_with_database_backed_policies_and_one_connection, |db: TestDb| async move {
    let fx = Fixture::new(db).await;
    let config =
        rustcms_core::config::DatabaseConfig { url: fx.db.url().to_owned(), max_connections: 1, log_statements: false };
    let single = rustcms_core::db::connect(&config).await.unwrap();
    let registry = rustcms_admin::Registry::new().register::<Queried>().finish();
    let ctx = rustcms_admin::Ctx::new(single, admin(), registry.clone());
    let queried = registry.get("queried").unwrap();
    let limit = std::time::Duration::from_secs(10);
    tokio::time::timeout(limit, queried.create(&ctx, data(json!({"slug": "a", "label": "A"}))))
        .await
        .expect("create")
        .unwrap();
    tokio::time::timeout(limit, queried.create(&ctx, data(json!({"slug": "b", "label": "B"}))))
        .await
        .expect("create")
        .unwrap();
    tokio::time::timeout(limit, queried.update(&ctx, "a", data(json!({"label": "A2"}))))
        .await
        .expect("update")
        .unwrap();
    tokio::time::timeout(limit, queried.delete(&ctx, "b")).await.expect("delete").unwrap();
    let page = tokio::time::timeout(limit, queried.list(&ctx, &ListQuery::default())).await.expect("list").unwrap();
    assert_eq!(page.meta.total, 1);
});

db_test!(ownership_changed_concurrently_is_seen_by_writes, |db: TestDb| async move {
    use sea_orm::ConnectionTrait;
    let fx = Fixture::new(db).await;
    let posts = fx.resource("posts");
    let owner = fx.ctx(editor("7"));
    let post = posts.create(&owner, data(json!({"title": "mine"}))).await.unwrap();

    // Another transaction transfers the post to user 8 and commits while the owner's update waits.
    let transfer = rustcms_core::db::begin_write(fx.db.conn()).await.unwrap();
    let sql = match transfer.get_database_backend() {
        sea_orm::DbBackend::Postgres => format!("UPDATE posts SET author_id = 8 WHERE id = '{}'", post.key),
        _ => format!("UPDATE posts SET author_id = 8 WHERE id = X'{}'", post.key.replace('-', "")),
    };
    transfer.execute_unprepared(&sql).await.unwrap();
    let posts2 = posts.clone();
    let key = post.key.clone();
    let update = tokio::spawn(async move { posts2.update(&owner, &key, data(json!({"title": "hijacked"}))).await });
    wait_until_blocked(&fx, &transfer).await;
    transfer.commit().await.unwrap();
    assert!(matches!(update.await.unwrap(), Err(rustcms_core::AppError::NotFound)), "out of the old owner's scope now");
    let new_owner = fx.ctx(editor("8"));
    assert_eq!(posts.find(&new_owner, &post.key).await.unwrap().data["title"], json!("mine"));
});

/// Starts a write transaction running `pg`/`sqlite` SQL against the post `key`
/// (left uncommitted, so writes to that row wait for it). MySQL runs the SQLite
/// form (`X'…'` literals for `binary(16)` keys).
async fn blocking_write(fx: &Fixture, key: &str, pg: &str, sqlite: &str) -> sea_orm::DatabaseTransaction {
    use sea_orm::ConnectionTrait;
    let txn = rustcms_core::db::begin_write(fx.db.conn()).await.unwrap();
    let sql = match txn.get_database_backend() {
        sea_orm::DbBackend::Postgres => pg.replace("{id}", &format!("'{key}'")),
        _ => sqlite.replace("{id}", &format!("X'{}'", key.replace('-', ""))),
    };
    txn.execute_unprepared(&sql).await.unwrap();
    txn
}

/// Waits until another session is blocked by `blocker` (PostgreSQL, MySQL), so
/// the race is exercised deterministically. SQLite writers serialize on the
/// database lock and reach the same outcome whatever the timing.
async fn wait_until_blocked(fx: &Fixture, blocker: &sea_orm::DatabaseTransaction) {
    use sea_orm::{ConnectionTrait, Statement};
    if blocker.get_database_backend() == sea_orm::DbBackend::MySql {
        let id: u64 = blocker
            .query_one_raw(Statement::from_string(sea_orm::DbBackend::MySql, "SELECT CONNECTION_ID() AS id"))
            .await
            .unwrap()
            .unwrap()
            .try_get("", "id")
            .unwrap();
        let probe = format!(
            "SELECT COUNT(*) AS n FROM performance_schema.data_lock_waits w \
             JOIN performance_schema.threads t ON t.THREAD_ID = w.BLOCKING_THREAD_ID WHERE t.PROCESSLIST_ID = {id}"
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let row = fx
                .db
                .conn()
                .query_one_raw(Statement::from_string(sea_orm::DbBackend::MySql, probe.clone()))
                .await
                .unwrap()
                .unwrap();
            if row.try_get::<i64>("", "n").unwrap() > 0 {
                return;
            }
            assert!(std::time::Instant::now() < deadline, "writer never blocked on the lock");
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }
    if blocker.get_database_backend() != sea_orm::DbBackend::Postgres {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        return;
    }
    let pid: i32 = blocker
        .query_one_raw(Statement::from_string(sea_orm::DbBackend::Postgres, "SELECT pg_backend_pid() AS pid"))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "pid")
        .unwrap();
    let probe = format!("SELECT count(*)::int4 AS n FROM pg_stat_activity WHERE {pid} = ANY(pg_blocking_pids(pid))");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let row = fx
            .db
            .conn()
            .query_one_raw(Statement::from_string(sea_orm::DbBackend::Postgres, probe.clone()))
            .await
            .unwrap()
            .unwrap();
        if row.try_get::<i32>("", "n").unwrap() > 0 {
            return;
        }
        assert!(std::time::Instant::now() < deadline, "writer never blocked on the lock");
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
}

db_test!(records_deleted_while_writes_wait_are_missing, |db: TestDb| async move {
    let fx = Fixture::new(db).await;
    let posts = fx.resource("posts");
    let owner = fx.ctx(editor("7"));
    let a = posts.create(&owner, data(json!({"title": "a"}))).await.unwrap();
    let b = posts.create(&owner, data(json!({"title": "b"}))).await.unwrap();
    let delete = "DELETE FROM posts WHERE id = {id}";

    let blocker = blocking_write(&fx, &a.key, delete, delete).await;
    let (posts2, ctx, key) = (posts.clone(), owner.clone(), a.key.clone());
    let update = tokio::spawn(async move { posts2.update(&ctx, &key, data(json!({"title": "x"}))).await });
    wait_until_blocked(&fx, &blocker).await;
    blocker.commit().await.unwrap();
    assert!(matches!(update.await.unwrap(), Err(rustcms_core::AppError::NotFound)));

    let blocker = blocking_write(&fx, &b.key, delete, delete).await;
    let (posts2, ctx, key) = (posts.clone(), owner.clone(), b.key.clone());
    let action = tokio::spawn(async move { posts2.run_action(&ctx, "touch", &[key], Data::new()).await });
    wait_until_blocked(&fx, &blocker).await;
    blocker.commit().await.unwrap();
    let outcome = action.await.unwrap().unwrap();
    assert!(outcome.processed.is_empty() && outcome.denied.is_empty());
    assert_eq!(outcome.missing, vec![b.key.clone()], "a vanished record is reported, not dropped");
});

db_test!(replacement_rows_inserted_while_writes_wait_are_not_written, |db: TestDb| async move {
    let fx = Fixture::new(db).await;
    if fx.db.conn().get_database_backend() != sea_orm::DbBackend::Postgres {
        // SQLite writers hold the database lock: no row can be replaced meanwhile.
        // MySQL point-locks the row now under the key, and that row is the one
        // re-checked and written (no stale-row case to guard).
        return;
    }
    let posts = fx.resource("posts");
    let owner = fx.ctx(editor("7"));
    let post = posts.create(&owner, data(json!({"title": "old"}))).await.unwrap();
    // Delete the row and insert a new one under the same key in one statement.
    let replace = "WITH d AS (DELETE FROM posts WHERE id = {id} RETURNING *) \
                   INSERT INTO posts SELECT * FROM d";

    let blocker = blocking_write(&fx, &post.key, replace, "").await;
    let (posts2, ctx, key) = (posts.clone(), owner.clone(), post.key.clone());
    let update = tokio::spawn(async move { posts2.update(&ctx, &key, data(json!({"title": "x"}))).await });
    wait_until_blocked(&fx, &blocker).await;
    blocker.commit().await.unwrap();
    assert!(matches!(update.await.unwrap(), Err(rustcms_core::AppError::NotFound)), "replacement not locked");

    let blocker = blocking_write(&fx, &post.key, replace, "").await;
    let (posts2, ctx, key) = (posts.clone(), owner.clone(), post.key.clone());
    let action = tokio::spawn(async move { posts2.run_action(&ctx, "touch", &[key], Data::new()).await });
    wait_until_blocked(&fx, &blocker).await;
    blocker.commit().await.unwrap();
    let outcome = action.await.unwrap().unwrap();
    assert!(outcome.processed.is_empty());
    assert_eq!(outcome.missing, vec![post.key.clone()]);
    assert_eq!(posts.find(&owner, &post.key).await.unwrap().data["title"], json!("old"));
});

db_test!(lists_follow_the_default_sort_without_a_sort_parameter, |db: TestDb| async move {
    let fx = Fixture::new(db).await;
    let posts = fx.resource("posts");
    let ctx = fx.ctx(admin());
    for title in ["b", "c", "a"] {
        posts.create(&ctx, data(json!({"title": title}))).await.unwrap();
    }
    let titles = |page: rustcms_admin::Page| page.data.iter().map(|e| e.data["title"].clone()).collect::<Vec<_>>();
    // The posts table announces `-title`; the list honours it.
    assert_eq!(titles(posts.list(&ctx, &ListQuery::default()).await.unwrap()), [json!("c"), json!("b"), json!("a")]);
    let asc = ListQuery { sort: Some("title".into()), ..Default::default() };
    assert_eq!(titles(posts.list(&ctx, &asc).await.unwrap()), [json!("a"), json!("b"), json!("c")]);
});

db_test!(write_responses_respect_the_scope_after_the_write, |db: TestDb| async move {
    let fx = Fixture::new(db).await;
    let categories = fx.resource("categories");
    let writer = fx.ctx(editor("7").with(&["categories.create", "categories.update"]));

    let hidden = categories.create(&writer, data(json!({"name": "Hidden plan", "slug": "plan"}))).await.unwrap();
    assert!(hidden.data.is_empty() && hidden.title.is_none() && hidden.labels.is_empty(), "{hidden:?}");
    assert!(hidden.abilities.is_empty(), "nothing is said about a record out of scope");

    let open = categories.create(&writer, data(json!({"name": "Open", "slug": "open"}))).await.unwrap();
    assert_eq!(open.data["name"], json!("Open"), "reverse check: in scope, the record is returned");
    let moved = categories.update(&writer, &open.key, data(json!({"name": "Hidden now"}))).await.unwrap();
    assert_eq!(moved.key, open.key);
    assert!(moved.data.is_empty() && moved.title.is_none(), "{moved:?}");

    // Whoever may see hidden categories gets the data.
    let admin_ctx = fx.ctx(admin());
    assert_eq!(categories.find(&admin_ctx, &hidden.key).await.unwrap().data["name"], json!("Hidden plan"));
});

db_test!(lookups_by_key_do_not_reveal_existence_without_the_ability, |db: TestDb| async move {
    use rustcms_admin::SchemaOperation;
    let fx = Fixture::new(db).await;
    let tags = fx.resource("tags");
    tags.create(&fx.ctx(admin()), data(json!({"slug": "rust", "label": "Rust"}))).await.unwrap();

    let nobody = fx.ctx(editor("9").without_all());
    for key in ["rust", "missing"] {
        assert!(matches!(tags.find(&nobody, key).await, Err(AppError::Forbidden)), "find {key}");
        assert!(matches!(tags.update(&nobody, key, Data::new()).await, Err(AppError::Forbidden)), "update {key}");
        assert!(matches!(tags.delete(&nobody, key).await, Err(AppError::Forbidden)), "delete {key}");
        for op in [SchemaOperation::View, SchemaOperation::Edit] {
            assert!(matches!(tags.schema(&nobody, op, Some(key)).await, Err(AppError::Forbidden)), "schema {key}");
        }
    }

    // Reverse check: with the ability, a missing key is still 404.
    let viewer = fx.ctx(editor("7"));
    assert!(matches!(tags.find(&viewer, "missing").await, Err(AppError::NotFound)));
    assert_eq!(tags.find(&viewer, "rust").await.unwrap().data["label"], json!("Rust"));

    // A blind edit (update without view) still works and returns no data.
    let blind = fx.ctx(editor("9").without_all().with(&["tags.update"]));
    let updated = tags.update(&blind, "rust", data(json!({"label": "Rust 2"}))).await.unwrap();
    assert_eq!(updated.key, "rust");
    assert!(updated.data.is_empty());
    assert!(matches!(tags.update(&blind, "missing", Data::new()).await, Err(AppError::NotFound)));
});

pub mod gauge {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "gauges")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i32,
        pub narrow: f32,
        pub wide: f64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

struct Gauges;

impl rustcms_admin::Resource for Gauges {
    type Entity = gauge::Entity;
    const SLUG: &'static str = "gauges";

    fn meta() -> rustcms_admin::ResourceMeta {
        rustcms_admin::ResourceMeta::new("Gauge")
    }

    fn fields() -> Vec<rustcms_admin::Field> {
        vec![rustcms_admin::Field::column(gauge::Column::Narrow), rustcms_admin::Field::column(gauge::Column::Wide)]
    }
}

db_test!(floats_out_of_f32_range_are_rejected, |db: TestDb| async move {
    use sea_orm::Schema;
    let backend = db.conn().get_database_backend();
    rustcms_testing::create_table(db.conn(), &Schema::new(backend).create_table_from_entity(gauge::Entity)).await;
    let registry = rustcms_admin::Registry::new().register::<Gauges>().finish();
    let ctx = rustcms_admin::Ctx::new(db.conn().clone(), admin(), registry.clone());
    let gauges = registry.get("gauges").unwrap();

    let err = gauges.create(&ctx, data(json!({"narrow": 1e100, "wide": 1.0}))).await.unwrap_err();
    assert_eq!(field_errors(err), vec![("narrow".to_owned(), vec!["is out of range".to_owned()])]);

    // Reverse check: in range for `f32`, and any finite `f64` for `f64`.
    let created = gauges.create(&ctx, data(json!({"narrow": 1.5, "wide": 1e100}))).await.unwrap();
    assert_eq!(created.data["narrow"], json!(1.5));
    assert_eq!(created.data["wide"], json!(1e100));
});
