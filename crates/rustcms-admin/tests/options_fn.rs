//! Options computed on the server (`options_fn`, ADR 0044 part E): who may
//! ask `form/options`, what the function sees, how its answer is checked,
//! the save's preflight and locked check (values outside the answer, stale
//! dependencies, never a value the user cannot observe), replays, imports,
//! and declarations that cannot hold refusing to boot. SQLite, PostgreSQL,
//! MySQL.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use rustcms_admin::schema::{FormOptionsRequest, FormOptionsResponse};
use rustcms_admin::*;
use rustcms_auth::{ADMIN_GUARD, Actor, AuthProvider, AuthState, RbacService};
use rustcms_core::{AppBuilder, AppError, AppResult, Config, Principal};
use rustcms_testing::{Factory, TestApp, TestClient, TestDb, UserFactory, db_test};
use sea_orm::{ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, Schema};
use serde_json::{Value, json};
use support::PrincipalExt;

mod support;

pub mod entry {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "option_entries")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i32,
        #[sea_orm(unique)]
        pub title: String,
        pub group_id: Option<i32>,
        pub category: Option<String>,
        pub hint: Option<String>,
        pub topic: Option<String>,
        pub tags: Option<Json>,
        pub rank: Option<i64>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod group {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "option_groups")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i32,
        pub name: String,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// The titles of the entries whose options were computed, in order.
static CALLS: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn calls(title: &str) -> usize {
    CALLS.lock().unwrap().iter().filter(|t| *t == title).count()
}

fn option(value: impl Into<Value>, label: &str) -> SelectOption {
    SelectOption { value: value.into(), label: label.to_owned() }
}

/// Sets `column` of the entry titled `title`, behind the save's back.
async fn store(db: &sea_orm::DatabaseConnection, title: &str, column: entry::Column, value: Value) {
    entry::Entity::update_many()
        .col_expr(column, sea_orm::sea_query::Expr::value(sea_orm::Value::from(value.as_str().map(str::to_owned))))
        .filter(entry::Column::Title.eq(title))
        .exec(db)
        .await
        .unwrap();
}

/// A category's topics; the title of the entry steers failures and changes
/// made behind the save's back.
fn topics<'a>(ctx: &'a Ctx, state: &'a FormState) -> BoxFuture<'a, AppResult<Vec<SelectOption>>> {
    Box::pin(async move {
        let title = state.text("title").to_owned();
        CALLS.lock().unwrap().push(title.clone());
        match title.as_str() {
            "boom" => return Err(AppError::internal("no topics today")),
            "slow" => tokio::time::sleep(Duration::from_millis(300)).await,
            "wait" => tokio::time::sleep(Duration::from_millis(80)).await,
            "many" => return Ok((0..501).map(|i| option(format!("t{i}"), "T")).collect()),
            "dup" => return Ok(vec![option("apple", "Apple"), option("apple", "Apple again")]),
            // A second pooled connection: none is held by the save meanwhile.
            "conn" => {
                ctx.db.execute_unprepared("SELECT 1").await?;
            }
            _ => {}
        }
        // Changes made by someone else while the save runs the function (once:
        // read from the record, whatever the user may read).
        if title.starts_with("touch") {
            let row = entry::Entity::find().filter(entry::Column::Title.eq(&title)).one(&ctx.db).await?;
            if let Some(row) = row {
                if title.starts_with("touch hint") && row.hint.as_deref() == Some("a") {
                    store(&ctx.db, &title, entry::Column::Hint, json!("b")).await;
                }
                if title.starts_with("touch category") && row.category.as_deref() == Some("fruit") {
                    store(&ctx.db, &title, entry::Column::Category, json!("veg")).await;
                }
            }
        }
        if title == "go blind" {
            store(&ctx.db, &title, entry::Column::Title, json!("Blind now")).await;
        }
        Ok(match state.text("category") {
            "fruit" => vec![option("apple", "Apple"), option("pear", "Pear")],
            "veg" => vec![option("kale", "Kale")],
            // Through a relation, the key it binds.
            "group" => vec![option(format!("g{}", state.get("group_id").cloned().unwrap_or(Value::Null)), "Group")],
            _ => Vec::new(),
        })
    })
}

fn tags<'a>(_: &'a Ctx, state: &'a FormState) -> BoxFuture<'a, AppResult<Vec<SelectOption>>> {
    Box::pin(async move {
        Ok(match (state.text("title"), state.text("category")) {
            ("bad tags", _) => vec![option(Value::Null, "Nothing")],
            (_, "fruit") => vec![option("sweet", "Sweet"), option("sour", "Sour")],
            (_, "veg") => vec![option("green", "Green")],
            _ => Vec::new(),
        })
    })
}

fn ranks<'a>(_: &'a Ctx, state: &'a FormState) -> BoxFuture<'a, AppResult<Vec<SelectOption>>> {
    Box::pin(async move {
        Ok(match state.text("title") {
            "bad rank" => vec![option("x", "X")],
            // JSON integers on an i64 column: the wire's strings.
            _ => vec![option(1, "One"), option(2, "Two"), option("3", "Three")],
        })
    })
}

/// View denied on entries titled `Blind…`; otherwise the permissions.
struct EntryPolicy;

#[async_trait::async_trait]
impl Policy for EntryPolicy {
    async fn allows(&self, ctx: &Ctx, ability: &str, record: Option<&Data>) -> bool {
        let blind = record.is_some_and(|r| r["title"].as_str().unwrap_or_default().starts_with("Blind"));
        ctx.can(&format!("option_entries.{ability}")) && !(ability == "view" && blind)
    }
}

pub struct EntryResource;

impl Resource for EntryResource {
    type Entity = entry::Entity;
    const SLUG: &'static str = "option_entries";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Entry")
            .title("title")
            .audit(false)
            .unique("option_entries_title_key", &["title"], "is taken")
            .import_key("title")
    }

    fn fields() -> Vec<Field> {
        use entry::Column as C;
        vec![
            Field::column(C::Title).required(),
            Field::column(C::GroupId).belongs_to("option_groups").creatable(|c| c.can("option_entries.set_group")),
            Field::column(C::Category),
            Field::column(C::Hint).readable(|c| c.can("option_entries.read_hint")),
            Field::column(C::Topic),
            Field::column(C::Tags),
            Field::column(C::Rank).readable(|c| !c.can("option_entries.hide_rank")),
        ]
    }

    fn form() -> Form {
        Form::new().schema(components![
            TextInput::make("title"),
            SelectInput::make("group_id"),
            SelectInput::make("category").options([("fruit", "Fruit"), ("veg", "Veg"), ("group", "Group")]),
            TextInput::make("hint"),
            // Declared options: shown until the answer comes (never checked).
            SelectInput::make("topic")
                .options([("apple", "Apple")])
                .depends_on(["category", "hint"])
                .options_fn(topics)
                .options_timeout(Duration::from_millis(100)),
            CheckboxList::make("tags").depends_on(["category"]).options_fn(tags),
            SelectInput::make("rank").shown_when(when("category").ne("veg")).depends_on(["category"]).options_fn(ranks),
        ])
    }

    fn policy() -> Arc<dyn Policy> {
        Arc::new(EntryPolicy)
    }

    fn scope(query: sea_orm::Select<entry::Entity>, _ctx: &Ctx) -> sea_orm::Select<entry::Entity> {
        query.filter(entry::Column::Title.not_like("Hidden%"))
    }
}

pub struct GroupResource;

impl Resource for GroupResource {
    type Entity = group::Entity;
    const SLUG: &'static str = "option_groups";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Group").title("name").audit(false)
    }

    fn fields() -> Vec<Field> {
        vec![Field::column(group::Column::Name).required()]
    }

    fn relations() -> Vec<Relation> {
        vec![Relation::has_many("entries", "option_entries", "group_id")]
    }
}

async fn tables(db: &TestDb) {
    let schema = Schema::new(db.conn().get_database_backend());
    rustcms_testing::create_table(db.conn(), &schema.create_table_from_entity(group::Entity)).await;
    rustcms_testing::create_table(db.conn(), &schema.create_table_from_entity(entry::Entity)).await;
}

async fn registry(db: &TestDb) -> Arc<Registry> {
    tables(db).await;
    Registry::new().register::<GroupResource>().register::<EntryResource>().finish()
}

fn data(value: Value) -> Data {
    value.as_object().unwrap().clone()
}

async fn ask(ctx: &Ctx, value: Value) -> Result<FormOptionsResponse, AppError> {
    let request: FormOptionsRequest = serde_json::from_value(value).unwrap();
    ctx.registry.get("option_entries").unwrap().form_options(ctx, request).await
}

fn values(answer: &FormOptionsResponse) -> Vec<Value> {
    answer.options.as_ref().expect("options").iter().map(|o| o.value.clone()).collect()
}

fn issues(err: AppError) -> Value {
    match err {
        AppError::Validation(errors) => serde_json::to_value(errors.issues()).unwrap(),
        other => panic!("not a validation error: {other:?}"),
    }
}

fn editor() -> Principal {
    Principal { id: "7".into(), guard: "admin".into(), ..Default::default() }.with(&[
        "option_entries.view_any",
        "option_entries.view",
        "option_entries.create",
        "option_entries.update",
    ])
}

async fn create(ctx: &Ctx, value: Value) -> AppResult<String> {
    Ok(ctx.registry.get("option_entries").unwrap().create(ctx, data(value)).await?.key)
}

async fn update(ctx: &Ctx, key: &str, value: Value) -> AppResult<Envelope> {
    ctx.registry.get("option_entries").unwrap().update(ctx, key, data(value)).await
}

async fn stored(db: &TestDb, key: &str) -> entry::Model {
    entry::Entity::find_by_id(key.parse::<i32>().unwrap()).one(db.conn()).await.unwrap().unwrap()
}

db_test!(the_route_answers_the_options_of_the_form_state, |db: TestDb| async move {
    let registry = registry(&db).await;
    let ctx = Ctx::new(db.conn().clone(), editor(), registry.clone());
    let answer =
        ask(&ctx, json!({"operation": "create", "input": "topic", "state": {"title": "A", "category": "fruit"}}))
            .await
            .unwrap();
    assert_eq!(
        serde_json::to_value(&answer).unwrap(),
        json!({"options": [{"value": "apple", "label": "Apple"}, {"value": "pear", "label": "Pear"}]})
    );
    let veg = json!({"operation": "create", "input": "topic", "state": {"category": "veg"}});
    assert_eq!(values(&ask(&ctx, veg).await.unwrap()), [json!("kale")]);
    // Values decoded to the column's kind and canonical (an i64 is a string).
    let rank = json!({"operation": "create", "input": "rank", "state": {"category": "fruit"}});
    assert_eq!(values(&ask(&ctx, rank).await.unwrap()), [json!("1"), json!("2"), json!("3")]);
    // Hidden by its conditions, unknown, without `options_fn`, unreadable: 404.
    for (input, state) in
        [("rank", json!({"category": "veg"})), ("nope", json!({})), ("title", json!({})), ("category", json!({}))]
    {
        let body = json!({"operation": "create", "input": input, "state": state});
        assert!(matches!(ask(&ctx, body).await, Err(AppError::NotFound)), "{input}");
    }
    let unreadable = Ctx::new(db.conn().clone(), editor().with(&["option_entries.hide_rank"]), registry.clone());
    let body = json!({"operation": "create", "input": "rank", "state": {"category": "fruit"}});
    assert!(matches!(ask(&unreadable, body).await, Err(AppError::NotFound)));

    // On edit, the stored values (a record the user may view) under what was sent.
    let admin = Ctx::new(db.conn().clone(), support::admin(), registry.clone());
    let own = create(&admin, json!({"title": "Stored", "category": "veg", "topic": "kale"})).await.unwrap();
    let hidden = create(&admin, json!({"title": "Hidden one"})).await.unwrap();
    let edit = |key: &str| json!({"operation": "edit", "key": key, "input": "topic", "state": {"title": "Renamed"}});
    assert_eq!(values(&ask(&ctx, edit(&own)).await.unwrap()), [json!("kale")]);
    assert!(matches!(ask(&ctx, edit(&hidden)).await, Err(AppError::NotFound)));
    // Admitted as the form's write: create, or update of that record.
    let reader = Ctx::new(db.conn().clone(), editor().without_all().with(&["option_entries.view"]), registry.clone());
    let create_body = json!({"operation": "create", "input": "topic", "state": {}});
    assert!(matches!(ask(&reader, create_body.clone()).await, Err(AppError::Forbidden)));
    assert!(matches!(ask(&reader, edit(&own)).await, Err(AppError::Forbidden)));
    for body in [
        json!({"operation": "view", "key": own, "input": "topic", "state": {}}),
        json!({"operation": "edit", "input": "topic", "state": {}}),
        json!({"operation": "create", "key": own, "input": "topic", "state": {}}),
    ] {
        assert!(issues(ask(&ctx, body).await.unwrap_err()).get("operation").is_some());
    }
});

db_test!(answers_the_server_refuses_are_failures, |db: TestDb| async move {
    let registry = registry(&db).await;
    let ctx = Ctx::new(db.conn().clone(), editor(), registry);
    // An error, a timeout, more than 500 options, a value given twice.
    for title in ["boom", "slow", "many", "dup"] {
        let body = json!({"operation": "create", "input": "topic", "state": {"title": title, "category": "fruit"}});
        assert_eq!(serde_json::to_value(ask(&ctx, body).await.unwrap()).unwrap(), json!({"failed": true}), "{title}");
    }
    // A value that is not of the column's kind; a list item without a value.
    let rank = json!({"operation": "create", "input": "rank", "state": {"title": "bad rank", "category": "fruit"}});
    assert!(ask(&ctx, rank).await.unwrap().failed);
    let list = json!({"operation": "create", "input": "tags", "state": {"title": "bad tags", "category": "fruit"}});
    assert!(ask(&ctx, list).await.unwrap().failed);
});

db_test!(saves_check_the_value_against_the_answer, |db: TestDb| async move {
    let registry = registry(&db).await;
    let ctx = Ctx::new(db.conn().clone(), editor(), registry.clone());
    let not_option = json!([rustcms_core::i18n::Issue::of("is not one of the options")]);
    let err = create(&ctx, json!({"title": "Kale fruit", "category": "fruit", "topic": "kale"})).await.unwrap_err();
    assert_eq!(issues(err), json!({"topic": not_option}));
    // The declared options are not checked: pear is in the answer only.
    let key = create(&ctx, json!({"title": "Pear", "category": "fruit", "topic": "pear", "rank": "2"})).await.unwrap();
    let row = stored(&db, &key).await;
    assert_eq!((row.topic.as_deref(), row.rank), (Some("pear"), Some(2)));
    // A checkbox list keeps its shape (an array, each item once) whatever the answer.
    for (tags, issue) in [
        (json!(["sweet", "sweet"]), "may only be given once"),
        (json!("sweet"), "is invalid"),
        (json!(["green"]), "is not one of the options"),
    ] {
        let err = create(&ctx, json!({"title": "Tags", "category": "fruit", "tags": tags})).await.unwrap_err();
        assert_eq!(issues(err), json!({"tags": [rustcms_core::i18n::Issue::of(issue)]}), "{tags}");
    }
    create(&ctx, json!({"title": "Tags", "category": "fruit", "tags": ["sour", "sweet"]})).await.unwrap();
    // An omitted value is checked when a dependency is sent (readable, viewable).
    let err = update(&ctx, &key, json!({"category": "veg"})).await.unwrap_err();
    assert_eq!(issues(err), json!({"topic": not_option}));
    update(&ctx, &key, json!({"category": "veg", "topic": "kale"})).await.unwrap();
    // An old record is never refused for an input the save leaves alone: no
    // dependency sent, no function run.
    store(db.conn(), "Pear", entry::Column::Topic, json!("gone")).await;
    let before = calls("Renamed pear");
    update(&ctx, &key, json!({"title": "Renamed pear"})).await.unwrap();
    assert_eq!(calls("Renamed pear"), before);
    // A failing function refuses the save, nothing written.
    let err = create(&ctx, json!({"title": "boom", "category": "fruit", "topic": "apple"})).await.unwrap_err();
    let form = match err {
        AppError::Validation(errors) => errors.form_errors,
        other => panic!("{other:?}"),
    };
    assert_eq!(form, ["could not check the options; try again"]);
    assert_eq!(rustcms_core::i18n::Issue::of(&form[0]).code, "options_failed");
});

db_test!(an_unreadable_value_never_decides_a_save, |db: TestDb| async move {
    let registry = registry(&db).await;
    let admin = Ctx::new(db.conn().clone(), support::admin(), registry.clone());
    let key = create(&admin, json!({"title": "Odd", "category": "fruit", "topic": "apple"})).await.unwrap();
    // Values no answer holds, stored behind the forms' back.
    store(db.conn(), "Odd", entry::Column::Topic, json!("gone")).await;
    entry::Entity::update_many()
        .col_expr(entry::Column::Rank, sea_orm::sea_query::Expr::value(9i64))
        .filter(entry::Column::Title.eq("Odd"))
        .exec(db.conn())
        .await
        .unwrap();
    // An update without `view`: neither stored value is the user's to see.
    let blind = Ctx::new(db.conn().clone(), editor().without_all().with(&["option_entries.update"]), registry.clone());
    update(&blind, &key, json!({"category": "fruit"})).await.unwrap();
    // A field the user may not read: its stored rank is never checked.
    let unreadable = Ctx::new(db.conn().clone(), editor().with(&["option_entries.hide_rank"]), registry.clone());
    let err = update(&unreadable, &key, json!({"category": "fruit"})).await.unwrap_err();
    assert_eq!(issues(err), json!({"topic": [rustcms_core::i18n::Issue::of("is not one of the options")]}));
    // Who may read both sees both refused.
    let reader = Ctx::new(db.conn().clone(), editor(), registry.clone());
    let err = update(&reader, &key, json!({"category": "fruit"})).await.unwrap_err();
    let issues = issues(err);
    assert!(issues.get("topic").is_some() && issues.get("rank").is_some(), "{issues}");
});

db_test!(a_dependency_changed_meanwhile_is_a_conflict, |db: TestDb| async move {
    let registry = registry(&db).await;
    let admin = Ctx::new(db.conn().clone(), support::admin(), registry.clone());
    let key = create(&admin, json!({"title": "touch category", "category": "fruit", "topic": "apple"})).await.unwrap();
    // The function read fruit; the record says veg under the lock.
    let err = update(&admin, &key, json!({"topic": "pear"})).await.unwrap_err();
    assert!(matches!(err, AppError::Conflict { code: "stale", .. }), "{err:?}");
    assert_eq!(stored(&db, &key).await.topic.as_deref(), Some("apple"));

    // A dependency the user may not read never causes a 409 (nothing tells it changed).
    let hint = create(&admin, json!({"title": "touch hint one", "category": "fruit", "hint": "a"})).await.unwrap();
    let editor = Ctx::new(db.conn().clone(), editor(), registry.clone());
    update(&editor, &hint, json!({"topic": "pear"})).await.unwrap();
    // Who reads it: 409.
    let other = create(&admin, json!({"title": "touch hint two", "category": "fruit", "hint": "a"})).await.unwrap();
    let err = update(&admin, &other, json!({"topic": "pear"})).await.unwrap_err();
    assert!(matches!(err, AppError::Conflict { code: "stale", .. }), "{err:?}");

    // Which inputs are checked changed meanwhile (the record no longer viewable): 409.
    let blind = create(&admin, json!({"title": "go blind", "category": "fruit", "topic": "apple"})).await.unwrap();
    let err = update(&editor, &blind, json!({"category": "fruit"})).await.unwrap_err();
    assert!(matches!(err, AppError::Conflict { code: "stale", .. }), "{err:?}");
});

db_test!(the_function_runs_without_the_transaction, |db: TestDb| async move {
    let registry = registry(&db).await;
    // One connection: the function's query would wait for the save's own.
    let mut options = sea_orm::ConnectOptions::new(db.url().to_owned());
    options.max_connections(1).min_connections(1).acquire_timeout(Duration::from_secs(5)).sqlx_logging(false);
    let single = sea_orm::Database::connect(options).await.unwrap();
    let ctx = Ctx::new(single, editor(), registry);
    let key = create(&ctx, json!({"title": "conn", "category": "fruit", "topic": "apple"})).await.unwrap();
    update(&ctx, &key, json!({"topic": "pear"})).await.unwrap();
    assert_eq!(calls("conn"), 2);
});

db_test!(committed_retries_run_no_function, |db: TestDb| async move {
    db.migrate::<rustcms_migration::Migrator>().await;
    let registry = registry(&db).await;
    let ctx = Ctx::new(db.conn().clone(), editor(), registry).with_idempotency_key(Some("k-options".into()));
    let body = json!({"title": "replayed", "category": "fruit", "topic": "apple"});
    let key = create(&ctx, body.clone()).await.unwrap();
    assert_eq!(calls("replayed"), 1);
    assert_eq!(create(&ctx, body).await.unwrap(), key);
    assert_eq!(calls("replayed"), 1);
    let ctx = ctx.with_idempotency_key(Some("k-update".into()));
    update(&ctx, &key, json!({"topic": "pear"})).await.unwrap();
    update(&ctx, &key, json!({"topic": "pear"})).await.unwrap();
    assert_eq!(calls("replayed"), 2);
});

db_test!(the_function_sees_the_relation_and_the_route_is_bounded, |db: TestDb| async move {
    let (app, admin) = served(db).await;
    let client = app.acting_as(&admin).await;
    let created = client.post_json(&app.admin_api("resources/option_groups/records"), &json!({"name": "G"})).await;
    created.assert_status(StatusCode::CREATED);
    let group = created.json()["data"]["key"].as_str().unwrap().to_owned();
    let url = app.admin_api("resources/option_entries/form/options");
    let via = json!({"operation": "create", "input": "topic", "state": {"category": "group", "group_id": 999},
        "via": {"parent": "option_groups", "record": group, "relation": "entries"}});
    // The key the relation binds, not the one sent.
    let answer = client.post_json(&url, &via).await;
    answer.assert_status(StatusCode::OK);
    assert_eq!(answer.json()["options"][0]["value"], json!(format!("g{group}")));
    // Without the relation's create (its key not writable): 403.
    let writer = user_with(&app, "writer", &["create", "view", "view_any"]).await;
    let writer = app.acting_as(&writer).await;
    writer.post_json(&url, &via).await.assert_status(StatusCode::FORBIDDEN);
    // One bucket with reactions: 120 a minute (one used above).
    let body = json!({"operation": "create", "input": "topic", "state": {"category": "veg"}});
    for _ in 1..120 {
        client.post_json(&url, &body).await.assert_status(StatusCode::OK);
    }
    client.post_json(&url, &body).await.assert_status(StatusCode::TOO_MANY_REQUESTS);
});

db_test!(a_user_has_two_turns_at_once, |db: TestDb| async move {
    let registry = registry(&db).await;
    let ctx = Ctx::new(db.conn().clone(), editor(), registry);
    let body = json!({"operation": "create", "input": "topic", "state": {"title": "wait", "category": "fruit"}});
    let (a, b, c) = tokio::join!(ask(&ctx, body.clone()), ask(&ctx, body.clone()), ask(&ctx, body));
    assert!(a.is_ok() && b.is_ok(), "{a:?} {b:?}");
    assert!(matches!(c, Err(AppError::TooManyRequests { .. })), "{c:?}");
});

db_test!(schemas_say_where_options_come_from, |db: TestDb| async move {
    let registry = registry(&db).await;
    let ctx = Ctx::new(db.conn().clone(), editor(), registry.clone());
    let entries = registry.get("option_entries").unwrap();
    let key = create(&ctx, json!({"title": "Schema", "category": "veg", "topic": "kale"})).await.unwrap();
    let from = |schema: &ResourceSchema| -> Value {
        let form = serde_json::to_value(&schema.form).unwrap();
        let mut out = serde_json::Map::new();
        for input in form["components"].as_array().unwrap() {
            if let Some(from) = input.get("optionsFrom") {
                out.insert(input["name"].as_str().unwrap().to_owned(), from.clone());
            }
        }
        Value::Object(out)
    };
    let expected = json!({
        "topic": {"dependsOn": ["category", "hint"]},
        "tags": {"dependsOn": ["category"]},
        "rank": {"dependsOn": ["category"]},
    });
    let create_schema = entries.schema(&ctx, SchemaOperation::Create, None).await.unwrap();
    assert_eq!(from(&create_schema), expected);
    // The declared options are sent as they are: no function runs for a schema.
    let form = serde_json::to_value(&create_schema.form).unwrap();
    let topic = form["components"].as_array().unwrap().iter().find(|c| c["name"] == json!("topic")).unwrap().clone();
    assert_eq!(topic["options"], json!([{"value": "apple", "label": "Apple"}]));
    assert_eq!(from(&entries.schema(&ctx, SchemaOperation::Edit, Some(&key)).await.unwrap()), expected);
    assert_eq!(from(&entries.schema(&ctx, SchemaOperation::View, Some(&key)).await.unwrap()), json!({}));
    // Not for an input the user may not read (write-only).
    let unreadable = Ctx::new(db.conn().clone(), editor().with(&["option_entries.hide_rank"]), registry.clone());
    let schema = entries.schema(&unreadable, SchemaOperation::Create, None).await.unwrap();
    assert!(from(&schema).get("rank").is_none());
});

fn build(config: Config) -> AppBuilder {
    // Uploaded CSV files (imports) go to a temporary directory of this app.
    let root = std::env::temp_dir().join(format!("rustcms-options-{}", uuid::Uuid::now_v7()));
    let config = config.with("storage.root", root.to_string_lossy()).expect("storage config");
    let mut app = AppBuilder::new(config);
    app.provider(rustcms_core::storage::StorageProvider);
    app.provider(AuthProvider::new());
    app.provider(AdminProvider::new().resource::<GroupResource>().resource::<EntryResource>());
    app
}

/// The app with the entries, and its super admin.
async fn served(db: TestDb) -> (TestApp, rustcms_core::models::user::Model) {
    db.migrate::<rustcms_migration::Migrator>().await;
    tables(&db).await;
    let app = TestApp::new(build, db).await;
    let auth = app.state().service::<AuthState>().unwrap();
    let conn = app.state().db();
    let rbac = RbacService::new(conn, &auth.resolver);
    rbac.sync_permissions(&auth.permissions.all()).await.unwrap();
    let admin = UserFactory::new().name("Admin").create(conn).await;
    let super_admin = rustcms_auth::entities::role::super_admin_id(conn, ADMIN_GUARD).await.unwrap().unwrap();
    rbac.assign_role(Actor::System, admin.id, super_admin).await.unwrap();
    (app, admin)
}

/// A user with these abilities on entries (and viewing groups).
async fn user_with(app: &TestApp, name: &str, abilities: &[&str]) -> rustcms_core::models::user::Model {
    let auth = app.state().service::<AuthState>().unwrap();
    let db = app.state().db();
    let rbac = RbacService::new(db, &auth.resolver);
    let mut permissions: Vec<String> = abilities.iter().map(|a| format!("option_entries.{a}")).collect();
    permissions.extend(["option_groups.view".to_owned(), "option_groups.view_any".to_owned()]);
    let user = UserFactory::new().name(name).create(db).await;
    let role = rbac.create_role(Actor::System, ADMIN_GUARD, name, &permissions).await.unwrap();
    rbac.assign_role(Actor::System, user.id, role.id).await.unwrap();
    user
}

async fn upload(client: &TestClient<'_>, app: &TestApp, csv: &str) -> String {
    let request = Request::builder()
        .method(Method::POST)
        .uri(app.admin_api("resources/option_entries/imports"))
        .header(header::CONTENT_TYPE, "text/csv")
        .body(Body::from(csv.to_owned()))
        .unwrap();
    let response = client.request(request).await;
    response.assert_status(StatusCode::CREATED);
    response.json()["id"].as_str().unwrap().to_owned()
}

/// Runs the queue, retried jobs included (their backoff skipped).
async fn work(app: &TestApp) {
    for _ in 0..3 {
        rustcms_core::queue::run_until_idle(app.state(), None, 10).await.unwrap();
        rustcms_core::models::job::Entity::update_many()
            .col_expr(
                rustcms_core::models::job::Column::AvailableAt,
                sea_orm::sea_query::Expr::value(chrono::Utc::now()),
            )
            .exec(app.state().db())
            .await
            .unwrap();
    }
}

db_test!(imports_check_options_when_they_run_and_retry_stale_rows, |db: TestDb| async move {
    let (app, admin) = served(db).await;
    let client = app.acting_as(&admin).await;
    let records = app.admin_api("resources/option_entries/records");
    let made =
        client.post_json(&records, &json!({"title": "touch hint import", "category": "fruit", "hint": "a"})).await;
    made.assert_status(StatusCode::CREATED);
    let csv = "Title,Category,Topic\r\nNew,fruit,apple\r\nWrong,fruit,kale\r\ntouch hint import,fruit,pear\r\n";
    let id = upload(&client, &app, csv).await;
    // The dry run checks without the database: no options.
    let mapping = json!({"mapping": {"Title": "title", "Category": "category", "Topic": "topic"}});
    let preview = client.post_json(&app.admin_api(&format!("imports/{id}/preview")), &mapping).await;
    preview.assert_status(StatusCode::OK);
    assert_eq!(preview.json()["valid"], json!(3), "{}", preview.json());
    client.post_json(&app.admin_api(&format!("imports/{id}/run")), &mapping).await.assert_status(StatusCode::OK);
    work(&app).await;
    let shown = client.get(&app.admin_api(&format!("imports/{id}"))).await.json();
    // The last row's hint changed while its options were computed: the job ran
    // again (rows written are skipped), and the row went through.
    assert_eq!(
        (shown["status"].clone(), shown["created"].clone(), shown["updated"].clone(), shown["failed"].clone()),
        (json!("done"), json!(1), json!(1), json!(1)),
        "{shown}"
    );
    assert_eq!(shown["report"][0]["code"], json!("not_option"), "{shown}");
    let row = entry::Entity::find().filter(entry::Column::Title.eq("touch hint import")).one(app.state().db());
    let row = row.await.unwrap().unwrap();
    assert_eq!((row.topic.as_deref(), row.hint.as_deref()), (Some("pear"), Some("b")));
});

/// A resource whose form is `form`, registered to check it at boot.
fn boot(form: fn() -> Form) {
    struct Bad;
    static FORM: std::sync::OnceLock<fn() -> Form> = std::sync::OnceLock::new();
    FORM.set(form).ok();
    impl Resource for Bad {
        type Entity = entry::Entity;
        const SLUG: &'static str = "bad";
        fn meta() -> ResourceMeta {
            ResourceMeta::new("Bad")
        }
        fn fields() -> Vec<Field> {
            use entry::Column as C;
            vec![
                Field::column(C::Title),
                Field::column(C::GroupId).belongs_to("option_groups"),
                Field::column(C::Category),
                Field::column(C::Topic),
                Field::column(C::Tags),
            ]
        }
        fn form() -> Form {
            FORM.get().unwrap()()
        }
    }
    Registry::new().register::<GroupResource>().register::<Bad>().finish();
}

#[test]
#[should_panic(expected = "needs `depends_on([...])`")]
fn options_need_their_dependencies() {
    boot(|| Form::new().schema(components![SelectInput::make("topic").options_fn(topics)]));
}

#[test]
#[should_panic(expected = "need `options_fn`")]
fn dependencies_need_options() {
    boot(|| Form::new().schema(components![SelectInput::make("topic").depends_on(["category"])]));
}

#[test]
#[should_panic(expected = "at most")]
fn the_options_timeout_is_bounded() {
    boot(|| {
        Form::new().schema(components![
            SelectInput::make("topic")
                .depends_on([] as [&str; 0])
                .options_fn(topics)
                .options_timeout(Duration::from_secs(31))
        ])
    });
}

#[test]
#[should_panic(expected = "not `belongs_to`")]
fn belongs_to_selects_take_their_options_from_the_relation() {
    boot(|| {
        Form::new().schema(components![SelectInput::make("group_id").depends_on([] as [&str; 0]).options_fn(topics)])
    });
}

#[test]
#[should_panic(expected = "for selects and checkbox lists")]
fn text_inputs_have_no_options() {
    boot(|| Form::new().schema(components![TextInput::make("topic").depends_on([] as [&str; 0]).options_fn(topics)]));
}

#[test]
#[should_panic(expected = "has one input")]
fn a_field_with_options_has_one_input() {
    boot(|| {
        Form::new().schema(components![
            SelectInput::make("topic").depends_on([] as [&str; 0]).options_fn(topics),
            SelectInput::make("topic")
        ])
    });
}

#[test]
#[should_panic(expected = "with a scalar or list value")]
fn dependencies_are_scalar_inputs_of_the_form() {
    boot(|| {
        Form::new().schema(components![
            Repeater::make("tags").schema(components![TextInput::make("name")]),
            SelectInput::make("topic").depends_on(["tags"]).options_fn(topics)
        ])
    });
}

#[test]
#[should_panic(expected = "cannot have `options_fn`")]
fn row_inputs_have_no_computed_options() {
    boot(|| {
        Form::new().schema(components![
            Repeater::make("tags")
                .schema(components![SelectInput::make("name").depends_on([] as [&str; 0]).options_fn(topics)])
        ])
    });
}

#[test]
#[should_panic(expected = "the child of a relationship repeater cannot have")]
fn children_of_relationship_repeaters_have_no_computed_options() {
    struct Parent;
    impl Resource for Parent {
        type Entity = group::Entity;
        const SLUG: &'static str = "option_groups";
        fn meta() -> ResourceMeta {
            ResourceMeta::new("Group")
        }
        fn fields() -> Vec<Field> {
            vec![Field::column(group::Column::Name)]
        }
        fn relations() -> Vec<Relation> {
            vec![Relation::has_many("entries", "option_entries", "group_id")]
        }
        fn form() -> Form {
            Form::new().schema(components![
                TextInput::make("name"),
                Repeater::make("entries").relationship().schema(components![TextInput::make("title")])
            ])
        }
    }
    Registry::new().register::<Parent>().register::<EntryResource>().finish();
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct Levels {
    level: String,
}

impl rustcms_core::settings::Settings for Levels {
    const GROUP: &'static str = "levels";
}

#[async_trait::async_trait]
impl SettingsPage for Levels {
    fn meta() -> SettingsMeta {
        SettingsMeta::new("Levels")
    }

    fn fields() -> Vec<SettingField> {
        vec![SettingField::select("level", [("a", "A")])]
    }

    fn form() -> Form {
        Form::new().schema(components![
            SelectInput::make("level").options([("a", "A")]).depends_on([] as [&str; 0]).options_fn(topics)
        ])
    }
}

#[test]
#[should_panic(expected = "cannot have `options_fn`")]
fn settings_pages_have_no_computed_options() {
    let _ = AdminProvider::new().settings::<Levels>();
}

db_test!(the_contract_of_computed_options, |db: TestDb| async move {
    let registry = registry(&db).await;
    let ctx = Ctx::new(db.conn().clone(), support::admin(), registry.clone());
    let entries = registry.get("option_entries").unwrap();
    let create = entries.schema(&ctx, SchemaOperation::Create, None).await.unwrap();
    let answer =
        ask(&ctx, json!({"operation": "create", "input": "topic", "state": {"category": "fruit"}})).await.unwrap();
    let failed = ask(&ctx, json!({"operation": "create", "input": "topic", "state": {"title": "boom"}})).await.unwrap();
    if db.backend() == rustcms_testing::Backend::Sqlite {
        support::contract("options-fn.schema.create.json", &serde_json::to_value(&create).unwrap());
        support::contract("options-fn.options.json", &serde_json::to_value(&answer).unwrap());
        support::contract("options-fn.failed.json", &serde_json::to_value(&failed).unwrap());
    }
});
