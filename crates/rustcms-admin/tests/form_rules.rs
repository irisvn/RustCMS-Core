//! Rules computed on the server (ADR 0044, part E): whether an input is
//! shown or required and which options it offers, answered in the schema, by
//! `form/rules`, and checked again on every save. SQLite, PostgreSQL, MySQL.

use std::time::Duration;

use axum::http::{Method, StatusCode};
use rustcms_admin::*;
use rustcms_auth::{ADMIN_GUARD, Actor, AuthProvider, AuthState, RbacService};
use rustcms_core::{AppBuilder, AppError, Config};
use rustcms_testing::{Factory, TestApp, TestClient, TestDb, TestResponse, UserFactory, db_test};
use sea_orm::{ActiveModelTrait, EntityTrait, Schema, Set};
use serde_json::{Value, json};

mod support;

pub mod account {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "accounts")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i32,
        pub name: String,
        pub country: Option<String>,
        pub tax_id: Option<String>,
        pub note: Option<String>,
        pub subject: Option<String>,
        pub topic: Option<String>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// Topics of a subject: what `options_fn` offers.
fn topics(subject: &str) -> Vec<SelectOption> {
    let values: &[&str] = if subject == "math" { &["algebra", "geometry"] } else { &["poetry"] };
    values.iter().map(|v| SelectOption { value: json!(v), label: v.to_uppercase() }).collect()
}

pub struct AccountResource;

impl Resource for AccountResource {
    type Entity = account::Entity;
    const SLUG: &'static str = "accounts";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Account").title("name")
    }

    fn fields() -> Vec<Field> {
        use account::Column as C;
        vec![
            Field::column(C::Name).required(),
            Field::column(C::Country),
            Field::column(C::TaxId),
            // Required where its rule shows it.
            Field::column(C::Note).required(),
            Field::column(C::Subject),
            Field::column(C::Topic),
        ]
    }

    fn form() -> Form {
        Form::new().schema(components![
            TextInput::make("name"),
            // "err" and "slow" make the rules fail.
            TextInput::make("country"),
            // Hidden by a declared condition for an account named "hide": never required then.
            TextInput::make("tax_id")
                .shown_when(when("name").ne("hide"))
                .depends_on(["country"])
                .required_fn(|_ctx, state| {
                    let country = state.text("country").to_owned();
                    Box::pin(async move {
                        match country.as_str() {
                            "err" => Err(AppError::internal("no country service")),
                            "slow" => {
                                tokio::time::sleep(Duration::from_secs(5)).await;
                                Ok(false)
                            }
                            country => Ok(country == "vn"),
                        }
                    })
                })
                .rules_timeout(Duration::from_millis(200)),
            // Shown for Vietnam, and (declared) once a name is given.
            TextInput::make("note").shown_when(when("name").filled()).depends_on(["country"]).shown_when_fn(
                |_ctx, state| {
                    let shown = state.text("country") == "vn";
                    Box::pin(async move { Ok(shown) })
                }
            ),
            TextInput::make("subject"),
            SelectInput::make("topic").depends_on(["subject"]).options_fn(|_ctx, state| {
                let options = topics(state.text("subject"));
                Box::pin(async move { Ok(options) })
            }),
        ])
    }
}

fn build(config: Config) -> AppBuilder {
    let mut app = AppBuilder::new(config);
    app.provider(AuthProvider::new());
    app.provider(AdminProvider::new().resource::<AccountResource>());
    app
}

async fn setup(db: TestDb) -> (TestApp, rustcms_core::models::user::Model) {
    db.migrate::<rustcms_migration::Migrator>().await;
    let schema = Schema::new(db.conn().get_database_backend());
    rustcms_testing::create_table(db.conn(), &schema.create_table_from_entity(account::Entity)).await;
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

async fn create(client: &TestClient<'_>, app: &TestApp, body: Value) -> TestResponse {
    client.post_json(&app.admin_api("resources/accounts/records"), &body).await
}

async fn stored(app: &TestApp, country: &str) -> i32 {
    let account =
        account::ActiveModel { name: Set("A".into()), country: Set(Some(country.into())), ..Default::default() };
    account.insert(app.state().db()).await.unwrap().id
}

fn input<'a>(schema: &'a Value, name: &str) -> &'a Value {
    schema["form"]["components"].as_array().unwrap().iter().find(|c| c["name"] == name).unwrap()
}

db_test!(the_schema_carries_the_first_answer, |db: TestDb| async move {
    let (app, admin) = setup(db).await;
    let client = app.acting_as(&admin).await;
    let create = client.get(&app.admin_api("resources/accounts/schema?operation=create")).await.json();
    if app.state().db().get_database_backend() == sea_orm::DatabaseBackend::Sqlite {
        // Shared with the SPA tests: inputs with rules computed on the server.
        support::contract("form-rules.schema.create.json", &create);
    }
    assert_eq!(input(&create, "tax_id")["serverRules"], json!({"dependsOn": ["country"], "required": false}));
    assert_eq!(input(&create, "note")["serverRules"], json!({"dependsOn": ["country"], "shown": false}));
    // Options computed for the form's values (no subject: poetry), translated labels.
    assert_eq!(input(&create, "topic")["options"], json!([{"value": "poetry", "label": "POETRY"}]));
    // The edit form: the record's values.
    let key = stored(&app, "vn").await;
    let edit = client.get(&app.admin_api(&format!("resources/accounts/schema?operation=edit&key={key}"))).await.json();
    assert_eq!(input(&edit, "tax_id")["serverRules"]["required"], json!(true));
    assert_eq!(input(&edit, "note")["serverRules"]["shown"], json!(true));
    // A failing rule: shown, not required, declared options.
    let failing = stored(&app, "err").await;
    let edit =
        client.get(&app.admin_api(&format!("resources/accounts/schema?operation=edit&key={failing}"))).await.json();
    assert_eq!(input(&edit, "tax_id")["serverRules"]["required"], json!(false));
    assert_eq!(input(&edit, "note")["serverRules"]["shown"], json!(true));
});

db_test!(the_route_answers_for_the_form_s_state, |db: TestDb| async move {
    let (app, admin) = setup(db).await;
    let client = app.acting_as(&admin).await;
    let url = app.admin_api("resources/accounts/form/rules");
    let ask = |state: Value| json!({"operation": "create", "state": state, "seq": 3});
    let answer = client.post_json(&url, &ask(json!({"name": "A", "country": "vn", "subject": "math"}))).await;
    answer.assert_status(StatusCode::OK);
    let answer = answer.json();
    assert_eq!(answer["seq"], json!(3));
    assert_eq!(answer["rules"]["tax_id"], json!({"shown": true, "required": true}));
    assert_eq!(answer["rules"]["note"]["shown"], json!(true));
    let values: Vec<_> =
        answer["rules"]["topic"]["options"].as_array().unwrap().iter().map(|o| o["value"].clone()).collect();
    assert_eq!(values, vec![json!("algebra"), json!("geometry")]);
    // A failing rule: nothing to apply.
    let failed = client.post_json(&url, &ask(json!({"country": "err"}))).await.json();
    assert_eq!((failed["failed"].clone(), failed["rules"].clone()), (json!(true), json!({})));
    // An edit of a record the user may update.
    let key = stored(&app, "us").await;
    let edit = json!({"operation": "edit", "key": key.to_string(), "state": {}, "seq": 1});
    assert_eq!(client.post_json(&url, &edit).await.json()["rules"]["tax_id"]["required"], json!(false));
});

db_test!(saves_hold_the_rules_on_every_path, |db: TestDb| async move {
    let (app, admin) = setup(db).await;
    let client = app.acting_as(&admin).await;
    // Required for Vietnam.
    let refused = create(&client, &app, json!({"name": "A", "country": "vn"})).await;
    refused.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(refused.json()["errors"]["tax_id"][0], json!("is required"), "{}", refused.json());
    // Hidden outside Vietnam: the note sent is dropped.
    let created = create(&client, &app, json!({"name": "A", "country": "us", "note": "dropped"})).await;
    created.assert_status(StatusCode::CREATED);
    let key: i32 = created.json()["data"]["key"].as_str().unwrap().parse().unwrap();
    let account = account::Entity::find_by_id(key).one(app.state().db()).await.unwrap().unwrap();
    assert_eq!(account.note, None);
    // A topic outside the subject's.
    let refused = create(&client, &app, json!({"name": "A", "subject": "lit", "topic": "geometry"})).await;
    assert_eq!(refused.json()["errors"]["topic"][0], json!("is not one of the options"), "{}", refused.json());
    create(&client, &app, json!({"name": "A", "subject": "math", "topic": "geometry"}))
        .await
        .assert_status(StatusCode::CREATED);
    // A rule that fails or runs out of time refuses the save.
    for country in ["err", "slow"] {
        let refused = create(&client, &app, json!({"name": "A", "country": country})).await;
        refused.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(refused.json()["formIssues"][0]["code"], json!("form_rules_failed"), "{}", refused.json());
    }
    // Shown by its rule, the field's own `required` holds.
    let refused = create(&client, &app, json!({"name": "A", "country": "vn", "tax_id": "1"})).await;
    assert_eq!(refused.json()["errors"]["note"][0], json!("is required"), "{}", refused.json());
    // Hidden by a declared condition: the rule's "required" does not apply.
    let created = create(&client, &app, json!({"name": "hide", "country": "vn", "note": "n"})).await;
    created.assert_status(StatusCode::CREATED);
    // An old record breaking the rule stays editable where the save leaves it alone…
    let old = stored(&app, "vn").await;
    client.fetch_csrf().await;
    let old_url = app.admin_api(&format!("resources/accounts/records/{old}"));
    client.send(Method::PATCH, &old_url, Some(&json!({"name": "renamed"}))).await.assert_status(StatusCode::OK);
    // …but not when the save sends what the rule depends on.
    let refused = client.send(Method::PATCH, &old_url, Some(&json!({"country": "vn"}))).await;
    assert_eq!(refused.json()["errors"]["tax_id"][0], json!("is required"), "{}", refused.json());
    // On update, the record as the form holds it: a Vietnamese account keeps its note.
    let key = stored(&app, "vn").await;
    client.fetch_csrf().await;
    let url = app.admin_api(&format!("resources/accounts/records/{key}"));
    let changed = client.send(Method::PATCH, &url, Some(&json!({"note": "kept", "tax_id": "123"}))).await;
    changed.assert_status(StatusCode::OK);
    let account = account::Entity::find_by_id(key).one(app.state().db()).await.unwrap().unwrap();
    assert_eq!(account.note.as_deref(), Some("kept"));
});

mod boot {
    use super::*;

    fn fails(form: fn() -> Form) -> String {
        thread_local! { static FORM: std::cell::Cell<Option<fn() -> Form>> = const { std::cell::Cell::new(None) }; }
        struct Probe;
        impl Resource for Probe {
            type Entity = account::Entity;
            const SLUG: &'static str = "accounts";
            fn meta() -> ResourceMeta {
                ResourceMeta::new("Account")
            }
            fn fields() -> Vec<Field> {
                AccountResource::fields()
            }
            fn form() -> Form {
                FORM.with(|f| f.get().expect("a form"))()
            }
        }
        FORM.with(|f| f.set(Some(form)));
        let Err(err) = std::panic::catch_unwind(|| Registry::new().register::<Probe>().finish()) else {
            panic!("refused at boot")
        };
        err.downcast_ref::<String>().cloned().unwrap_or_default()
    }

    #[test]
    fn rules_need_their_dependencies() {
        let message = fails(|| {
            Form::new().schema(components![TextInput::make("note").shown_when_fn(|_, _| Box::pin(async { Ok(true) }))])
        });
        assert!(message.contains("need `depends_on"), "{message}");
        let message = fails(|| {
            Form::new().schema(components![
                TextInput::make("note").depends_on(["nowhere"]).shown_when_fn(|_, _| Box::pin(async { Ok(true) }))
            ])
        });
        assert!(message.contains("`nowhere`, which is not an input"), "{message}");
    }

    #[test]
    fn rules_hold_on_the_right_inputs() {
        let message = fails(|| {
            Form::new().schema(components![
                TextInput::make("country"),
                TextInput::make("topic").depends_on(["country"]).options_fn(|_, _| Box::pin(async { Ok(vec![]) })),
            ])
        });
        assert!(message.contains("`options_fn` is for selects"), "{message}");
        let message = fails(|| {
            Form::new().schema(components![
                TextInput::make("country"),
                Hidden::make("note").depends_on(["country"]).shown_when_fn(|_, _| Box::pin(async { Ok(true) })),
            ])
        });
        assert!(message.contains("for inputs of fields"), "{message}");
    }

    #[test]
    fn conditions_never_read_a_computed_visibility() {
        let message = fails(|| {
            Form::new().schema(components![
                TextInput::make("country"),
                TextInput::make("note").depends_on(["country"]).shown_when_fn(|_, _| Box::pin(async { Ok(true) })),
                TextInput::make("subject").shown_when(when("note").filled()),
            ])
        });
        assert!(message.contains("whose visibility is computed on the server"), "{message}");
    }

    #[test]
    fn the_test_s_resource_boots() {
        Registry::new().register::<AccountResource>().finish();
    }
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct RuledSettings {
    pub notes: Vec<serde_json::Value>,
}

impl rustcms_core::settings::Settings for RuledSettings {
    const GROUP: &'static str = "ruled";
}

#[async_trait::async_trait]
impl SettingsPage for RuledSettings {
    fn meta() -> SettingsMeta {
        SettingsMeta::new("Ruled")
    }

    fn fields() -> Vec<SettingField> {
        vec![SettingField::repeater(
            Repeater::make("notes")
                .schema(components![TextInput::make("body")])
                .depends_on(Vec::<String>::new())
                .required_fn(|_, _| Box::pin(async { Ok(true) })),
        )]
    }
}

// Settings pages take no rules computed on the server (ADR 0044, part E).
#[test]
#[should_panic(expected = "are for resource forms")]
fn rules_on_settings_pages() {
    let _ = AdminProvider::new().settings::<RuledSettings>();
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct RowRuledSettings {
    pub notes: Vec<serde_json::Value>,
}

impl rustcms_core::settings::Settings for RowRuledSettings {
    const GROUP: &'static str = "row_ruled";
}

#[async_trait::async_trait]
impl SettingsPage for RowRuledSettings {
    fn meta() -> SettingsMeta {
        SettingsMeta::new("Row ruled")
    }

    fn fields() -> Vec<SettingField> {
        vec![SettingField::repeater(Repeater::make("notes").schema(components![
            TextInput::make("a"),
            TextInput::make("b").depends_on(["a"]).shown_when_fn(|_, _| Box::pin(async { Ok(true) })),
        ]))]
    }
}

// Rows of any repeater take no rules computed on the server: they are the form's own inputs'.
#[test]
#[should_panic(expected = "cannot have rules computed on the server")]
fn rules_in_rows() {
    let _ = AdminProvider::new().settings::<RowRuledSettings>();
}
