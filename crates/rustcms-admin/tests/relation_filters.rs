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
        /// `answer`, `distractor`, or none.
        pub kind: Option<String>,
        pub content: String,
        /// Shown on the option's form for answers only.
        pub note: Option<String>,
        #[sea_orm(column_type = "Decimal(Some((10, 2)))")]
        pub weight: Option<Decimal>,
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
        Form::new().schema(components![TextInput::make("title"), rows("answers"), rows("distractors")])
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
            Relation::has_many("answers", "question_options", "question_id").where_eq("kind", "answer"),
            Relation::has_many("distractors", "question_options", "question_id").where_eq("kind", "distractor"),
            Relation::has_many("loose", "question_options", "question_id").where_null("kind"),
            // A value written without the column's scale: sent canonical.
            Relation::has_many("halves", "question_options", "question_id").where_eq("weight", "0.5"),
        ]
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
            Field::column(C::Kind),
            Field::column(C::Content).required(),
            Field::column(C::Note),
            Field::column(C::Weight),
            Field::column(C::Position).guarded().readable(|_| true),
        ]
    }

    fn form() -> Form {
        Form::new().schema(components![
            SelectInput::make("question_id"),
            TextInput::make("kind"),
            TextInput::make("content"),
            TextInput::make("note").shown_when(when("kind").eq("answer")),
        ])
    }

    // A hook may not move a row out of its relation.
    async fn mutate_before_update(
        _ctx: &Ctx,
        _txn: &DatabaseTransaction,
        _model: &option::Model,
        mut data: Data,
    ) -> AppResult<Data> {
        if data.get("content") == Some(&json!("flip")) {
            data.insert("kind".into(), json!("distractor"));
        }
        Ok(data)
    }
}

fn build(config: Config) -> AppBuilder {
    let mut app = AppBuilder::new(config);
    app.provider(AuthProvider::new());
    app.provider(AdminProvider::new().resource::<QuestionResource>().resource::<OptionResource>());
    app
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
async fn stored(app: &TestApp) -> Vec<(Option<String>, String, Option<i32>)> {
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
            (Some("answer".to_owned()), "a1".to_owned(), Some(1)),
            (Some("answer".to_owned()), "a2".to_owned(), Some(2)),
            (Some("distractor".to_owned()), "d1".to_owned(), Some(3)),
            (None, "loose".to_owned(), Some(9)),
        ]
    );
    // The filtered field is the relation's, never the client's.
    let sent =
        json!({"title": "Q", "answers": {"create": [{"ref": "n1", "data": {"content": "x", "kind": "distractor"}}]}});
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
    // A hook may not move a row to another relation.
    let a2 = key_of(&app, "a2").await;
    let moved =
        patch(&admin, &app, &key, json!({"answers": {"update": [{"key": a2, "data": {"content": "flip"}}]}})).await;
    moved.assert_status(StatusCode::INTERNAL_SERVER_ERROR);
});

db_test!(creates_through_a_filtered_relation_get_its_values, |db: TestDb| async move {
    let (app, user) = setup(db).await;
    let admin = app.acting_as(&user).await;
    let key = question(&app, &admin).await;
    let create = |relation: &str| app.admin_api(&format!("resources/questions/records/{key}/relations/{relation}"));
    admin.post_json(&create("distractors"), &json!({"content": "d2"})).await.assert_status(StatusCode::CREATED);
    admin.post_json(&create("loose"), &json!({"content": "l2"})).await.assert_status(StatusCode::CREATED);
    let all = stored(&app).await;
    assert!(all.iter().any(|o| o.1 == "d2" && o.0.as_deref() == Some("distractor")));
    assert!(all.iter().any(|o| o.1 == "l2" && o.0.is_none()));
    let refused = issues(admin.post_json(&create("answers"), &json!({"content": "x", "kind": "loose"})).await);
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
    assert_eq!((fixed("answers"), fixed("loose")), (json!({"kind": "answer"}), json!({"kind": null})));
    assert_eq!(fixed("halves"), json!({"weight": "0.50"}));
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
        Relation::has_many("answers", "question_options", "question_id").where_eq("kind", "answer"),
        Relation::has_many("all", "question_options", "question_id"),
    ],
    components![TextInput::make("title"), rows("answers"), rows("all")]
);
refused_at_boot!(
    one_value_written_two_ways,
    SameValue,
    "may hold the same rows",
    vec![
        Relation::has_many("answers", "question_options", "question_id").where_eq("weight", "1.5"),
        Relation::has_many("sevens", "question_options", "question_id").where_eq("weight", "1.50"),
    ],
    components![
        TextInput::make("title"),
        Repeater::make("answers").relationship().schema(components![TextInput::make("content")]),
        Repeater::make("sevens").relationship().schema(components![TextInput::make("content")])
    ]
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
    vec![Relation::has_many("answers", "question_options", "question_id").where_eq("kind", "answer")],
    components![
        TextInput::make("title"),
        Repeater::make("answers")
            .relationship()
            .schema(components![TextInput::make("content"), TextInput::make("kind")])
    ]
);
