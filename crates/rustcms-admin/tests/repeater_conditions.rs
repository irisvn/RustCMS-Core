//! Conditions on and in repeaters (ADR 0044, part A): a write that hides a
//! relationship repeater removes the rows the user sees (only when it hides
//! it); rows follow the repeater's conditions, which may read the record's
//! form (`when_root`), as JSON rows may. SQLite, PostgreSQL, MySQL.

use axum::http::StatusCode;
use rustcms_admin::*;
use rustcms_auth::{ADMIN_GUARD, Actor, AuthProvider, AuthState, RbacService};
use rustcms_core::{AppBuilder, Config};
use rustcms_testing::{Factory, TestApp, TestClient, TestDb, TestResponse, UserFactory, db_test};
use sea_orm::{ConnectionTrait, EntityTrait, QueryFilter, QueryOrder, Schema};
use serde_json::{Value, json};

mod support;

pub mod quiz {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "quizzes")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i32,
        pub title: String,
        /// `choice` or `blank`.
        pub kind: String,
        #[sea_orm(default_value = false)]
        pub explain: bool,
        pub links: Option<Json>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod option {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "quiz_options")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i32,
        pub quiz_id: i32,
        pub content: String,
        #[sea_orm(default_value = false)]
        pub correct: bool,
        pub hint: Option<String>,
        pub position: i32,
        pub deleted_at: Option<DateTimeUtc>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub struct QuizResource;

impl Resource for QuizResource {
    type Entity = quiz::Entity;
    const SLUG: &'static str = "quizzes";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Quiz").plural("Quizzes").title("title")
    }

    fn fields() -> Vec<Field> {
        use quiz::Column as C;
        vec![
            Field::column(C::Title).required(),
            Field::column(C::Kind).required(),
            Field::column(C::Explain),
            Field::column(C::Links),
        ]
    }

    fn form() -> Form {
        Form::new().schema(components![
            TextInput::make("title"),
            SelectInput::make("kind").options(vec![("choice", "Choice"), ("blank", "Blank")]),
            Toggle::make("explain"),
            // A JSON repeater whose rows read the form.
            Repeater::make("links").schema(components![
                TextInput::make("label").required(),
                TextInput::make("note").shown_when(when_root("explain").is_true()),
            ]),
            // The options of a choice question only: switching to a blank removes them.
            Repeater::make("options")
                .relationship()
                .table()
                .order_column("position")
                .min_items(1)
                .shown_when(when("kind").eq("choice"))
                .schema(components![
                    TextInput::make("content"),
                    Toggle::make("correct"),
                    // Explained when the quiz explains and the option is correct.
                    TextInput::make("hint")
                        .shown_when(Condition::all([when("correct").is_true(), when_root("explain").is_true()])),
                ]),
        ])
    }

    fn relations() -> Vec<Relation> {
        vec![Relation::has_many("options", "quiz_options", "quiz_id")]
    }
}

pub struct OptionResource;

impl Resource for OptionResource {
    type Entity = option::Entity;
    const SLUG: &'static str = "quiz_options";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Option").title("content").soft_deletes().lock_parent("quiz_id", "quizzes")
    }

    fn fields() -> Vec<Field> {
        use option::Column as C;
        vec![
            Field::column(C::QuizId).required().belongs_to("quizzes"),
            Field::column(C::Content).required().max_length(50),
            Field::column(C::Correct),
            // Required where it shows.
            Field::column(C::Hint).required(),
            // Sortable: the view page lists the options in order.
            Field::column(C::Position).guarded().readable(|_| true).sortable(),
            Field::column(C::DeletedAt).guarded().readable(|_| true),
        ]
    }

    // The option's own form hides the hint unless the content is "x": rows
    // follow the repeater's conditions, never this one.
    fn form() -> Form {
        Form::new().schema(components![
            TextInput::make("content"),
            Toggle::make("correct"),
            TextInput::make("hint").shown_when(when("content").eq("x")),
        ])
    }

    // Options named "hidden" are outside everyone's scope.
    fn scope(query: sea_orm::Select<option::Entity>, _ctx: &Ctx) -> sea_orm::Select<option::Entity> {
        query.filter(sea_orm::ColumnTrait::ne(&option::Column::Content, "hidden"))
    }
}

fn build(config: Config) -> AppBuilder {
    let mut app = AppBuilder::new(config);
    app.provider(AuthProvider::new());
    app.provider(AdminProvider::new().resource::<QuizResource>().resource::<OptionResource>());
    app
}

struct Users {
    admin: rustcms_core::models::user::Model,
    /// Quizzes, and options without `delete`.
    clerk: rustcms_core::models::user::Model,
}

async fn setup(db: TestDb) -> (TestApp, Users) {
    db.migrate::<rustcms_migration::Migrator>().await;
    let schema = Schema::new(db.conn().get_database_backend());
    rustcms_testing::create_table(db.conn(), &schema.create_table_from_entity(quiz::Entity)).await;
    rustcms_testing::create_table(db.conn(), &schema.create_table_from_entity(option::Entity)).await;
    let app = TestApp::new(build, db).await;
    let auth = app.state().service::<AuthState>().unwrap();
    let db = app.state().db();
    let rbac = RbacService::new(db, &auth.resolver);
    rbac.sync_permissions(&auth.permissions.all()).await.unwrap();
    let admin = UserFactory::new().name("Admin").create(db).await;
    let super_admin = rustcms_auth::entities::role::super_admin_id(db, ADMIN_GUARD).await.unwrap().unwrap();
    rbac.assign_role(Actor::System, admin.id, super_admin).await.unwrap();
    let clerk = UserFactory::new().name("Clerk").create(db).await;
    let permissions: Vec<String> = ["quizzes.view_any", "quizzes.view", "quizzes.create", "quizzes.update"]
        .into_iter()
        .chain(["quiz_options.view_any", "quiz_options.view", "quiz_options.create", "quiz_options.update"])
        .map(str::to_owned)
        .collect();
    let role = rbac.create_role(Actor::System, ADMIN_GUARD, "clerk", &permissions).await.unwrap();
    rbac.assign_role(Actor::System, clerk.id, role.id).await.unwrap();
    (app, Users { admin, clerk })
}

async fn create_quiz(client: &TestClient<'_>, app: &TestApp, body: Value) -> TestResponse {
    client.post_json(&app.admin_api("resources/quizzes/records"), &body).await
}

async fn update_quiz(client: &TestClient<'_>, app: &TestApp, quiz: &str, body: Value) -> TestResponse {
    client.fetch_csrf().await;
    client
        .send(axum::http::Method::PATCH, &app.admin_api(&format!("resources/quizzes/records/{quiz}")), Some(&body))
        .await
}

fn issues(response: TestResponse) -> Value {
    response.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
    response.json()["issues"].clone()
}

/// The quiz's options not in the trash, as `(content, hint)` by position.
async fn options(app: &TestApp, quiz: &str) -> Vec<(String, Option<String>)> {
    option::Entity::find()
        .filter(sea_orm::ColumnTrait::eq(&option::Column::QuizId, quiz.parse::<i32>().unwrap()))
        .filter(sea_orm::ColumnTrait::is_null(&option::Column::DeletedAt))
        .order_by_asc(option::Column::Position)
        .all(app.state().db())
        .await
        .unwrap()
        .into_iter()
        .map(|o| (o.content, o.hint))
        .collect()
}

/// A choice quiz with options "a" and "b", and one outside the scope.
async fn choice_quiz(app: &TestApp, client: &TestClient<'_>) -> String {
    let created = create_quiz(
        client,
        app,
        json!({"title": "Q", "kind": "choice", "explain": false, "options": {
            "create": [
                {"ref": "n1", "data": {"content": "a", "correct": true}},
                {"ref": "n2", "data": {"content": "b", "correct": false}},
            ],
            "order": [{"new": "n1"}, {"new": "n2"}],
        }}),
    )
    .await;
    created.assert_status(StatusCode::CREATED);
    let quiz = created.json()["data"]["key"].as_str().unwrap().to_owned();
    insert_option(app, &quiz, "hidden", 3).await;
    quiz
}

async fn insert_option(app: &TestApp, quiz: &str, content: &str, position: i32) {
    app.state()
        .db()
        .execute_unprepared(&format!(
            "INSERT INTO quiz_options (quiz_id, content, correct, position) VALUES ({quiz}, '{content}', false, \
             {position})"
        ))
        .await
        .unwrap();
}

db_test!(a_write_hiding_the_repeater_removes_the_rows_the_user_sees, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    let admin = app.acting_as(&users.admin).await;
    let quiz = choice_quiz(&app, &admin).await;
    // Changes sent with the switch are dropped: the repeater is hidden after it.
    let sent = json!({"kind": "blank", "options": {"create": [{"ref": "n9", "data": {"content": "z"}}]}});
    update_quiz(&admin, &app, &quiz, sent).await.assert_status(StatusCode::OK);
    // The row outside the scope stays; the others are in the trash.
    assert_eq!(options(&app, &quiz).await, [("hidden".to_owned(), None)]);
    let trashed = option::Entity::find().all(app.state().db()).await.unwrap();
    assert_eq!(trashed.iter().filter(|o| o.deleted_at.is_some()).count(), 2);
    // The parent's history counts them; each row has its own entry.
    let history = admin.get(&app.admin_api(&format!("resources/quizzes/records/{quiz}/history"))).await.json();
    let rows = history["data"].as_array().unwrap().iter().find(|e| e["event"] == "rows_changed").unwrap().clone();
    assert_eq!((rows["meta"]["relation"].clone(), rows["meta"]["deleted"].clone()), (json!("options"), json!(2)));
});

db_test!(a_repeater_hidden_before_the_write_keeps_its_rows, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    let admin = app.acting_as(&users.admin).await;
    // A blank quiz holds no options it shows: none to create, its minimum not required.
    let created = create_quiz(
        &admin,
        &app,
        json!({"title": "Q", "kind": "blank", "options": {"create": [{"ref": "n1", "data": {"content": "a"}}]}}),
    )
    .await;
    created.assert_status(StatusCode::CREATED);
    let quiz = created.json()["data"]["key"].as_str().unwrap().to_owned();
    assert_eq!(options(&app, &quiz).await, []);
    // Rows added elsewhere (the relation section, an import) stay through other edits.
    insert_option(&app, &quiz, "kept", 1).await;
    update_quiz(&admin, &app, &quiz, json!({"title": "Renamed"})).await.assert_status(StatusCode::OK);
    update_quiz(&admin, &app, &quiz, json!({"kind": "blank"})).await.assert_status(StatusCode::OK);
    assert_eq!(options(&app, &quiz).await, [("kept".to_owned(), None)]);
    // A choice quiz still needs its minimum.
    let refused = issues(create_quiz(&admin, &app, json!({"title": "Q", "kind": "choice"})).await);
    assert_eq!(refused["options"][0]["code"], json!("min_items"));
});

db_test!(removing_the_rows_needs_the_childs_delete, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    let admin = app.acting_as(&users.admin).await;
    let clerk = app.acting_as(&users.clerk).await;
    let quiz = choice_quiz(&app, &admin).await;
    update_quiz(&clerk, &app, &quiz, json!({"kind": "blank"})).await.assert_status(StatusCode::FORBIDDEN);
    // Nothing saved: the quiz is still a choice, its options all there.
    let stored = quiz::Entity::find_by_id(quiz.parse::<i32>().unwrap()).one(app.state().db()).await.unwrap().unwrap();
    assert_eq!(stored.kind, "choice");
    assert_eq!(options(&app, &quiz).await.len(), 3);
});

db_test!(rows_follow_the_repeaters_conditions_reading_the_form, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    let admin = app.acting_as(&users.admin).await;
    let row = |reference: &str, content: &str, correct: bool, hint: Option<&str>| json!({"ref": reference, "data": {"content": content, "correct": correct, "hint": hint}});
    // The quiz explains: a correct option's hint shows, and is required there.
    let refused = issues(
        create_quiz(
            &admin,
            &app,
            json!({"title": "Q", "kind": "choice", "explain": true, "options": {"create": [row("n1", "a", true, None)]}}),
        )
        .await,
    );
    assert_eq!(refused["options.n1.hint"][0]["code"], json!("required"));
    // A wrong option's hint is hidden (dropped, not required); the child form's
    // own condition (content "x") does not apply to rows.
    let created = create_quiz(
        &admin,
        &app,
        json!({"title": "Q", "kind": "choice", "explain": true, "options": {
            "create": [row("n1", "a", true, Some("because")), row("n2", "b", false, Some("dropped"))],
            "order": [{"new": "n1"}, {"new": "n2"}],
        }}),
    )
    .await;
    created.assert_status(StatusCode::CREATED);
    let quiz = created.json()["data"]["key"].as_str().unwrap().to_owned();
    assert_eq!(options(&app, &quiz).await, [("a".to_owned(), Some("because".to_owned())), ("b".to_owned(), None)]);
    // The form stops explaining in the same write: the row's hint is hidden.
    let a = option::Entity::find()
        .filter(sea_orm::ColumnTrait::eq(&option::Column::Content, "a"))
        .one(app.state().db())
        .await
        .unwrap()
        .unwrap()
        .id
        .to_string();
    let sent = json!({"explain": false, "options": {"update": [{"key": a, "data": {"hint": "changed"}}]}});
    update_quiz(&admin, &app, &quiz, sent).await.assert_status(StatusCode::OK);
    assert_eq!(options(&app, &quiz).await[0], ("a".to_owned(), Some("because".to_owned())));
});

db_test!(json_rows_read_the_form, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    let admin = app.acting_as(&users.admin).await;
    let links = json!([{"label": "Docs", "note": "read first"}]);
    let base = json!({"title": "Q", "kind": "blank", "links": links});
    // Not explaining: the note is hidden, so dropped.
    let created = create_quiz(&admin, &app, base.clone()).await;
    created.assert_status(StatusCode::CREATED);
    assert_eq!(created.json()["data"]["data"]["links"], json!([{"label": "Docs"}]));
    let quiz = created.json()["data"]["key"].as_str().unwrap().to_owned();
    // On update the stored form decides with the input over it.
    update_quiz(&admin, &app, &quiz, json!({"explain": true})).await.assert_status(StatusCode::OK);
    let updated = update_quiz(&admin, &app, &quiz, json!({"links": links})).await;
    updated.assert_status(StatusCode::OK);
    assert_eq!(updated.json()["data"]["data"]["links"], links);
    // Malformed values are refused even where hidden.
    let bad = json!({"explain": false, "links": [{"label": "Docs", "note": 3}]});
    assert!(issues(update_quiz(&admin, &app, &quiz, bad).await)["links.0.note"].is_array());
});

db_test!(schemas_send_row_conditions_with_their_scope, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    let admin = app.acting_as(&users.admin).await;
    let schema = admin.get(&app.admin_api("resources/quizzes/schema?operation=create")).await.json();
    let components = schema["form"]["components"].as_array().unwrap();
    let input = |name: &str| components.iter().find(|c| c["name"] == name).unwrap().clone();
    let explained = json!({"op": "is_true", "name": "explain", "scope": "root"});
    assert_eq!(input("links")["repeater"]["components"][1]["shownWhen"], explained);
    let options = input("options");
    assert_eq!(options["shownWhen"], json!({"op": "eq", "name": "kind", "value": "choice"}));
    assert_eq!(options["repeater"]["layout"], json!("table"));
    let hint = options["repeater"]["components"][2]["shownWhen"].clone();
    assert_eq!(hint, json!({"op": "all", "conditions": [{"op": "is_true", "name": "correct"}, explained]}));
});

db_test!(view_pages_show_a_relations_rows, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    let admin = app.acting_as(&users.admin).await;
    let quiz = choice_quiz(&app, &admin).await;
    // Made from the form: the options' repeater becomes an entry of the relation's rows.
    let schema = admin.get(&app.admin_api(&format!("resources/quizzes/schema?operation=view&key={quiz}"))).await.json();
    if app.state().db().get_database_backend() == sea_orm::DatabaseBackend::Sqlite {
        // Shared with the SPA tests: the view page, and the edit form (ADR 0044).
        support::contract("repeater-conditions.schema.view.json", &schema);
        let edit = admin.get(&app.admin_api(&format!("resources/quizzes/schema?operation=edit&key={quiz}"))).await;
        support::contract("repeater-conditions.schema.edit.json", &edit.json());
    }
    let entries = schema["infolist"]["components"].as_array().unwrap();
    let options = entries.iter().find(|e| e["name"] == "options").unwrap();
    assert_eq!(options["type"], json!("repeatable_entry"));
    assert_eq!(
        options["rows"],
        json!({"resource": "quiz_options", "relation": "options", "orderBy": "position", "limit": 100})
    );
    assert_eq!(options["shownWhen"], json!({"op": "eq", "name": "kind", "value": "choice"}));
    let names: Vec<&str> =
        options["components"].as_array().unwrap().iter().map(|c| c["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["content", "correct", "hint"]);
    // A user who may not list options does not get the entry.
    let reader = UserFactory::new().name("Reader").create(app.state().db()).await;
    let auth = app.state().service::<AuthState>().unwrap();
    let rbac = RbacService::new(app.state().db(), &auth.resolver);
    let role = rbac
        .create_role(Actor::System, ADMIN_GUARD, "reader", &["quizzes.view_any".to_owned(), "quizzes.view".to_owned()])
        .await
        .unwrap();
    rbac.assign_role(Actor::System, reader.id, role.id).await.unwrap();
    let reader = app.acting_as(&reader).await;
    let schema =
        reader.get(&app.admin_api(&format!("resources/quizzes/schema?operation=view&key={quiz}"))).await.json();
    let entries = schema["infolist"]["components"].as_array().unwrap();
    assert!(!entries.iter().any(|e| e["name"] == "options"));
});

macro_rules! entry_refused_at_boot {
    ($test:ident, $resource:ident, $message:literal, $entry:expr) => {
        pub struct $resource;

        impl Resource for $resource {
            type Entity = quiz::Entity;
            const SLUG: &'static str = "quizzes";

            fn meta() -> ResourceMeta {
                ResourceMeta::new("Quiz")
            }

            fn fields() -> Vec<Field> {
                vec![Field::column(quiz::Column::Title).required()]
            }

            fn infolist() -> Infolist {
                Infolist::new().schema(components![TextEntry::make("title"), $entry])
            }

            fn relations() -> Vec<Relation> {
                vec![Relation::has_many("options", "quiz_options", "quiz_id")]
            }
        }

        #[test]
        #[should_panic(expected = $message)]
        fn $test() {
            Registry::new().register::<$resource>().register::<OptionResource>().finish();
        }
    };
}

entry_refused_at_boot!(
    rows_of_no_relation,
    NoRelation,
    "`relationship()` needs a has-many relation named `answers`",
    RepeatableEntry::make("answers").relationship().schema(components![TextEntry::make("content")])
);
entry_refused_at_boot!(
    a_row_entry_of_no_child_field,
    NoChildField,
    "entry `colour` is not a field of the related resource",
    RepeatableEntry::make("options").relationship().schema(components![TextEntry::make("colour")])
);
entry_refused_at_boot!(
    a_row_entry_that_does_not_fit,
    WrongEntry,
    "entry `content`: a BooleanEntry (boolean columns) does not fit this field",
    RepeatableEntry::make("options").relationship().schema(components![BooleanEntry::make("content")])
);
entry_refused_at_boot!(
    too_many_rows_shown,
    TooManyRows,
    "`limit` shows at most 100 rows",
    RepeatableEntry::make("options").relationship().limit(500).schema(components![TextEntry::make("content")])
);
