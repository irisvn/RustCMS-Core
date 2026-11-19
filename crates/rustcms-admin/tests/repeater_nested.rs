//! Nested relationship repeaters (ADR 0044, part C): a question's blanks,
//! each with its choices, written in one save. Choices are options that also
//! name their question (`inherit`), and lock it, as the question's own
//! options do. SQLite, PostgreSQL, MySQL.

use std::sync::Mutex;

use axum::http::{Method, StatusCode};
use rustcms_admin::*;
use rustcms_auth::{ADMIN_GUARD, Actor, AuthProvider, AuthState, RbacService};
use rustcms_core::{AppBuilder, AppResult, Config};
use rustcms_testing::{Factory, TestApp, TestClient, TestDb, TestResponse, UserFactory, db_test};
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseTransaction, EntityTrait, QueryFilter, QueryOrder, Schema, Set};
use serde_json::{Value, json};

mod support;

pub mod question {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "questions")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i32,
        pub title: String,
        /// `choice` or `blank`.
        pub kind: String,
        /// A JSON repeater's rows: tags, distinct (`validate_rows`).
        pub tags: Option<Json>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod blank {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "question_blanks")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i32,
        pub question_id: i32,
        pub label: String,
        /// `select`, `text` or `open` (no choices).
        pub mode: String,
        pub position: i32,
        /// A JSON repeater's rows: hints.
        pub hints: Option<Json>,
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
        pub blank_id: Option<i32>,
        pub content: String,
        pub note: Option<String>,
        pub position: i32,
        /// A file, checked with the question's (ADR 0044, part B), at any level.
        pub image: Option<Uuid>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// The `after_save` hooks run, in order (`slug:key`).
static SAVED: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn saved(slug: &str, key: i32) {
    SAVED.lock().unwrap().push(format!("{slug}:{key}"));
}

pub struct QuestionResource;

#[async_trait::async_trait]
impl Resource for QuestionResource {
    type Entity = question::Entity;
    const SLUG: &'static str = "questions";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Question").title("title")
    }

    fn fields() -> Vec<Field> {
        use question::Column as C;
        vec![Field::column(C::Title).required(), Field::column(C::Kind).required(), Field::column(C::Tags)]
    }

    fn form() -> Form {
        Form::new().schema(components![
            // Reactions on relationship rows (ADR 0044, part D): the title says what to patch.
            TextInput::make("title").reacts(|_ctx, state| {
                let rows = state.relation_rows("blanks").to_vec();
                let first = rows.first().map(|r| r.id().to_owned()).unwrap_or_default();
                let patches = match state.text("title") {
                    "add" => vec![FormPatch::add_relation_row("blanks", json!({"label": "auto", "mode": "select"}))],
                    "add then name" => vec![
                        FormPatch::add_relation_row_as("blanks", "x", json!({"label": "a"})),
                        FormPatch::update_relation_row("blanks", "x", json!({"label": "b"})),
                    ],
                    "not yours" => vec![
                        FormPatch::add_relation_row("blanks", json!({"position": 1})),
                        FormPatch::add_relation_row("blanks", json!({"question_id": 1})),
                        FormPatch::add_relation_row("blanks", json!({"mode": "zzz"})),
                        FormPatch::update_relation_row("blanks", "999", json!({"label": "x"})),
                        FormPatch::add_relation_row("options", json!({"content": "hidden repeater"})),
                    ],
                    "rename" => rows
                        .iter()
                        .map(|r| {
                            FormPatch::update_relation_row(
                                "blanks",
                                r.id(),
                                json!({"label": format!("{}!", r.text("label"))}),
                            )
                        })
                        .collect(),
                    "same" => rows
                        .iter()
                        .map(|r| FormPatch::update_relation_row("blanks", r.id(), json!({"label": r.text("label")})))
                        .collect(),
                    "remove" => vec![FormPatch::remove_relation_row("blanks", first)],
                    "flag" => vec![FormPatch::flag_relation_row("blanks", first, "check this")],
                    _ => vec![],
                };
                Box::pin(async move { Ok(patches) })
            }),
            SelectInput::make("kind").options(vec![("choice", "Choice"), ("blank", "Blank")]),
            // Tags are distinct (a rule over a JSON repeater's rows).
            Repeater::make("tags").schema(components![TextInput::make("name").required()]).validate_rows(
                |_ctx, check| {
                    let mut seen = std::collections::HashSet::new();
                    for row in check.rows() {
                        if !seen.insert(row.get("name").cloned()) {
                            return Err(RowsError::input(row.id(), "name", "is a duplicate"));
                        }
                    }
                    Ok(())
                }
            ),
            // A question titled "two" has exactly two options (every live one,
            // also those the user cannot see); an option is never "bad".
            Repeater::make("options")
                .relationship()
                .order_column("position")
                .shown_when(when("kind").eq("choice"))
                .schema(components![TextInput::make("content")])
                .validate_rows(|_ctx, check| {
                    let mut refused: Option<RowsError> = None;
                    for row in check.rows() {
                        if row.get("content") == Some(&json!("bad")) {
                            let e = RowsError::input(row.id(), "content", "is bad");
                            refused = Some(match refused.take() {
                                Some(r) => r.and(e),
                                None => e,
                            });
                        }
                    }
                    if check.parent().get("title") == Some(&json!("two")) && check.rows().len() != 2 {
                        let e = RowsError::list("must have exactly two options");
                        refused = Some(match refused.take() {
                            Some(r) => r.and(e),
                            None => e,
                        });
                    }
                    refused.map_or(Ok(()), Err)
                }),
            Repeater::make("blanks")
                .relationship()
                .order_column("position")
                .shown_when(when("kind").eq("blank"))
                // Reacts to its rows: a row without a label gets one.
                .reacts(|_ctx, state| {
                    let patches = state
                        .relation_rows("blanks")
                        .iter()
                        .enumerate()
                        .filter(|(_, r)| r.text("label").is_empty())
                        .map(|(i, r)| {
                            FormPatch::update_relation_row(
                                "blanks",
                                r.id(),
                                json!({"label": format!("Blank {}", i + 1)}),
                            )
                        })
                        .collect();
                    Box::pin(async move { Ok(patches) })
                })
                .schema(components![
                    TextInput::make("label"),
                    SelectInput::make("mode"),
                    // A JSON repeater in a relationship row.
                    Repeater::make("hints").schema(components![TextInput::make("text").required().max_length(20)]),
                    // A blank's choices: options naming their question too.
                    Repeater::make("choices")
                        .relationship()
                        .inherit("question_id")
                        .order_column("position")
                        .shown_when(when("mode").ne("open"))
                        // A blank's choices are distinct (checked per blank).
                        .validate_rows(|_ctx, check| {
                            let mut seen = std::collections::HashSet::new();
                            match check.rows().iter().find(|r| !seen.insert(r.get("content").cloned())) {
                                Some(_) => Err(RowsError::list("must be distinct")),
                                None => Ok(()),
                            }
                        })
                        .schema(components![
                            TextInput::make("content"),
                            // Explained only for a select blank (the enclosing row).
                            TextInput::make("note").shown_when(when_parent("mode").eq("select")),
                            FileUpload::make("image"),
                        ]),
                ]),
        ])
    }

    fn relations() -> Vec<Relation> {
        vec![
            Relation::has_many("options", "question_options", "question_id").where_null("blank_id"),
            Relation::has_many("blanks", "question_blanks", "question_id"),
        ]
    }

    async fn after_save(_ctx: &Ctx, _txn: &DatabaseTransaction, record: &question::Model) -> AppResult<()> {
        saved(Self::SLUG, record.id);
        Ok(())
    }

    // Tag names are stored in lower case: the rule over the tags sees them so.
    async fn mutate_before_create(_ctx: &Ctx, _txn: &DatabaseTransaction, mut data: Data) -> AppResult<Data> {
        if let Some(Value::Array(tags)) = data.get_mut("tags") {
            for tag in tags.iter_mut() {
                if let Some(name) = tag.get("name").and_then(Value::as_str).map(str::to_lowercase) {
                    tag["name"] = json!(name);
                }
            }
        }
        Ok(data)
    }
}

pub struct BlankResource;

#[async_trait::async_trait]
impl Resource for BlankResource {
    type Entity = blank::Entity;
    const SLUG: &'static str = "question_blanks";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Blank").title("label").lock_parent("question_id", "questions")
    }

    fn fields() -> Vec<Field> {
        use blank::Column as C;
        vec![
            Field::column(C::QuestionId).required().belongs_to("questions"),
            Field::column(C::Label).required(),
            Field::column(C::Mode).required(),
            Field::column(C::Position).guarded().readable(|_| true).sortable(),
            Field::column(C::Hints),
        ]
    }

    fn form() -> Form {
        Form::new().schema(components![
            TextInput::make("label"),
            SelectInput::make("mode").options(vec![("select", "Select"), ("text", "Text"), ("open", "Open")]),
        ])
    }

    fn relations() -> Vec<Relation> {
        vec![Relation::has_many("choices", "question_options", "blank_id")]
    }

    async fn after_save(_ctx: &Ctx, _txn: &DatabaseTransaction, record: &blank::Model) -> AppResult<()> {
        saved(Self::SLUG, record.id);
        Ok(())
    }
}

pub struct OptionResource;

#[async_trait::async_trait]
impl Resource for OptionResource {
    type Entity = option::Entity;
    const SLUG: &'static str = "question_options";

    fn meta() -> ResourceMeta {
        // Choices and options alike lock their question.
        ResourceMeta::new("Option").title("content").lock_parent("question_id", "questions")
    }

    fn fields() -> Vec<Field> {
        use option::Column as C;
        vec![
            Field::column(C::QuestionId).required().belongs_to("questions"),
            Field::column(C::BlankId).belongs_to("question_blanks"),
            Field::column(C::Content).required().max_length(50),
            Field::column(C::Note),
            Field::column(C::Position).guarded().readable(|_| true).sortable(),
            Field::column(C::Image).media().accept(&["image/*"]),
        ]
    }

    fn form() -> Form {
        Form::new().schema(components![TextInput::make("content"), TextInput::make("note")])
    }

    // Options named "hidden" are outside everyone's scope.
    fn scope(query: sea_orm::Select<option::Entity>, _ctx: &Ctx) -> sea_orm::Select<option::Entity> {
        query.filter(option::Column::Content.ne("hidden"))
    }

    async fn after_save(_ctx: &Ctx, _txn: &DatabaseTransaction, record: &option::Model) -> AppResult<()> {
        saved(Self::SLUG, record.id);
        Ok(())
    }
}

fn build(config: Config) -> AppBuilder {
    // One directory for every run: these tests store no file content (media rows only).
    let root = std::env::temp_dir().join("rustcms-repeater-nested");
    let config = config.with("storage.root", root.to_string_lossy()).expect("storage config");
    let mut app = AppBuilder::new(config);
    app.provider(rustcms_core::storage::StorageProvider);
    app.provider(AuthProvider::new());
    app.provider(
        AdminProvider::new()
            .media()
            .resource::<QuestionResource>()
            .resource::<BlankResource>()
            .resource::<OptionResource>(),
    );
    app
}

/// A file of the media library (`mime`), trashed or not.
async fn stored_media(app: &TestApp, owner: i64, mime: &str, trashed: bool) -> String {
    let id = uuid::Uuid::now_v7();
    rustcms_admin::media::entity::ActiveModel {
        id: Set(id),
        disk: Set("local".into()),
        key: Set(format!("media/{id}")),
        name: Set("f".into()),
        mime: Set(mime.into()),
        size: Set(10),
        sha256: Set("0".repeat(64)),
        width: Set(None),
        height: Set(None),
        alt: Set(None),
        collection: Set(None),
        owner_id: Set(owner),
        created_at: Set(chrono::Utc::now()),
        deleted_at: Set(trashed.then(chrono::Utc::now)),
    }
    .insert(app.state().db())
    .await
    .unwrap();
    id.to_string()
}

struct Users {
    admin: rustcms_core::models::user::Model,
    /// Questions and blanks, options without `create`.
    clerk: rustcms_core::models::user::Model,
}

async fn setup(db: TestDb) -> (TestApp, Users) {
    db.migrate::<rustcms_migration::Migrator>().await;
    let schema = Schema::new(db.conn().get_database_backend());
    rustcms_testing::create_table(db.conn(), &schema.create_table_from_entity(question::Entity)).await;
    rustcms_testing::create_table(db.conn(), &schema.create_table_from_entity(blank::Entity)).await;
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
    let permissions: Vec<String> = ["questions", "question_blanks"]
        .into_iter()
        .flat_map(|slug| ["view_any", "view", "create", "update", "delete"].map(|a| format!("{slug}.{a}")))
        .chain(["question_options.view_any", "question_options.view", "question_options.update"].map(str::to_owned))
        .collect();
    let role = rbac.create_role(Actor::System, ADMIN_GUARD, "clerk", &permissions).await.unwrap();
    rbac.assign_role(Actor::System, clerk.id, role.id).await.unwrap();
    (app, Users { admin, clerk })
}

async fn create(client: &TestClient<'_>, app: &TestApp, body: Value) -> TestResponse {
    client.post_json(&app.admin_api("resources/questions/records"), &body).await
}

async fn update(client: &TestClient<'_>, app: &TestApp, question: i32, body: Value) -> TestResponse {
    client.fetch_csrf().await;
    client.send(Method::PATCH, &app.admin_api(&format!("resources/questions/records/{question}")), Some(&body)).await
}

async fn blanks_of(app: &TestApp, question: i32) -> Vec<blank::Model> {
    blank::Entity::find()
        .filter(blank::Column::QuestionId.eq(question))
        .order_by_asc(blank::Column::Position)
        .all(app.state().db())
        .await
        .unwrap()
}

async fn choices_of(app: &TestApp, blank: i32) -> Vec<option::Model> {
    option::Entity::find()
        .filter(option::Column::BlankId.eq(blank))
        .order_by_asc(option::Column::Position)
        .all(app.state().db())
        .await
        .unwrap()
}

/// A blank question with blanks (label, mode) and their choices, stored directly.
async fn stored(app: &TestApp, blanks: &[(&str, &str, &[&str])]) -> (i32, Vec<(i32, Vec<i32>)>) {
    let db = app.state().db();
    let question = question::ActiveModel { title: Set("Q".into()), kind: Set("blank".into()), ..Default::default() };
    let question = question.insert(db).await.unwrap().id;
    let mut out = Vec::new();
    for (position, (label, mode, choices)) in blanks.iter().enumerate() {
        let blank = blank::ActiveModel {
            question_id: Set(question),
            label: Set((*label).into()),
            mode: Set((*mode).into()),
            position: Set(position as i32 + 1),
            ..Default::default()
        };
        let blank = blank.insert(db).await.unwrap().id;
        let mut keys = Vec::new();
        for (position, content) in choices.iter().enumerate() {
            let choice = option::ActiveModel {
                question_id: Set(question),
                blank_id: Set(Some(blank)),
                content: Set((*content).into()),
                position: Set(position as i32 + 1),
                ..Default::default()
            };
            keys.push(choice.insert(db).await.unwrap().id);
        }
        out.push((blank, keys));
    }
    (question, out)
}

db_test!(a_question_its_blanks_and_their_choices_are_created_in_one_save, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    let admin = app.acting_as(&users.admin).await;
    SAVED.lock().unwrap().clear();
    let created = create(
        &admin,
        &app,
        json!({
            "title": "Fill in",
            "kind": "blank",
            "blanks": {"create": [{"ref": "n1", "data": {
                "label": "First",
                "mode": "select",
                "hints": [{"text": "think"}],
                "choices": {"create": [
                    {"ref": "n2", "data": {"content": "yes", "note": "because"}},
                    {"ref": "n3", "data": {"content": "no"}},
                ]},
            }}]},
        }),
    )
    .await;
    created.assert_status(StatusCode::CREATED);
    let question: i32 = created.json()["data"]["key"].as_str().unwrap().parse().unwrap();
    let blanks = blanks_of(&app, question).await;
    assert_eq!(blanks.len(), 1);
    assert_eq!((blanks[0].label.as_str(), blanks[0].position), ("First", 1));
    assert_eq!(blanks[0].hints, Some(json!([{"text": "think"}])));
    let choices = choices_of(&app, blanks[0].id).await;
    // The question's key is the server's (inherited); positions per blank.
    let read: Vec<_> =
        choices.iter().map(|c| (c.content.as_str(), c.note.as_deref(), c.question_id, c.position)).collect();
    assert_eq!(read, vec![("yes", Some("because"), question, 1), ("no", None, question, 2)]);
    // Bottom level first: each row's hooks see its rows, the question's last.
    let blank = blanks[0].id;
    assert_eq!(
        *SAVED.lock().unwrap(),
        vec![
            format!("question_options:{}", choices[0].id),
            format!("question_options:{}", choices[1].id),
            format!("question_blanks:{blank}"),
            format!("questions:{question}"),
        ]
    );
    // A blank's choices are not the question's options.
    let options = admin.get(&app.admin_api(&format!("resources/questions/records/{question}/relations/options"))).await;
    assert_eq!(options.json()["meta"]["total"], json!(0));
    // History: the blank records its choices; the question its blanks only.
    let history = admin.get(&app.admin_api(&format!("resources/question_blanks/records/{blank}/history"))).await.json();
    let rows = history["data"].as_array().unwrap().iter().find(|e| e["event"] == "rows_changed").unwrap().clone();
    assert_eq!((rows["meta"]["relation"].clone(), rows["meta"]["created"].clone()), (json!("choices"), json!(2)));
    let history = admin.get(&app.admin_api(&format!("resources/questions/records/{question}/history"))).await.json();
    let rows: Vec<_> = history["data"].as_array().unwrap().iter().filter(|e| e["event"] == "rows_changed").collect();
    assert_eq!(rows.len(), 1);
    assert_eq!((rows[0]["meta"]["relation"].clone(), rows[0]["meta"]["created"].clone()), (json!("blanks"), json!(1)));
});

db_test!(new_rows_take_positions_after_every_row_of_the_inherited_ancestor, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    let admin = app.acting_as(&users.admin).await;
    // Positions unique per question, whatever the blank (a unique key the table may hold).
    let (question, blanks) = stored(&app, &[("A", "select", &["a1", "a2"]), ("B", "select", &[])]).await;
    let (b, _) = &blanks[1];
    let changed = update(
        &admin,
        &app,
        question,
        json!({"blanks": {"update": [{"key": b.to_string(), "data": {
            "choices": {"create": [{"ref": "n1", "data": {"content": "b1"}}], "order": [{"new": "n1"}]},
        }}]}}),
    )
    .await;
    changed.assert_status(StatusCode::OK);
    assert_eq!(choices_of(&app, *b).await[0].position, 3);
});

db_test!(rows_are_changed_and_removed_at_every_level, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    let admin = app.acting_as(&users.admin).await;
    let (question, blanks) = stored(&app, &[("A", "select", &["a1", "a2"]), ("B", "text", &["b1"])]).await;
    let (a, a_choices) = &blanks[0];
    let (b, b_choices) = &blanks[1];
    SAVED.lock().unwrap().clear();
    let changed = update(
        &admin,
        &app,
        question,
        json!({
            "blanks": {
                // Only the blank's rows change.
                "update": [{"key": a.to_string(), "data": {"choices": {
                    "update": [{"key": a_choices[0].to_string(), "data": {"content": "a1!"}}],
                    "delete": [a_choices[1].to_string()],
                    "create": [{"ref": "n1", "data": {"content": "a3"}}],
                }}}],
                // Removing a blank removes its choices.
                "delete": [b.to_string()],
            },
        }),
    )
    .await;
    changed.assert_status(StatusCode::OK);
    let rest = blanks_of(&app, question).await;
    assert_eq!(rest.iter().map(|b| b.id).collect::<Vec<_>>(), vec![*a]);
    let choices: Vec<_> = choices_of(&app, *a).await.into_iter().map(|c| c.content).collect();
    assert_eq!(choices, vec!["a1!", "a3"]);
    assert!(option::Entity::find_by_id(b_choices[0]).one(app.state().db()).await.unwrap().is_none());
    // The blank's hook ran (its rows changed), after them.
    let log = SAVED.lock().unwrap().clone();
    assert_eq!(log.last().unwrap(), &format!("questions:{question}"));
    assert_eq!(log[log.len() - 2], format!("question_blanks:{a}"));
    // The removed choice has its own history.
    let removed = rustcms_core::models::audit_log::Entity::find()
        .filter(rustcms_core::models::audit_log::Column::SubjectKey.eq(b_choices[0].to_string()))
        .filter(rustcms_core::models::audit_log::Column::SubjectType.eq("question_options"))
        .all(app.state().db())
        .await
        .unwrap();
    assert!(removed.iter().any(|e| e.event == "deleted"), "{removed:?}");
});

db_test!(a_row_whose_rows_the_user_cannot_see_is_not_removed, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    let admin = app.acting_as(&users.admin).await;
    let (question, blanks) = stored(&app, &[("A", "select", &["a1", "hidden"])]).await;
    let (a, _) = &blanks[0];
    let refused = update(&admin, &app, question, json!({"blanks": {"delete": [a.to_string()]}})).await;
    refused.assert_status(StatusCode::FORBIDDEN);
    // Nothing removed.
    assert_eq!(blanks_of(&app, question).await.len(), 1);
    assert_eq!(choices_of(&app, *a).await.len(), 2);
});

db_test!(inherited_keys_are_the_server_s, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    let admin = app.acting_as(&users.admin).await;
    let (question, blanks) = stored(&app, &[("A", "select", &["a1"])]).await;
    let (a, _) = &blanks[0];
    let refused = update(
        &admin,
        &app,
        question,
        json!({"blanks": {"update": [{"key": a.to_string(), "data": {
            "choices": {"create": [{"ref": "n2", "data": {"content": "x", "question_id": question}}]},
        }}]}}),
    )
    .await;
    refused.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
    let path = format!("blanks.{a}.choices.n2.question_id");
    assert!(refused.json()["errors"][&path].is_array(), "{}", refused.json());
    // A stored choice naming another question is not one of the blank's rows.
    let (other, _) = stored(&app, &[]).await;
    let stray = option::ActiveModel {
        question_id: Set(other),
        blank_id: Set(Some(*a)),
        content: Set("stray".into()),
        position: Set(9),
        ..Default::default()
    };
    let stray = stray.insert(app.state().db()).await.unwrap().id;
    let refused = update(
        &admin,
        &app,
        question,
        json!({"blanks": {"update": [{"key": a.to_string(), "data": {
            "choices": {"update": [{"key": stray.to_string(), "data": {"content": "mine"}}]},
        }}]}}),
    )
    .await;
    refused.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
    let path = format!("blanks.{a}.choices.{stray}");
    assert_eq!(refused.json()["errors"][&path][0], json!("is not one of the rows"), "{}", refused.json());
});

db_test!(files_of_rows_of_rows_are_checked_with_the_record_s, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    let admin = app.acting_as(&users.admin).await;
    let png = stored_media(&app, users.admin.id, "image/png", false).await;
    let pdf = stored_media(&app, users.admin.id, "application/pdf", false).await;
    let gone = stored_media(&app, users.admin.id, "image/png", true).await;
    let question = |image: &str| {
        json!({"title": "Q", "kind": "blank", "blanks": {"create": [{"ref": "n1", "data": {
            "label": "A", "mode": "select",
            "choices": {"create": [{"ref": "n2", "data": {"content": "x", "image": image}}]},
        }}]}})
    };
    // A file in the trash, or of a type the field refuses: named at its row, every level.
    for image in [&gone, &pdf] {
        let refused = create(&admin, &app, question(image)).await;
        refused.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
        assert!(refused.json()["errors"]["blanks.n1.choices.n2.image"].is_array(), "{}", refused.json());
    }
    let created = create(&admin, &app, question(&png)).await;
    created.assert_status(StatusCode::CREATED);
    let choice = option::Entity::find().filter(option::Column::Content.eq("x")).one(app.state().db()).await.unwrap();
    assert_eq!(choice.unwrap().image.map(|i| i.to_string()), Some(png));
});

db_test!(errors_are_named_by_every_level, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    let admin = app.acting_as(&users.admin).await;
    let long = "x".repeat(51);
    let refused = create(
        &admin,
        &app,
        json!({
            "title": "Q",
            "kind": "blank",
            "blanks": {"create": [{"ref": "n1", "data": {
                "label": "A",
                "mode": "select",
                "hints": [{}],
                "choices": {"create": [{"ref": "n2", "data": {"content": long}}]},
            }}]},
        }),
    )
    .await;
    refused.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
    let errors = refused.json()["errors"].clone();
    assert!(errors["blanks.n1.hints.0.text"].is_array(), "{errors}");
    let (question, blanks) = stored(&app, &[("A", "select", &["a1"])]).await;
    let (a, choices) = &blanks[0];
    let refused = update(
        &admin,
        &app,
        question,
        json!({"blanks": {"update": [{"key": a.to_string(), "data": {
            "choices": {"update": [{"key": choices[0].to_string(), "data": {"content": "y".repeat(51)}}]},
        }}]}}),
    )
    .await;
    refused.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
    let path = format!("blanks.{a}.choices.{}.content", choices[0]);
    assert!(refused.json()["errors"][&path].is_array(), "{}", refused.json());
    // A malformed nested change is named where it is.
    let refused = update(
        &admin,
        &app,
        question,
        json!({"blanks": {"update": [{"key": a.to_string(), "data": {
            "choices": {"create": "nope"},
        }}]}}),
    )
    .await;
    refused.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
    let path = format!("blanks.{a}.choices");
    assert_eq!(refused.json()["errors"][&path][0], json!("is invalid"), "{}", refused.json());
});

db_test!(the_rows_read_the_enclosing_row_and_its_conditions_remove_them, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    let admin = app.acting_as(&users.admin).await;
    let (question, blanks) = stored(&app, &[("A", "text", &["a1"])]).await;
    let (a, choices) = &blanks[0];
    // A text blank: the choice's note is hidden, dropped.
    let changed = update(
        &admin,
        &app,
        question,
        json!({"blanks": {"update": [{"key": a.to_string(), "data": {
            "choices": {"update": [{"key": choices[0].to_string(), "data": {"note": "dropped"}}]},
        }}]}}),
    )
    .await;
    changed.assert_status(StatusCode::OK);
    assert_eq!(choices_of(&app, *a).await[0].note, None);
    // Switched to select in the same save: the note shows, and is kept.
    let changed = update(
        &admin,
        &app,
        question,
        json!({"blanks": {"update": [{"key": a.to_string(), "data": {
            "mode": "select",
            "choices": {"update": [{"key": choices[0].to_string(), "data": {"note": "kept"}}]},
        }}]}}),
    )
    .await;
    changed.assert_status(StatusCode::OK);
    assert_eq!(choices_of(&app, *a).await[0].note.as_deref(), Some("kept"));
    // Switched to open: the blank hides its choices, the save removes them (and ignores what was sent).
    let changed = update(
        &admin,
        &app,
        question,
        json!({"blanks": {"update": [{"key": a.to_string(), "data": {
            "mode": "open",
            "choices": {"create": [{"ref": "n9", "data": {"content": "ignored"}}]},
        }}]}}),
    )
    .await;
    changed.assert_status(StatusCode::OK);
    assert!(choices_of(&app, *a).await.is_empty());
});

db_test!(a_save_changes_a_thousand_rows_at_most, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    let admin = app.acting_as(&users.admin).await;
    // Named: ten blanks of a hundred choices, and the blanks.
    let blanks: Vec<Value> = (0..10)
        .map(|b| {
            let choices: Vec<Value> =
                (0..100).map(|c| json!({"ref": format!("c{c}"), "data": {"content": format!("{b}-{c}")}})).collect();
            json!({"ref": format!("b{b}"), "data": {"label": "L", "mode": "select", "choices": {"create": choices}}})
        })
        .collect();
    let refused = create(&admin, &app, json!({"title": "Q", "kind": "blank", "blanks": {"create": blanks}})).await;
    refused.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(refused.json()["formIssues"][0]["code"], json!("too_many_rows_changed"), "{}", refused.json());
    // Removed with their blank: counted too.
    let (question, blanks) = stored(&app, &[("A", "select", &[])]).await;
    let (a, _) = &blanks[0];
    let many: Vec<option::ActiveModel> = (0..995)
        .map(|i| option::ActiveModel {
            question_id: Set(question),
            blank_id: Set(Some(*a)),
            content: Set(format!("c{i}")),
            position: Set(i + 1),
            ..Default::default()
        })
        .collect();
    option::Entity::insert_many(many).exec(app.state().db()).await.unwrap();
    let created: Vec<Value> =
        (0..10).map(|i| json!({"ref": format!("n{i}"), "data": {"label": "L", "mode": "open"}})).collect();
    let refused =
        update(&admin, &app, question, json!({"blanks": {"delete": [a.to_string()], "create": created}})).await;
    refused.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(refused.json()["formIssues"][0]["code"], json!("too_many_rows_changed"), "{}", refused.json());
    assert_eq!(choices_of(&app, *a).await.len(), 995);
});

db_test!(every_level_is_admitted_before_anything_is_read, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    let clerk = app.acting_as(&users.clerk).await;
    let (question, blanks) = stored(&app, &[("A", "select", &["a1"])]).await;
    let (a, choices) = &blanks[0];
    // No `question_options.create`: a nested create is refused.
    let refused = update(
        &clerk,
        &app,
        question,
        json!({"blanks": {"update": [{"key": a.to_string(), "data": {
            "choices": {"create": [{"ref": "n1", "data": {"content": "x"}}]},
        }}]}}),
    )
    .await;
    refused.assert_status(StatusCode::FORBIDDEN);
    // An update is allowed.
    let changed = update(
        &clerk,
        &app,
        question,
        json!({"blanks": {"update": [{"key": a.to_string(), "data": {
            "choices": {"update": [{"key": choices[0].to_string(), "data": {"content": "a1!"}}]},
        }}]}}),
    )
    .await;
    changed.assert_status(StatusCode::OK);
    // Removing a blank removes its choices: the choices' `delete` is needed (the clerk has none).
    let refused = update(&clerk, &app, question, json!({"blanks": {"delete": [a.to_string()]}})).await;
    refused.assert_status(StatusCode::FORBIDDEN);
    assert_eq!(choices_of(&app, *a).await.len(), 1);
});

db_test!(two_saves_of_one_question_s_choices_wait_for_each_other, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    let (question, blanks) = stored(&app, &[("A", "select", &["a1"]), ("B", "select", &["b1"])]).await;
    let first = app.acting_as(&users.admin).await;
    let second = app.acting_as(&users.admin).await;
    let change = |blank: i32, choice: i32, content: &str| {
        json!({"blanks": {"update": [{"key": blank.to_string(), "data": {
            "choices": {"update": [{"key": choice.to_string(), "data": {"content": content}}]},
        }}]}})
    };
    let (one, two) = tokio::join!(
        update(&first, &app, question, change(blanks[0].0, blanks[0].1[0], "a1!")),
        update(&second, &app, question, change(blanks[1].0, blanks[1].1[0], "b1!")),
    );
    one.assert_status(StatusCode::OK);
    two.assert_status(StatusCode::OK);
    assert_eq!(choices_of(&app, blanks[0].0).await[0].content, "a1!");
    assert_eq!(choices_of(&app, blanks[1].0).await[0].content, "b1!");
});

db_test!(the_view_page_shows_rows_of_rows, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    let admin = app.acting_as(&users.admin).await;
    let (question, _) = stored(&app, &[("A", "select", &["a1"])]).await;
    let schema = admin.get(&app.admin_api(&format!("resources/questions/schema?operation=view&key={question}"))).await;
    let schema = schema.json();
    let blanks = schema["infolist"]["components"].as_array().unwrap().iter().find(|c| c["name"] == "blanks").unwrap();
    assert_eq!(blanks["rows"]["relation"], json!("blanks"));
    // The blank's choices, read through the blank's relation.
    let choices = blanks["components"].as_array().unwrap().iter().find(|c| c["name"] == "choices").unwrap();
    assert_eq!(choices["rows"]["resource"], json!("question_options"));
    assert_eq!(choices["rows"]["relation"], json!("choices"));
    // In the nested repeater's order, though the blank's own form has no repeater.
    assert_eq!(choices["rows"]["orderBy"], json!("position"));
    assert!(choices["components"].as_array().unwrap().iter().any(|c| c["name"] == "content"));
});

db_test!(the_edit_form_holds_the_nested_repeater, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    let admin = app.acting_as(&users.admin).await;
    let (question, _) = stored(&app, &[("A", "select", &["a1"])]).await;
    let schema = admin.get(&app.admin_api(&format!("resources/questions/schema?operation=edit&key={question}"))).await;
    schema.assert_status(StatusCode::OK);
    let schema = schema.json();
    if app.state().db().get_database_backend() == sea_orm::DatabaseBackend::Sqlite {
        // Shared with the SPA tests: rows holding rows.
        support::contract("repeater-nested.schema.edit.json", &schema);
    }
    let blanks = schema["form"]["components"].as_array().unwrap().iter().find(|c| c["name"] == "blanks").unwrap();
    let rows = blanks["repeater"]["components"].as_array().unwrap();
    let choices = rows.iter().find(|c| c["name"] == "choices").unwrap();
    assert_eq!(choices["repeater"]["relationship"]["resource"], json!("question_options"));
    assert_eq!(choices["repeater"]["relationship"]["relation"], json!("choices"));
    let note = choices["repeater"]["components"].as_array().unwrap().iter().find(|c| c["name"] == "note").unwrap();
    assert_eq!(note["shownWhen"]["scope"], json!("parent"));
    // New blanks' rows: the create form, its choices too.
    let create = blanks["repeater"]["createComponents"].as_array().unwrap();
    assert!(create.iter().any(|c| c["name"] == "choices" && c["repeater"]["relationship"]["update"] == json!(false)));
    // The blank's JSON repeater, with its own rules.
    let hints = rows.iter().find(|c| c["name"] == "hints").unwrap();
    assert_eq!(hints["repeater"]["components"][0]["name"], json!("text"));
});

mod boot {
    use super::*;

    pub(super) fn fails(registry: fn() -> Registry) -> String {
        let Err(err) = std::panic::catch_unwind(move || registry().finish()) else { panic!("refused at boot") };
        err.downcast_ref::<String>().cloned().unwrap_or_default()
    }

    macro_rules! questions {
        ($name:ident, $rows:expr) => {
            struct $name;
            impl Resource for $name {
                type Entity = question::Entity;
                const SLUG: &'static str = "questions";
                fn meta() -> ResourceMeta {
                    ResourceMeta::new("Question")
                }
                fn fields() -> Vec<Field> {
                    vec![Field::column(question::Column::Title), Field::column(question::Column::Kind)]
                }
                fn form() -> Form {
                    Form::new().schema(components![Repeater::make("blanks").relationship().schema($rows)])
                }
                fn relations() -> Vec<Relation> {
                    vec![Relation::has_many("blanks", "question_blanks", "question_id")]
                }
            }
        };
    }

    #[test]
    fn the_nested_child_locks_its_parent_or_an_inherited_ancestor() {
        // Choices lock the question, but are not told its key: they would lock
        // a question the write never locked.
        questions!(
            NoInherit,
            components![
                TextInput::make("label"),
                Repeater::make("choices").relationship().schema(components![TextInput::make("content")]),
            ]
        );
        let message =
            fails(|| Registry::new().register::<NoInherit>().register::<BlankResource>().register::<OptionResource>());
        assert!(message.contains("must lock its parent"), "{message}");
    }

    #[test]
    fn inherited_fields_belong_to_an_enclosing_level() {
        questions!(
            WrongField,
            components![
                TextInput::make("label"),
                Repeater::make("choices")
                    .relationship()
                    .inherit("content")
                    .schema(components![TextInput::make("note")]),
            ]
        );
        let message =
            fails(|| Registry::new().register::<WrongField>().register::<BlankResource>().register::<OptionResource>());
        assert!(message.contains("`inherit(\"content\")`"), "{message}");
        // On a form's own repeater: refused.
        struct TopInherit;
        impl Resource for TopInherit {
            type Entity = question::Entity;
            const SLUG: &'static str = "questions";
            fn meta() -> ResourceMeta {
                ResourceMeta::new("Question")
            }
            fn fields() -> Vec<Field> {
                vec![Field::column(question::Column::Title)]
            }
            fn form() -> Form {
                Form::new().schema(components![
                    Repeater::make("blanks")
                        .relationship()
                        .inherit("question_id")
                        .schema(components![TextInput::make("label")])
                ])
            }
            fn relations() -> Vec<Relation> {
                vec![Relation::has_many("blanks", "question_blanks", "question_id")]
            }
        }
        let message = fails(|| Registry::new().register::<TopInherit>().register::<BlankResource>());
        assert!(message.contains("`inherit` is for relationship repeaters nested in rows"), "{message}");
    }

    #[test]
    fn nested_relations_are_the_row_resource_s() {
        questions!(
            Unknown,
            components![
                TextInput::make("label"),
                Repeater::make("answers").relationship().schema(components![TextInput::make("content")]),
            ]
        );
        let message =
            fails(|| Registry::new().register::<Unknown>().register::<BlankResource>().register::<OptionResource>());
        assert!(message.contains("needs a has-many relation named `answers`"), "{message}");
    }

    #[test]
    fn when_parent_reads_an_enclosing_row_only() {
        questions!(ParentAtTop, components![TextInput::make("label").shown_when(when_parent("kind").eq("blank"))]);
        let message = fails(|| Registry::new().register::<ParentAtTop>().register::<BlankResource>());
        assert!(message.contains("when_parent"), "{message}");
    }

    #[test]
    fn table_rows_hold_no_nested_repeater() {
        struct Table;
        impl Resource for Table {
            type Entity = question::Entity;
            const SLUG: &'static str = "questions";
            fn meta() -> ResourceMeta {
                ResourceMeta::new("Question")
            }
            fn fields() -> Vec<Field> {
                vec![Field::column(question::Column::Title)]
            }
            fn form() -> Form {
                Form::new().schema(components![Repeater::make("blanks").relationship().table().schema(components![
                        TextInput::make("label"),
                        Repeater::make("choices")
                            .relationship()
                            .inherit("question_id")
                            .schema(components![TextInput::make("content")]),
                    ])])
            }
            fn relations() -> Vec<Relation> {
                vec![Relation::has_many("blanks", "question_blanks", "question_id")]
            }
        }
        let message =
            fails(|| Registry::new().register::<Table>().register::<BlankResource>().register::<OptionResource>());
        assert!(message.contains("table rows hold no nested repeater"), "{message}");
    }

    #[test]
    fn a_nested_repeater_s_condition_reads_its_row_only() {
        // A row the save does not touch keeps its rows: a condition on the form
        // could hide them without the save removing them.
        struct RootCondition;
        impl Resource for RootCondition {
            type Entity = question::Entity;
            const SLUG: &'static str = "questions";
            fn meta() -> ResourceMeta {
                ResourceMeta::new("Question")
            }
            fn fields() -> Vec<Field> {
                vec![Field::column(question::Column::Title), Field::column(question::Column::Kind)]
            }
            fn form() -> Form {
                Form::new().schema(components![
                    TextInput::make("title"),
                    Repeater::make("blanks").relationship().schema(components![
                        TextInput::make("label"),
                        Repeater::make("choices")
                            .relationship()
                            .inherit("question_id")
                            .shown_when(when_root("title").filled())
                            .schema(components![TextInput::make("content")]),
                    ]),
                ])
            }
            fn relations() -> Vec<Relation> {
                vec![Relation::has_many("blanks", "question_blanks", "question_id")]
            }
        }
        let message = fails(|| {
            Registry::new().register::<RootCondition>().register::<BlankResource>().register::<OptionResource>()
        });
        assert!(message.contains("reads its row only"), "{message}");
    }

    #[test]
    fn nested_relationship_entries_show_relations_of_the_row() {
        struct Viewed;
        impl Resource for Viewed {
            type Entity = question::Entity;
            const SLUG: &'static str = "questions";
            fn meta() -> ResourceMeta {
                ResourceMeta::new("Question")
            }
            fn fields() -> Vec<Field> {
                vec![Field::column(question::Column::Title)]
            }
            fn infolist() -> Infolist {
                Infolist::new().schema(components![RepeatableEntry::make("blanks").relationship().schema(components![
                    TextEntry::make("label"),
                    RepeatableEntry::make("answers").relationship().schema(components![TextEntry::make("content")]),
                ])])
            }
            fn relations() -> Vec<Relation> {
                vec![Relation::has_many("blanks", "question_blanks", "question_id")]
            }
        }
        let message =
            fails(|| Registry::new().register::<Viewed>().register::<BlankResource>().register::<OptionResource>());
        assert!(message.contains("`relationship()` needs a has-many relation of `question_blanks`"), "{message}");
    }

    #[test]
    fn nested_relationship_entries_are_repeatable() {
        struct Viewed;
        impl Resource for Viewed {
            type Entity = question::Entity;
            const SLUG: &'static str = "questions";
            fn meta() -> ResourceMeta {
                ResourceMeta::new("Question")
            }
            fn fields() -> Vec<Field> {
                vec![Field::column(question::Column::Title)]
            }
            fn infolist() -> Infolist {
                Infolist::new().schema(components![
                    RepeatableEntry::make("blanks")
                        .relationship()
                        .schema(components![TextEntry::make("label"), TextEntry::make("choices").relationship(),])
                ])
            }
            fn relations() -> Vec<Relation> {
                vec![Relation::has_many("blanks", "question_blanks", "question_id")]
            }
        }
        let message =
            fails(|| Registry::new().register::<Viewed>().register::<BlankResource>().register::<OptionResource>());
        assert!(message.contains("`relationship()` is for repeatable entries"), "{message}");
    }

    #[test]
    fn the_resources_of_the_test_boot() {
        Registry::new()
            .register::<QuestionResource>()
            .register::<BlankResource>()
            .register::<OptionResource>()
            .finish();
    }
}

mod validate {
    use super::*;

    async fn options_of(app: &TestApp, question: i32) -> Vec<String> {
        option::Entity::find()
            .filter(option::Column::QuestionId.eq(question))
            .filter(option::Column::BlankId.is_null())
            .order_by_asc(option::Column::Position)
            .all(app.state().db())
            .await
            .unwrap()
            .into_iter()
            .map(|o| o.content)
            .collect()
    }

    fn options(contents: &[&str]) -> Value {
        let rows: Vec<Value> = contents
            .iter()
            .enumerate()
            .map(|(i, c)| json!({"ref": format!("n{}", i + 1), "data": {"content": c}}))
            .collect();
        json!({"create": rows})
    }

    db_test!(the_rule_holds_on_create_rows_sent_or_not, |db: TestDb| async move {
        let (app, users) = setup(db).await;
        let admin = app.acting_as(&users.admin).await;
        // No option sent: the rule runs on none.
        let refused = create(&admin, &app, json!({"title": "two", "kind": "choice"})).await;
        refused.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(
            refused.json()["errors"]["options"][0],
            json!("must have exactly two options"),
            "{}",
            refused.json()
        );
        // Rows of a hidden repeater are not checked.
        create(&admin, &app, json!({"title": "two", "kind": "blank"})).await.assert_status(StatusCode::CREATED);
        // Errors at a new row's input, by its ref.
        let refused =
            create(&admin, &app, json!({"title": "two", "kind": "choice", "options": options(&["a", "bad"])})).await;
        refused.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(refused.json()["errors"]["options.n2.content"][0], json!("is bad"), "{}", refused.json());
        let created =
            create(&admin, &app, json!({"title": "two", "kind": "choice", "options": options(&["a", "b"])})).await;
        created.assert_status(StatusCode::CREATED);
    });

    db_test!(the_rule_runs_when_a_save_changes_the_rows, |db: TestDb| async move {
        let (app, users) = setup(db).await;
        let admin = app.acting_as(&users.admin).await;
        let created =
            create(&admin, &app, json!({"title": "two", "kind": "choice", "options": options(&["a", "b"])})).await;
        let question: i32 = created.json()["data"]["key"].as_str().unwrap().parse().unwrap();
        // A third option: refused, nothing saved.
        let refused = update(&admin, &app, question, json!({"options": options(&["c"])})).await;
        refused.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
        assert!(refused.json()["errors"]["options"].is_array(), "{}", refused.json());
        assert_eq!(options_of(&app, question).await, vec!["a", "b"]);
        // A stored row's error names its key.
        let keys: Vec<i32> = option::Entity::find()
            .filter(option::Column::QuestionId.eq(question))
            .order_by_asc(option::Column::Position)
            .all(app.state().db())
            .await
            .unwrap()
            .iter()
            .map(|o| o.id)
            .collect();
        let bad = json!({"options": {"update": [{"key": keys[0].to_string(), "data": {"content": "bad"}}]}});
        let refused = update(&admin, &app, question, bad).await;
        let path = format!("options.{}.content", keys[0]);
        assert_eq!(refused.json()["errors"][&path][0], json!("is bad"), "{}", refused.json());
        // A save leaving the rows alone does not check them: an old question with
        // three options stays editable.
        let extra = option::ActiveModel {
            question_id: Set(question),
            content: Set("c".into()),
            position: Set(9),
            ..Default::default()
        };
        extra.insert(app.state().db()).await.unwrap();
        update(&admin, &app, question, json!({"kind": "choice"})).await.assert_status(StatusCode::OK);
    });

    db_test!(the_rule_reads_rows_the_user_cannot_see, |db: TestDb| async move {
        let (app, users) = setup(db).await;
        let admin = app.acting_as(&users.admin).await;
        let created = create(&admin, &app, json!({"title": "x", "kind": "choice", "options": options(&["a"])})).await;
        let question: i32 = created.json()["data"]["key"].as_str().unwrap().parse().unwrap();
        // Outside everyone's scope, still one of the question's options.
        let hidden = option::ActiveModel {
            question_id: Set(question),
            content: Set("hidden".into()),
            position: Set(9),
            ..Default::default()
        };
        hidden.insert(app.state().db()).await.unwrap();
        let key = option::Entity::find().filter(option::Column::Content.eq("a")).one(app.state().db()).await.unwrap();
        let key = key.unwrap().id.to_string();
        // Two options with the hidden one: the rule holds.
        let change = json!({"title": "two", "options": {"update": [{"key": key, "data": {"content": "a!"}}]}});
        update(&admin, &app, question, change).await.assert_status(StatusCode::OK);
    });

    db_test!(a_nested_rule_runs_per_row, |db: TestDb| async move {
        let (app, users) = setup(db).await;
        let admin = app.acting_as(&users.admin).await;
        let blank = |r: &str, choices: &[&str]| {
            let rows: Vec<Value> = choices
                .iter()
                .enumerate()
                .map(|(i, c)| json!({"ref": format!("c{i}"), "data": {"content": c}}))
                .collect();
            json!({"ref": r, "data": {"label": r, "mode": "select", "choices": {"create": rows}}})
        };
        // The same choice in two blanks: fine; twice in one: refused there.
        let created = create(
            &admin,
            &app,
            json!({"title": "Q", "kind": "blank", "blanks": {"create": [
                blank("b1", &["x", "y"]), blank("b2", &["x"]),
            ]}}),
        )
        .await;
        created.assert_status(StatusCode::CREATED);
        let refused = create(
            &admin,
            &app,
            json!({"title": "Q", "kind": "blank", "blanks": {"create": [
                blank("b1", &["x"]), blank("b2", &["x", "x"]),
            ]}}),
        )
        .await;
        refused.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(refused.json()["errors"]["blanks.b2.choices"][0], json!("must be distinct"), "{}", refused.json());
    });

    db_test!(a_json_repeater_s_rule_names_rows_by_index, |db: TestDb| async move {
        let (app, users) = setup(db).await;
        let admin = app.acting_as(&users.admin).await;
        let tags = json!([{"name": "a"}, {"name": "b"}, {"name": "a"}]);
        let refused = create(&admin, &app, json!({"title": "Q", "kind": "blank", "tags": tags})).await;
        refused.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(refused.json()["errors"]["tags.2.name"][0], json!("is a duplicate"), "{}", refused.json());
        let created = create(&admin, &app, json!({"title": "Q", "kind": "blank", "tags": [{"name": "a"}]})).await;
        created.assert_status(StatusCode::CREATED);
        // The rule reads what the save writes, after the hooks ("A" is stored "a").
        let refused =
            create(&admin, &app, json!({"title": "Q", "kind": "blank", "tags": [{"name": "A"}, {"name": "a"}]})).await;
        refused.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(refused.json()["errors"]["tags.1.name"][0], json!("is a duplicate"), "{}", refused.json());
    });

    #[test]
    fn rules_hold_on_the_form_s_repeaters_only() {
        struct NestedJson;
        impl Resource for NestedJson {
            type Entity = question::Entity;
            const SLUG: &'static str = "questions";
            fn meta() -> ResourceMeta {
                ResourceMeta::new("Question")
            }
            fn fields() -> Vec<Field> {
                vec![Field::column(question::Column::Tags)]
            }
            fn form() -> Form {
                Form::new().schema(components![Repeater::make("tags").schema(components![
                    TextInput::make("name"),
                    Repeater::make("aliases")
                        .schema(components![TextInput::make("alias")])
                        .validate_rows(|_, _| Ok(())),
                ])])
            }
        }
        let message = super::boot::fails(|| Registry::new().register::<NestedJson>());
        assert!(message.contains("`validate_rows` is for relationship repeaters"), "{message}");
    }
}

mod reactions {
    use super::*;

    async fn react(client: &TestClient<'_>, app: &TestApp, body: Value) -> TestResponse {
        client.post_json(&app.admin_api("resources/questions/form/react"), &body).await
    }

    fn asked(title: &str, rows: Value) -> Value {
        json!({"operation": "create", "changed": "title", "state": {"title": title, "kind": "blank", "blanks": rows}, "seq": 1})
    }

    db_test!(a_reaction_adds_and_names_rows, |db: TestDb| async move {
        let (app, users) = setup(db).await;
        let admin = app.acting_as(&users.admin).await;
        // The server names a new row (a ref not taken in the state).
        let answer = react(&admin, &app, asked("add", json!([{"ref": "r1", "data": {"label": "mine"}}]))).await;
        answer.assert_status(StatusCode::OK);
        let patch = answer.json()["patches"][0].clone();
        assert_eq!(patch["op"], json!("add_relation_row"));
        assert_eq!(patch["ref"], json!("r2"));
        assert_eq!(patch["data"], json!({"label": "auto", "mode": "select"}));
        // A later patch of the answer names the row it added.
        let answer = react(&admin, &app, asked("add then name", json!([]))).await.json();
        assert_eq!(
            answer["patches"][1],
            json!({"op": "update_relation_row", "field": "blanks", "id": "x", "data": {"label": "b"}})
        );
    });

    db_test!(patches_the_user_could_not_make_are_dropped, |db: TestDb| async move {
        let (app, users) = setup(db).await;
        let admin = app.acting_as(&users.admin).await;
        // The order column, an inherited or parent key, a refused value, an unknown
        // row, rows of a hidden repeater: none kept.
        let answer = react(&admin, &app, asked("not yours", json!([]))).await.json();
        assert_eq!(answer["patches"], json!([]), "{answer}");
        // Nothing to change: dropped.
        let rows = json!([{"ref": "n1", "data": {"label": "a"}}]);
        assert_eq!(react(&admin, &app, asked("same", rows)).await.json()["patches"], json!([]));
        // More rows than the panel edits: the repeater is left out of the state, the
        // reaction still runs, its rows are not patched.
        let many: Vec<Value> = (0..101).map(|i| json!({"ref": format!("n{i}"), "data": {}})).collect();
        let answer = react(&admin, &app, asked("add", json!(many))).await;
        answer.assert_status(StatusCode::OK);
        assert_eq!(answer.json()["patches"], json!([]));
    });

    db_test!(stored_rows_are_patched_by_key_as_the_user_may, |db: TestDb| async move {
        let (app, users) = setup(db).await;
        let (question, blanks) = stored(&app, &[("A", "select", &[]), ("B", "select", &[])]).await;
        let rows = json!([
            {"key": blanks[0].0.to_string(), "data": {"label": "A", "mode": "select"}},
            {"key": blanks[1].0.to_string(), "data": {"label": "B", "mode": "select"}},
            {"ref": "n1", "data": {"label": "C"}},
        ]);
        let edit = |title: &str| {
            json!({"operation": "edit", "key": question.to_string(), "changed": "title",
                "state": {"title": title, "kind": "blank", "blanks": rows}, "seq": 1})
        };
        let admin = app.acting_as(&users.admin).await;
        let answer = react(&admin, &app, edit("rename")).await.json();
        let labels: Vec<_> = answer["patches"].as_array().unwrap().iter().map(|p| p["data"]["label"].clone()).collect();
        assert_eq!(labels, vec![json!("A!"), json!("B!"), json!("C!")]);
        let answer = react(&admin, &app, edit("remove")).await.json();
        assert_eq!(
            answer["patches"][0],
            json!({"op": "remove_relation_row", "field": "blanks", "id": blanks[0].0.to_string()})
        );
        let answer = react(&admin, &app, edit("flag")).await.json();
        assert_eq!(answer["patches"][0]["message"], json!("check this"));
        // A user who may not remove blanks: the removal is dropped.
        let reader = UserFactory::new().name("Reader").create(app.state().db()).await;
        let auth = app.state().service::<AuthState>().unwrap();
        let rbac = RbacService::new(app.state().db(), &auth.resolver);
        let permissions: Vec<String> = [
            "questions.view_any",
            "questions.view",
            "questions.update",
            "question_blanks.view_any",
            "question_blanks.view",
            "question_blanks.update",
        ]
        .map(str::to_owned)
        .to_vec();
        let role = rbac.create_role(Actor::System, ADMIN_GUARD, "reader", &permissions).await.unwrap();
        rbac.assign_role(Actor::System, reader.id, role.id).await.unwrap();
        let reader = app.acting_as(&reader).await;
        assert_eq!(react(&reader, &app, edit("remove")).await.json()["patches"], json!([]));
        // A user who may create blanks but not list them: the form does not show the
        // repeater, nor does a reaction patch it.
        let blind = UserFactory::new().name("Blind").create(app.state().db()).await;
        let permissions: Vec<String> =
            ["questions.view_any", "questions.view", "questions.create", "question_blanks.create"]
                .map(str::to_owned)
                .to_vec();
        let role = rbac.create_role(Actor::System, ADMIN_GUARD, "blind", &permissions).await.unwrap();
        rbac.assign_role(Actor::System, blind.id, role.id).await.unwrap();
        let blind = app.acting_as(&blind).await;
        assert_eq!(react(&blind, &app, asked("add", json!([]))).await.json()["patches"], json!([]));
    });

    db_test!(a_relationship_repeater_reacts_to_its_rows, |db: TestDb| async move {
        let (app, users) = setup(db).await;
        let admin = app.acting_as(&users.admin).await;
        let body = json!({"operation": "create", "changed": "blanks", "seq": 1, "state": {"title": "Q", "kind": "blank",
            "blanks": [{"ref": "n1", "data": {"label": "set"}}, {"ref": "n2", "data": {}}]}});
        let answer = react(&admin, &app, body).await;
        answer.assert_status(StatusCode::OK);
        assert_eq!(
            answer.json()["patches"],
            json!([{"op": "update_relation_row", "field": "blanks", "id": "n2", "data": {"label": "Blank 2"}}])
        );
    });
}
