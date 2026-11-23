//! Filtered has-many relations (ADR 0044, part A): `where_eq` / `where_null`
//! restrict a relation wherever it is read (its section, counts, repeaters),
//! set their values on records created through it, and let several repeaters
//! edit one child resource through relations that cannot share a row.
//! SQLite, PostgreSQL, MySQL.

use axum::http::StatusCode;
use rustcms_admin::*;
use rustcms_auth::{ADMIN_GUARD, Actor, AuthProvider, AuthState, RbacService};
use rustcms_core::{AppBuilder, AppResult, Config};
use rustcms_testing::{Factory, TestApp, TestClient, TestDb, TestResponse, UserFactory, db_test};
use sea_orm::{ConnectionTrait, DatabaseTransaction, EntityTrait, QueryOrder, Schema};
use serde_json::{Value, json};

pub mod question {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "questions")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i32,
        pub title: String,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod option {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "question_options")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i32,
        pub question_id: i32,
        /// 1 (answer), 2 (distractor), or none.
        pub kind: Option<i32>,
        pub content: String,
        /// Shown on the option's form for answers only.
        pub note: Option<String>,
        #[sea_orm(column_type = "Decimal(Some((10, 2)))")]
        pub weight: Option<Decimal>,
        pub score: Option<f64>,
        pub extra: Option<Json>,
        pub flagged: Option<bool>,
        /// Set by repeaters; none for options created elsewhere.
        pub position: Option<i32>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

fn rows(name: &str) -> rustcms_admin::ui::Input {
    Repeater::make(name).relationship().order_column("position").schema(components![TextInput::make("content")])
}

/// Answer options (`kind` 1).
const ANSWER: i32 = 1;
/// Distractor options (`kind` 2).
const DISTRACTOR: i32 = 2;

pub struct QuestionResource;

impl Resource for QuestionResource {
    type Entity = question::Entity;
    const SLUG: &'static str = "questions";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Question").title("title")
    }

    fn fields() -> Vec<Field> {
        vec![Field::column(question::Column::Title).required()]
    }

    fn form() -> Form {
        // A question needs an answer (for users who see them, ADR 0044).
        Form::new().schema(components![TextInput::make("title"), rows("answers").min_items(1), rows("distractors")])
    }

    fn table() -> Table {
        Table::new().columns(vec![
            TextColumn::make("title"),
            TextColumn::make("answers_count").counts("answers"),
            TextColumn::make("loose_count").counts("loose"),
        ])
    }

    fn relations() -> Vec<Relation> {
        vec![
            Relation::has_many("answers", "question_options", "question_id").where_eq("kind", ANSWER),
            Relation::has_many("distractors", "question_options", "question_id").where_eq("kind", DISTRACTOR),
            Relation::has_many("loose", "question_options", "question_id").where_null("kind"),
            Relation::has_many("flagged", "question_options", "question_id").where_eq("flagged", true),
        ]
    }
}

/// Options: the panel's permissions; with [`PER_RECORD`] set (nextest runs
/// each test in its own process), options named "secret" visible to no one.
struct OptionPolicy;

static PER_RECORD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[async_trait::async_trait]
impl Policy for OptionPolicy {
    async fn allows(&self, ctx: &Ctx, ability: &str, record: Option<&Data>) -> bool {
        let secret = record.is_some_and(|r| r.get("content") == Some(&json!("secret")));
        ctx.can(&format!("question_options.{ability}"))
            && !(secret && PER_RECORD.load(std::sync::atomic::Ordering::SeqCst))
    }

    async fn admits(&self, ctx: &Ctx, ability: &str) -> bool {
        ctx.can(&format!("question_options.{ability}"))
    }

    fn view_depends_on_record(&self) -> bool {
        PER_RECORD.load(std::sync::atomic::Ordering::SeqCst)
    }
}

pub struct OptionResource;

#[async_trait::async_trait]
impl Resource for OptionResource {
    type Entity = option::Entity;
    const SLUG: &'static str = "question_options";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Option").title("content").lock_parent("question_id", "questions")
    }

    fn fields() -> Vec<Field> {
        use option::Column as C;
        vec![
            Field::column(C::QuestionId).required().belongs_to("questions"),
            // Read by users who may delete questions; written on create by super admins.
            Field::column(C::Kind)
                .readable(|ctx| ctx.principal.super_admin || ctx.can("questions.delete"))
                .creatable(|ctx| ctx.principal.super_admin),
            Field::column(C::Content).required(),
            Field::column(C::Note),
            Field::column(C::Weight),
            Field::column(C::Score),
            Field::column(C::Extra),
            Field::column(C::Flagged),
            Field::column(C::Position).guarded().readable(|_| true),
        ]
    }

    fn policy() -> std::sync::Arc<dyn Policy> {
        std::sync::Arc::new(OptionPolicy)
    }

    fn form() -> Form {
        Form::new().schema(components![
            SelectInput::make("question_id"),
            NumberInput::make("kind"),
            // Tells what it sees, then tries to move the option to the
            // distractors before writing the answer's note.
            TextInput::make("content").reacts(|_, state| {
                let seen = format!(
                    "kind: {}, question: {}",
                    state.get("kind").cloned().unwrap_or_default(),
                    state.get("question_id").cloned().unwrap_or_default()
                );
                Box::pin(async move {
                    Ok(vec![
                        FormPatch::notice(seen),
                        FormPatch::set("kind", DISTRACTOR),
                        FormPatch::set("note", "an answer's note"),
                    ])
                })
            }),
            TextInput::make("note").shown_when(when("kind").eq(ANSWER)),
        ])
    }

    // A hook may not move a row out of its relation: neither change nor drop
    // the values it sets (writing them again is fine).
    async fn mutate_before_create(_ctx: &Ctx, _txn: &DatabaseTransaction, mut data: Data) -> AppResult<Data> {
        match data.get("content").and_then(Value::as_str) {
            Some("drop") => {
                data.remove("kind");
            }
            Some("swap") => {
                data.insert("kind".into(), json!(DISTRACTOR));
            }
            Some("unflag") => {
                data.insert("flagged".into(), json!(false));
            }
            _ => {}
        }
        Ok(data)
    }

    async fn mutate_before_update(
        _ctx: &Ctx,
        _txn: &DatabaseTransaction,
        _model: &option::Model,
        mut data: Data,
    ) -> AppResult<Data> {
        match data.get("content").and_then(Value::as_str) {
            Some("flip") => {
                data.insert("kind".into(), json!(DISTRACTOR));
            }
            Some("clear") => {
                data.insert("kind".into(), Value::Null);
            }
            Some("again") => {
                data.insert("kind".into(), json!(ANSWER));
            }
            _ => {}
        }
        Ok(data)
    }
}

/// The same tables, a filtered field required by the child (ADR 0044): the
/// relation's value is in the input before `required` is checked.
pub struct TopicResource;

impl Resource for TopicResource {
    type Entity = question::Entity;
    const SLUG: &'static str = "topics";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Topic").title("title")
    }

    fn fields() -> Vec<Field> {
        vec![Field::column(question::Column::Title).required()]
    }

    fn form() -> Form {
        Form::new().schema(components![TextInput::make("title"), rows("answers")])
    }

    fn relations() -> Vec<Relation> {
        vec![Relation::has_many("answers", "topic_options", "question_id").where_eq("kind", ANSWER)]
    }
}

pub struct TopicOptionResource;

impl Resource for TopicOptionResource {
    type Entity = option::Entity;
    const SLUG: &'static str = "topic_options";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Topic option").title("content").lock_parent("question_id", "topics")
    }

    fn fields() -> Vec<Field> {
        use option::Column as C;
        vec![
            Field::column(C::QuestionId).required().belongs_to("topics"),
            Field::column(C::Kind).required(),
            Field::column(C::Content).required(),
            Field::column(C::Position).guarded().readable(|_| true),
        ]
    }
}

fn build(config: Config) -> AppBuilder {
    let mut app = AppBuilder::new(config);
    app.provider(AuthProvider::new());
    app.provider(
        AdminProvider::new()
            .resource::<QuestionResource>()
            .resource::<OptionResource>()
            .resource::<TopicResource>()
            .resource::<TopicOptionResource>(),
    );
    app
}

/// A user with every ability on questions and options but `questions.delete`
/// (`kind` unreadable), or with it (`kind` readable, not writable on create).
async fn editor(app: &TestApp, reads_kind: bool) -> rustcms_core::models::user::Model {
    let auth = app.state().service::<AuthState>().unwrap();
    let db = app.state().db();
    let rbac = RbacService::new(db, &auth.resolver);
    let mut permissions: Vec<String> = ["view_any", "view", "create", "update"]
        .iter()
        .flat_map(|a| [format!("questions.{a}"), format!("question_options.{a}")])
        .collect();
    permissions.push("question_options.delete".to_owned());
    if reads_kind {
        permissions.push("questions.delete".to_owned());
    }
    let name = if reads_kind { "writer" } else { "reader" };
    let user = UserFactory::new().name(name).create(db).await;
    let role = rbac.create_role(Actor::System, ADMIN_GUARD, name, &permissions).await.unwrap();
    rbac.assign_role(Actor::System, user.id, role.id).await.unwrap();
    user
}

async fn setup(db: TestDb) -> (TestApp, rustcms_core::models::user::Model) {
    db.migrate::<rustcms_migration::Migrator>().await;
    let schema = Schema::new(db.conn().get_database_backend());
    rustcms_testing::create_table(db.conn(), &schema.create_table_from_entity(question::Entity)).await;
    rustcms_testing::create_table(db.conn(), &schema.create_table_from_entity(option::Entity)).await;
    // Positions are unique per question, whatever the relation.
    db.conn()
        .execute_unprepared("CREATE UNIQUE INDEX question_options_position ON question_options (question_id, position)")
        .await
        .unwrap();
    let app = TestApp::new(build, db).await;
    let auth = app.state().service::<AuthState>().unwrap();
    let db = app.state().db();
    let rbac = RbacService::new(db, &auth.resolver);
    rbac.sync_permissions(&auth.permissions.all()).await.unwrap();
    let admin = UserFactory::new().name("Admin").create(db).await;
    let super_admin = rustcms_auth::entities::role::super_admin_id(db, ADMIN_GUARD).await.unwrap().unwrap();
    rbac.assign_role(Actor::System, admin.id, super_admin).await.unwrap();
    (app, admin)
}

fn issues(response: TestResponse) -> Value {
    response.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
    response.json()["issues"].clone()
}

async fn patch(client: &TestClient<'_>, app: &TestApp, question: &str, body: Value) -> TestResponse {
    client.fetch_csrf().await;
    client
        .send(
            axum::http::Method::PATCH,
            &app.admin_api(&format!("resources/questions/records/{question}")),
            Some(&body),
        )
        .await
}

/// Every option as `(kind, content, position)`, by position.
async fn stored(app: &TestApp) -> Vec<(Option<i32>, String, Option<i32>)> {
    option::Entity::find()
        .order_by_asc(option::Column::Position)
        .all(app.state().db())
        .await
        .unwrap()
        .into_iter()
        .map(|o| (o.kind, o.content, o.position))
        .collect()
}

fn new_rows(contents: &[&str]) -> Value {
    let create: Vec<Value> =
        contents.iter().enumerate().map(|(i, c)| json!({"ref": format!("n{i}"), "data": {"content": c}})).collect();
    json!({ "create": create })
}

/// A question with answers "a1", "a2", a distractor "d1" and an option of no kind.
async fn question(app: &TestApp, admin: &TestClient<'_>) -> String {
    let created = admin
        .post_json(
            &app.admin_api("resources/questions/records"),
            &json!({"title": "Q", "answers": new_rows(&["a1", "a2"]), "distractors": new_rows(&["d1"])}),
        )
        .await;
    created.assert_status(StatusCode::CREATED);
    let key = created.json()["data"]["key"].as_str().unwrap().to_owned();
    app.state()
        .db()
        .execute_unprepared(&format!(
            "INSERT INTO question_options (question_id, kind, content, position) VALUES ({key}, NULL, 'loose', 9)"
        ))
        .await
        .unwrap();
    key
}

async fn key_of(app: &TestApp, content: &str) -> String {
    let all = option::Entity::find().all(app.state().db()).await.unwrap();
    all.into_iter().find(|o| o.content == content).unwrap().id.to_string()
}

db_test!(repeaters_on_one_child_write_their_relations_rows, |db: TestDb| async move {
    let (app, user) = setup(db).await;
    let admin = app.acting_as(&user).await;
    question(&app, &admin).await;
    // Each relation sets its kind; positions follow every row of the question.
    assert_eq!(
        stored(&app).await,
        [
            (Some(ANSWER), "a1".to_owned(), Some(1)),
            (Some(ANSWER), "a2".to_owned(), Some(2)),
            (Some(DISTRACTOR), "d1".to_owned(), Some(3)),
            (None, "loose".to_owned(), Some(9)),
        ]
    );
    // The filtered field is the relation's, never the client's.
    let sent =
        json!({"title": "Q", "answers": {"create": [{"ref": "n1", "data": {"content": "x", "kind": DISTRACTOR}}]}});
    let refused = issues(admin.post_json(&app.admin_api("resources/questions/records"), &sent).await);
    assert_eq!(refused["answers.n1.kind"][0]["code"], json!("not_allowed"));
});

db_test!(a_relation_holds_only_its_filtered_rows, |db: TestDb| async move {
    let (app, user) = setup(db).await;
    let admin = app.acting_as(&user).await;
    let key = question(&app, &admin).await;
    let list = |relation: &str| app.admin_api(&format!("resources/questions/records/{key}/relations/{relation}"));
    let contents = |page: Value| -> Vec<String> {
        page["data"].as_array().unwrap().iter().map(|e| e["data"]["content"].as_str().unwrap().to_owned()).collect()
    };
    assert_eq!(contents(admin.get(&list("answers")).await.json()), ["a1", "a2"]);
    assert_eq!(contents(admin.get(&list("loose")).await.json()), ["loose"]);
    // Counts too.
    let page = admin.get(&app.admin_api("resources/questions/records")).await.json();
    assert_eq!(page["data"][0]["counts"], json!({"answers": 2, "loose": 1}));
    // Counts with a view policy deciding per record apply the filters too.
    PER_RECORD.store(true, std::sync::atomic::Ordering::SeqCst);
    app.state()
        .db()
        .execute_unprepared(&format!(
            "INSERT INTO question_options (question_id, kind, content, position) VALUES ({key}, {ANSWER}, 'secret', 10)"
        ))
        .await
        .unwrap();
    let page = admin.get(&app.admin_api("resources/questions/records")).await.json();
    assert_eq!(page["data"][0]["counts"], json!({"answers": 2, "loose": 1}));
    PER_RECORD.store(false, std::sync::atomic::Ordering::SeqCst);
    app.state().db().execute_unprepared("DELETE FROM question_options WHERE content = 'secret'").await.unwrap();
    // A row of another relation is not one of this one's.
    let d1 = key_of(&app, "d1").await;
    let found = issues(
        patch(&admin, &app, &key, json!({"answers": {"update": [{"key": d1, "data": {"content": "z"}}]}})).await,
    );
    assert_eq!(found[format!("answers.{d1}")][0]["code"], json!("not_a_row"));
    // Deleting through one relation leaves the others' rows.
    let a1 = key_of(&app, "a1").await;
    patch(&admin, &app, &key, json!({"answers": {"delete": [a1]}})).await.assert_status(StatusCode::OK);
    assert_eq!(stored(&app).await.len(), 3);
    // A hook may not move a row to another relation, nor out of every one.
    let a2 = key_of(&app, "a2").await;
    for content in ["flip", "clear"] {
        let sent = json!({"answers": {"update": [{"key": a2, "data": {"content": content}}]}});
        patch(&admin, &app, &key, sent).await.assert_status(StatusCode::INTERNAL_SERVER_ERROR);
    }
    // Writing the relation's value again keeps the row in it.
    let sent = json!({"answers": {"update": [{"key": a2, "data": {"content": "again"}}]}});
    patch(&admin, &app, &key, sent).await.assert_status(StatusCode::OK);
});

db_test!(a_hook_dropping_or_changing_a_created_rows_filter_fails, |db: TestDb| async move {
    let (app, user) = setup(db).await;
    let admin = app.acting_as(&user).await;
    let key = question(&app, &admin).await;
    let before = stored(&app).await;
    let section = |relation: &str| app.admin_api(&format!("resources/questions/records/{key}/relations/{relation}"));
    for (relation, content) in [("answers", "drop"), ("answers", "swap"), ("flagged", "unflag")] {
        let created = admin.post_json(&section(relation), &json!({"content": content})).await;
        created.assert_status(StatusCode::INTERNAL_SERVER_ERROR);
        let sent = json!({"answers": {"create": [{"ref": "n1", "data": {"content": content}}]}});
        if relation == "answers" {
            patch(&admin, &app, &key, sent).await.assert_status(StatusCode::INTERNAL_SERVER_ERROR);
        }
    }
    assert_eq!(stored(&app).await, before);
});

db_test!(a_filter_the_user_may_not_read_hides_the_relation, |db: TestDb| async move {
    let (app, user) = setup(db).await;
    let admin = app.acting_as(&user).await;
    let key = question(&app, &admin).await;
    let reader = editor(&app, false).await;
    let reader = app.acting_as(&reader).await;
    // Absent from the record's page: no section, no repeater, no entry.
    let view = reader.get(&app.admin_api(&format!("resources/questions/schema?operation=view&key={key}"))).await;
    let names: Vec<String> =
        view.json()["relations"].as_array().unwrap().iter().map(|r| r["name"].as_str().unwrap().to_owned()).collect();
    assert_eq!(names, ["flagged"]);
    let edit = reader.get(&app.admin_api(&format!("resources/questions/schema?operation=edit&key={key}"))).await;
    let inputs: Vec<String> = edit.json()["form"]["components"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["name"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(inputs, ["title"]);
    // Its routes answer 404, as for a relation that does not exist.
    let section = |relation: &str| app.admin_api(&format!("resources/questions/records/{key}/relations/{relation}"));
    for relation in ["answers", "loose"] {
        reader.get(&section(relation)).await.assert_status(StatusCode::NOT_FOUND);
        reader.post_json(&section(relation), &json!({"content": "x"})).await.assert_status(StatusCode::NOT_FOUND);
    }
    reader.get(&section("flagged")).await.assert_status(StatusCode::OK);
    // A change sent for its repeater: 403.
    let a1 = key_of(&app, "a1").await;
    for sent in [
        json!({"answers": {"update": [{"key": a1, "data": {"content": "z"}}]}}),
        json!({"answers": {"create": [{"ref": "n1", "data": {"content": "z"}}]}}),
    ] {
        patch(&reader, &app, &key, sent).await.assert_status(StatusCode::FORBIDDEN);
    }
    // No count column, no counts.
    let list = reader.get(&app.admin_api("resources/questions/records")).await.json();
    assert!(list["data"][0]["counts"].is_null(), "{list}");
    let schema = reader.get(&app.admin_api("resources/questions/schema")).await.json();
    let columns: Vec<&str> =
        schema["table"]["columns"].as_array().unwrap().iter().map(|c| c["name"].as_str().unwrap()).collect();
    assert_eq!(columns, ["title"]);
    // A repeater left out imposes no minimum: a question without answers.
    let created = reader.post_json(&app.admin_api("resources/questions/records"), &json!({"title": "Q2"})).await;
    created.assert_status(StatusCode::CREATED);
    // The super admin still needs one.
    let refused = issues(admin.post_json(&app.admin_api("resources/questions/records"), &json!({"title": "Q3"})).await);
    assert_eq!(refused["answers"][0]["code"], json!("min_items"));
});

db_test!(a_filter_the_user_may_not_write_offers_no_new, |db: TestDb| async move {
    let (app, user) = setup(db).await;
    let admin = app.acting_as(&user).await;
    let key = question(&app, &admin).await;
    let writer = editor(&app, true).await;
    let writer = app.acting_as(&writer).await;
    let view = writer.get(&app.admin_api(&format!("resources/questions/schema?operation=view&key={key}"))).await.json();
    let relation = |name: &str| view["relations"].as_array().unwrap().iter().find(|r| r["name"] == name).cloned();
    assert_eq!(relation("answers").unwrap()["create"], json!(false));
    assert_eq!(relation("flagged").unwrap()["create"], json!(true));
    let edit = writer.get(&app.admin_api(&format!("resources/questions/schema?operation=edit&key={key}"))).await.json();
    let answers = edit["form"]["components"].as_array().unwrap().iter().find(|c| c["name"] == "answers").cloned();
    let relationship = answers.unwrap()["repeater"]["relationship"].clone();
    assert_eq!((relationship["create"].clone(), relationship["update"].clone()), (json!(false), json!(true)));
    // Creates through the relation: 403; updates of its rows are fine.
    let section = app.admin_api(&format!("resources/questions/records/{key}/relations/answers"));
    writer.post_json(&section, &json!({"content": "x"})).await.assert_status(StatusCode::FORBIDDEN);
    let sent = json!({"answers": {"create": [{"ref": "n1", "data": {"content": "z"}}]}});
    patch(&writer, &app, &key, sent).await.assert_status(StatusCode::FORBIDDEN);
    let a1 = key_of(&app, "a1").await;
    let sent = json!({"answers": {"update": [{"key": a1, "data": {"content": "z"}}]}});
    patch(&writer, &app, &key, sent).await.assert_status(StatusCode::OK);
});

db_test!(a_required_filtered_field_gets_the_relations_value, |db: TestDb| async move {
    let (app, user) = setup(db).await;
    let admin = app.acting_as(&user).await;
    let sent = json!({"title": "T", "answers": new_rows(&["t1"])});
    let created = admin.post_json(&app.admin_api("resources/topics/records"), &sent).await;
    created.assert_status(StatusCode::CREATED);
    let key = created.json()["data"]["key"].as_str().unwrap().to_owned();
    let section = app.admin_api(&format!("resources/topics/records/{key}/relations/answers"));
    let row = admin.post_json(&section, &json!({"content": "t2"})).await;
    row.assert_status(StatusCode::CREATED);
    assert_eq!(row.json()["data"]["data"]["kind"], json!(ANSWER));
    let kinds: Vec<Option<i32>> = stored(&app).await.into_iter().map(|o| o.0).collect();
    assert_eq!(kinds, [Some(ANSWER), Some(ANSWER)]);
});

db_test!(creates_through_a_filtered_relation_get_its_values, |db: TestDb| async move {
    let (app, user) = setup(db).await;
    let admin = app.acting_as(&user).await;
    let key = question(&app, &admin).await;
    let create = |relation: &str| app.admin_api(&format!("resources/questions/records/{key}/relations/{relation}"));
    admin.post_json(&create("distractors"), &json!({"content": "d2"})).await.assert_status(StatusCode::CREATED);
    admin.post_json(&create("loose"), &json!({"content": "l2"})).await.assert_status(StatusCode::CREATED);
    let all = stored(&app).await;
    assert!(all.iter().any(|o| o.1 == "d2" && o.0 == Some(DISTRACTOR)));
    assert!(all.iter().any(|o| o.1 == "l2" && o.0.is_none()));
    let refused = issues(admin.post_json(&create("answers"), &json!({"content": "x", "kind": 3})).await);
    assert_eq!(refused["kind"][0]["code"], json!("set_by_relation"));
    // The child form's conditions read the relation's value, as the panel's form holds it.
    let noted = admin.post_json(&create("answers"), &json!({"content": "a9", "note": "kept"})).await;
    noted.assert_status(StatusCode::CREATED);
    assert_eq!(noted.json()["data"]["data"]["note"], json!("kept"));
    // The record's page sends what the relation sets, for its "New" form.
    let schema =
        admin.get(&app.admin_api(&format!("resources/questions/schema?operation=view&key={key}"))).await.json();
    let relations = schema["relations"].as_array().unwrap();
    let fixed = |name: &str| relations.iter().find(|r| r["name"] == name).unwrap()["fixed"].clone();
    assert_eq!((fixed("answers"), fixed("loose")), (json!({"kind": ANSWER}), json!({"kind": null})));
    assert_eq!(fixed("flagged"), json!({"flagged": true}));
});

macro_rules! refused_at_boot {
    ($test:ident, $resource:ident, $message:literal, $relations:expr, $form:expr) => {
        pub struct $resource;

        impl Resource for $resource {
            type Entity = question::Entity;
            const SLUG: &'static str = "questions";

            fn meta() -> ResourceMeta {
                ResourceMeta::new("Question")
            }

            fn fields() -> Vec<Field> {
                vec![Field::column(question::Column::Title).required()]
            }

            fn form() -> Form {
                Form::new().schema($form)
            }

            fn relations() -> Vec<Relation> {
                $relations
            }
        }

        #[test]
        #[should_panic(expected = $message)]
        fn $test() {
            Registry::new().register::<$resource>().register::<OptionResource>().finish();
        }
    };
}

refused_at_boot!(
    repeaters_whose_relations_may_share_rows,
    Overlapping,
    "may hold the same rows",
    vec![
        Relation::has_many("answers", "question_options", "question_id").where_eq("kind", ANSWER),
        Relation::has_many("all", "question_options", "question_id"),
    ],
    components![TextInput::make("title"), rows("answers"), rows("all")]
);
refused_at_boot!(
    one_value_twice,
    SameValue,
    "may hold the same rows",
    vec![
        Relation::has_many("answers", "question_options", "question_id").where_eq("kind", ANSWER),
        Relation::has_many("again", "question_options", "question_id").where_eq("kind", ANSWER),
    ],
    components![
        TextInput::make("title"),
        Repeater::make("answers").relationship().schema(components![TextInput::make("content")]),
        Repeater::make("again").relationship().schema(components![TextInput::make("content")])
    ]
);
// Compared exactly by the engine, by SQL with collations or rounding: refused.
refused_at_boot!(
    a_value_on_a_string,
    OnString,
    "`where_eq` takes a boolean, integer, UUID or date field, not a string",
    vec![Relation::has_many("answers", "question_options", "question_id").where_eq("content", "a")],
    components![TextInput::make("title")]
);
refused_at_boot!(
    a_value_on_a_decimal,
    OnDecimal,
    "`where_eq` takes a boolean, integer, UUID or date field, not a decimal",
    vec![Relation::has_many("answers", "question_options", "question_id").where_eq("weight", "0.5")],
    components![TextInput::make("title")]
);
refused_at_boot!(
    a_value_on_a_float,
    OnFloat,
    "`where_eq` takes a boolean, integer, UUID or date field, not a f64",
    vec![Relation::has_many("answers", "question_options", "question_id").where_eq("score", 1.5)],
    components![TextInput::make("title")]
);
refused_at_boot!(
    a_value_on_json,
    OnJson,
    "`where_eq` takes a boolean, integer, UUID or date field, not a json",
    vec![Relation::has_many("answers", "question_options", "question_id").where_eq("extra", json!({"a": 1}))],
    components![TextInput::make("title")]
);
refused_at_boot!(
    a_filter_on_an_unknown_field,
    UnknownField,
    "`question_options.colour`: is not a field",
    vec![Relation::has_many("answers", "question_options", "question_id").where_null("colour")],
    components![TextInput::make("title")]
);
refused_at_boot!(
    null_on_a_required_column,
    NotNullable,
    "`where_null` needs a nullable column",
    vec![Relation::has_many("answers", "question_options", "question_id").where_null("content")],
    components![TextInput::make("title")]
);
refused_at_boot!(
    a_value_of_another_kind,
    WrongKind,
    "is not a i32",
    vec![Relation::has_many("answers", "question_options", "question_id").where_eq("position", "first")],
    components![TextInput::make("title")]
);
refused_at_boot!(
    the_filtered_field_as_a_row_input,
    FilteredInput,
    "`kind`, which the relation filters, cannot be an input",
    vec![Relation::has_many("answers", "question_options", "question_id").where_eq("kind", ANSWER)],
    components![
        TextInput::make("title"),
        Repeater::make("answers")
            .relationship()
            .schema(components![TextInput::make("content"), TextInput::make("kind")])
    ]
);

/// A form reaction of the option's create form opened through `relation` of
/// question `key` (ADR 0043, 0044), its state forging the relation's values.
async fn react_via(client: &TestClient<'_>, app: &TestApp, key: &str, relation: Option<&str>) -> TestResponse {
    let mut body = json!({
        "operation": "create",
        "changed": "content",
        "state": {"content": "x", "kind": 3, "question_id": 0},
        "seq": 1,
    });
    if let Some(relation) = relation {
        body["via"] = json!({"parent": "questions", "record": key, "relation": relation});
    }
    client.post_json(&app.admin_api("resources/question_options/form/react"), &body).await
}

db_test!(a_reaction_through_a_filtered_relation_sees_and_keeps_its_values, |db: TestDb| async move {
    let (app, user) = setup(db).await;
    let admin = app.acting_as(&user).await;
    let key = question(&app, &admin).await;
    // The reaction sees the relation's values, not the forged ones; its patch
    // to a filtered field is dropped, so the answer's note stays shown.
    for (relation, kind) in [("answers", json!(ANSWER)), ("distractors", json!(DISTRACTOR)), ("loose", Value::Null)] {
        let answer = react_via(&admin, &app, &key, Some(relation)).await;
        answer.assert_status(StatusCode::OK);
        let mut expected = vec![json!({"op": "notice", "message": format!("kind: {kind}, question: {key}")})];
        if relation == "answers" {
            expected.push(json!({"op": "set", "field": "note", "value": "an answer's note"}));
        }
        assert_eq!(answer.json()["patches"], json!(expected), "{relation}");
    }
    // Outside a relation, the same patch is the super admin's to make (and
    // hides the note).
    let answer = react_via(&admin, &app, &key, None).await;
    answer.assert_status(StatusCode::OK);
    assert_eq!(
        answer.json()["patches"],
        json!([
            {"op": "notice", "message": "kind: 3, question: 0"},
            {"op": "set", "field": "kind", "value": DISTRACTOR},
        ])
    );
});

db_test!(a_reaction_through_a_filtered_relation_needs_its_create, |db: TestDb| async move {
    let (app, user) = setup(db).await;
    let admin = app.acting_as(&user).await;
    let key = question(&app, &admin).await;
    // A filter the user may not read: refused, as the relation's create is.
    let reader = editor(&app, false).await;
    let reader = app.acting_as(&reader).await;
    react_via(&reader, &app, &key, Some("answers")).await.assert_status(StatusCode::FORBIDDEN);
    // A filter the user may read but not write on create: 403.
    let writer = editor(&app, true).await;
    let writer = app.acting_as(&writer).await;
    react_via(&writer, &app, &key, Some("answers")).await.assert_status(StatusCode::FORBIDDEN);
    // A relation without filters on what they may write: answered.
    for client in [&reader, &writer] {
        react_via(client, &app, &key, Some("flagged")).await.assert_status(StatusCode::OK);
    }
});
