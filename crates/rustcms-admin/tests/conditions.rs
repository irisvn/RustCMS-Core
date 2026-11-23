//! Inputs and layouts shown under conditions (ADR 0037): carried by the
//! schema; on every write the server evaluates them on the resulting values,
//! drops values sent for hidden inputs, clears `clear_when_hidden` fields,
//! and requires required fields only when shown. Resources, settings pages
//! and action forms; declarations that cannot hold refuse to boot. SQLite,
//! PostgreSQL, MySQL.

use std::sync::Mutex;

use axum::http::{Method, StatusCode};
use rustcms_admin::*;
use rustcms_auth::{ADMIN_GUARD, Actor, AuthProvider, AuthState, RbacService};
use rustcms_core::settings::{self, Settings};
use rustcms_core::{AppBuilder, Config};
use rustcms_testing::{Factory, TestApp, TestClient, TestDb, TestResponse, UserFactory, db_test};
use sea_orm::{EntityTrait, Schema};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub mod offer {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "offers")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i32,
        pub name: String,
        pub link_type: String,
        pub url: Option<String>,
        #[sea_orm(default_value = false)]
        pub on_sale: bool,
        pub discount: Option<i32>,
        pub note: Option<String>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

fn offer_fields() -> Vec<Field> {
    use offer::Column as C;
    vec![
        Field::column(C::Name).required(),
        Field::column(C::LinkType).required(),
        Field::column(C::Url).required(),
        Field::column(C::OnSale),
        Field::column(C::Discount),
        Field::column(C::Note),
    ]
}

/// The data the `review` action's handler received.
static REVIEWED: Mutex<Option<Data>> = Mutex::new(None);

pub struct OfferResource;

impl Resource for OfferResource {
    type Entity = offer::Entity;
    const SLUG: &'static str = "offers";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Offer").title("name").importable()
    }

    fn fields() -> Vec<Field> {
        offer_fields()
    }

    fn form() -> Form {
        Form::new().schema(components![
            TextInput::make("name"),
            SelectInput::make("link_type").options([("internal", "Internal"), ("external", "External")]),
            TextInput::make("url").shown_when(when("link_type").eq("external")),
            Toggle::make("on_sale"),
            Section::make("Sale").shown_when(when("on_sale").is_true()).schema(components![
                NumberInput::make("discount").clear_when_hidden(),
                // Shown only with a discount: hidden whenever the section is.
                TextInput::make("note").shown_when(when("discount").filled()),
            ]),
        ])
    }

    fn actions() -> Vec<Action<offer::Entity>> {
        vec![
            Action::<offer::Entity>::new("review", |_ctx, _txn, _offers, data| {
                Box::pin(async move {
                    *REVIEWED.lock().unwrap() = Some(data);
                    Ok(())
                })
            })
            .form(vec![
                ActionField::select("verdict", [("ok", "OK"), ("other", "Other")]).required(),
                ActionField::text("reason").required().shown_when(when("verdict").eq("other")),
            ]),
        ]
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShippingSettings {
    pub enabled: bool,
    pub carrier: Option<String>,
    pub fee: i32,
}

impl Default for ShippingSettings {
    fn default() -> Self {
        Self { enabled: false, carrier: None, fee: 5 }
    }
}

impl Settings for ShippingSettings {
    const GROUP: &'static str = "shipping";
}

#[async_trait::async_trait]
impl SettingsPage for ShippingSettings {
    fn meta() -> SettingsMeta {
        SettingsMeta::new("Shipping")
    }

    fn fields() -> Vec<SettingField> {
        vec![
            SettingField::boolean("enabled"),
            SettingField::text("carrier").nullable().required(),
            SettingField::integer("fee"),
        ]
    }

    fn form() -> Form {
        Form::new().schema(components![
            Toggle::make("enabled"),
            TextInput::make("carrier").shown_when(when("enabled").is_true()),
            NumberInput::make("fee").shown_when(when("enabled").is_true()).clear_when_hidden(),
        ])
    }
}

/// A relay whose host is shown only when custom, reset when hidden, and
/// whose password is kept only while the host keeps its value.
#[derive(Clone, Serialize, Deserialize)]
pub struct RelaySettings {
    pub custom: bool,
    pub host: String,
    pub port: i32,
    pub password: settings::Secret,
}

impl Default for RelaySettings {
    fn default() -> Self {
        Self { custom: false, host: "relay.example".into(), port: 25, password: settings::Secret::default() }
    }
}

impl Settings for RelaySettings {
    const GROUP: &'static str = "relay";
}

#[async_trait::async_trait]
impl SettingsPage for RelaySettings {
    fn meta() -> SettingsMeta {
        SettingsMeta::new("Relay")
    }

    fn fields() -> Vec<SettingField> {
        vec![
            SettingField::boolean("custom"),
            SettingField::text("host"),
            SettingField::integer("port"),
            SettingField::secret("password").kept_while(&["host"]),
        ]
    }

    fn form() -> Form {
        Form::new().schema(components![
            Toggle::make("custom"),
            TextInput::make("host").shown_when(when("custom").is_true()).clear_when_hidden(),
            NumberInput::make("port").shown_when(when("custom").is_true()),
            SecretInput::make("password"),
        ])
    }
}

fn build(config: Config) -> AppBuilder {
    // Imports keep their file on a disk: each app its own temporary directory.
    let root = std::env::temp_dir().join(format!("rustcms-conditions-{}", uuid::Uuid::now_v7()));
    let config = config.with("storage.root", root.to_string_lossy()).expect("storage config");
    let mut app = AppBuilder::new(config);
    app.provider(rustcms_core::storage::StorageProvider);
    app.provider(AuthProvider::new());
    app.provider(
        AdminProvider::new().resource::<OfferResource>().settings::<ShippingSettings>().settings::<RelaySettings>(),
    );
    app
}

async fn setup(db: TestDb) -> (TestApp, rustcms_core::models::user::Model) {
    db.migrate::<rustcms_migration::Migrator>().await;
    let schema = Schema::new(db.conn().get_database_backend());
    rustcms_testing::create_table(db.conn(), &schema.create_table_from_entity(offer::Entity)).await;
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
    client.post_json(&app.admin_api("resources/offers/records"), &body).await
}

async fn update(client: &TestClient<'_>, app: &TestApp, key: &str, body: Value) -> TestResponse {
    client.send(Method::PATCH, &app.admin_api(&format!("resources/offers/records/{key}")), Some(&body)).await
}

async fn stored(app: &TestApp, id: i32) -> offer::Model {
    offer::Entity::find_by_id(id).one(app.state().db()).await.unwrap().unwrap()
}

fn key(response: &TestResponse) -> String {
    response.json()["data"]["key"].as_str().unwrap().to_owned()
}

db_test!(the_schema_carries_the_conditions, |db: TestDb| async move {
    let (app, admin) = setup(db).await;
    let client = app.acting_as(&admin).await;
    let schema = client.get(&app.admin_api("resources/offers/schema")).await.json();
    let components = schema["form"]["components"].as_array().unwrap();
    let url = components.iter().find(|c| c["name"] == json!("url")).unwrap();
    assert_eq!(url["shownWhen"], json!({"op": "eq", "name": "link_type", "value": "external"}));
    let section = components.iter().find(|c| c["type"] == json!("section")).unwrap();
    assert_eq!(section["shownWhen"], json!({"op": "is_true", "name": "on_sale"}));
    let discount = &section["components"][0];
    assert_eq!((discount["clearWhenHidden"].clone(), discount.get("shownWhen")), (json!(true), None));
    assert!(components.iter().find(|c| c["name"] == json!("name")).unwrap().get("shownWhen").is_none());
    assert_eq!(schema["vocabularyVersion"], json!(27));
});

db_test!(hidden_values_are_never_stored_and_required_means_when_shown, |db: TestDb| async move {
    let (app, admin) = setup(db).await;
    let client = app.acting_as(&admin).await;

    // Internal link: `url` hidden, so not required, and a value sent for it is dropped.
    let internal = create(&client, &app, json!({"name": "a", "link_type": "internal", "url": "https://x"})).await;
    internal.assert_status(StatusCode::CREATED);
    let id: i32 = key(&internal).parse().unwrap();
    assert_eq!(stored(&app, id).await.url, None);

    // External link: `url` shown and required.
    let missing = create(&client, &app, json!({"name": "b", "link_type": "external"})).await;
    missing.assert_validation_error("url");
    assert_eq!(missing.json()["issues"]["url"][0]["code"], json!("required"));
    let external = create(&client, &app, json!({"name": "c", "link_type": "external", "url": "https://c"})).await;
    external.assert_status(StatusCode::CREATED);
    let key = key(&external);
    let id: i32 = key.parse().unwrap();

    // Hidden by an update: the stored value is kept (no `clear_when_hidden`)…
    update(&client, &app, &key, json!({"link_type": "internal"})).await.assert_status(StatusCode::OK);
    assert_eq!(stored(&app, id).await.url.as_deref(), Some("https://c"));
    // …and a value sent while hidden is dropped.
    update(&client, &app, &key, json!({"url": "https://other"})).await.assert_status(StatusCode::OK);
    assert_eq!(stored(&app, id).await.url.as_deref(), Some("https://c"));
    // Shown again by the stored value merged with the input: required again.
    let emptied = update(&client, &app, &key, json!({"link_type": "external", "url": null})).await;
    assert_eq!(emptied.json()["issues"]["url"][0]["code"], json!("required"));
});

db_test!(a_hidden_section_clears_and_hides_what_depends_on_it, |db: TestDb| async move {
    let (app, admin) = setup(db).await;
    let client = app.acting_as(&admin).await;
    let body = json!({"name": "s", "link_type": "internal", "on_sale": true, "discount": 10, "note": "spring"});
    let created = create(&client, &app, body).await;
    created.assert_status(StatusCode::CREATED);
    let key = key(&created);
    let id: i32 = key.parse().unwrap();
    let offer = stored(&app, id).await;
    assert_eq!((offer.discount, offer.note.as_deref()), (Some(10), Some("spring")));

    // The section hidden: `discount` cleared; `note` hidden too (its condition
    // reads a hidden field), kept, and a value sent for it dropped.
    update(&client, &app, &key, json!({"on_sale": false, "note": "autumn"})).await.assert_status(StatusCode::OK);
    let offer = stored(&app, id).await;
    assert_eq!((offer.discount, offer.note.as_deref()), (None, Some("spring")));

    // Created hidden: nothing stored for the hidden inputs.
    let body = json!({"name": "t", "link_type": "internal", "discount": 50, "note": "x"});
    let hidden = create(&client, &app, body).await;
    let offer = stored(&app, key_of(&hidden)).await;
    assert_eq!((offer.on_sale, offer.discount, offer.note), (false, None, None));
});

db_test!(the_import_preview_checks_rows_as_the_create_will, |db: TestDb| async move {
    let (app, admin) = setup(db).await;
    let client = app.acting_as(&admin).await;
    let request = axum::http::Request::builder()
        .method(Method::POST)
        .uri(app.admin_api("resources/offers/imports"))
        .header(axum::http::header::CONTENT_TYPE, "text/csv")
        .body(axum::body::Body::from("name,link_type,url\r\na,internal,https://x\r\nb,external,\r\n"))
        .unwrap();
    let uploaded = client.request(request).await;
    uploaded.assert_status(StatusCode::CREATED);
    let id = uploaded.json()["id"].as_str().unwrap().to_owned();
    let mapping = json!({"mapping": {"name": "name", "link_type": "link_type", "url": "url"}});
    let preview = client.post_json(&app.admin_api(&format!("imports/{id}/preview")), &mapping).await.json();
    // Row 2: `url` hidden, fine. Row 3: `url` shown and required.
    assert_eq!(preview["valid"], json!(1), "{preview}");
    assert_eq!(
        (
            preview["report"][0]["row"].clone(),
            preview["report"][0]["field"].clone(),
            preview["report"][0]["code"].clone()
        ),
        (json!(3), json!("url"), json!("required"))
    );
});

fn key_of(response: &TestResponse) -> i32 {
    key(response).parse().unwrap()
}

db_test!(settings_and_actions_follow_their_conditions, |db: TestDb| async move {
    let (app, admin) = setup(db).await;
    let client = app.acting_as(&admin).await;
    let put = |version: &'static str, data: Value| {
        let client = &client;
        let url = app.admin_api("settings/shipping");
        async move {
            client.fetch_csrf().await;
            client.send(Method::PUT, &url, Some(&json!({"version": version, "data": data}))).await
        }
    };

    // Disabled: `carrier` hidden, not required, a value sent dropped.
    put("0", json!({"enabled": false, "carrier": "Post"})).await.assert_status(StatusCode::OK);
    let saved: ShippingSettings = settings::get(app.state()).await.unwrap();
    assert_eq!(saved, ShippingSettings::default());
    // Enabled: required.
    let missing = put("0", json!({"enabled": true})).await;
    assert_eq!(missing.json()["issues"]["carrier"][0]["code"], json!("required"));
    put("0", json!({"enabled": true, "carrier": "Post", "fee": 9})).await.assert_status(StatusCode::OK);
    // Disabled again: `fee` back to its default (`clear_when_hidden`), `carrier` kept.
    put("1", json!({"enabled": false})).await.assert_status(StatusCode::OK);
    let saved: ShippingSettings = settings::get(app.state()).await.unwrap();
    assert_eq!(saved, ShippingSettings { enabled: false, carrier: Some("Post".into()), fee: 5 });

    // Actions: a hidden field reaches the handler as null and is not required.
    let offer = create(&client, &app, json!({"name": "r", "link_type": "internal"})).await;
    let run = |data: Value| json!({"keys": [key(&offer)], "data": data});
    let path = app.admin_api("resources/offers/actions/review");
    client
        .post_json(&path, &run(json!({"verdict": "ok", "reason": "sent anyway"})))
        .await
        .assert_status(StatusCode::OK);
    assert_eq!(
        REVIEWED.lock().unwrap().take(),
        Some(json!({"verdict": "ok", "reason": null}).as_object().unwrap().clone())
    );
    let missing = client.post_json(&path, &run(json!({"verdict": "other"}))).await;
    missing.assert_validation_error("reason");
    client.post_json(&path, &run(json!({"verdict": "other", "reason": "late"}))).await.assert_status(StatusCode::OK);
    assert_eq!(REVIEWED.lock().unwrap().take().unwrap()["reason"], json!("late"));
});

/// A form whose `name` input (NOT NULL, no default) is conditional.
pub struct NotNullResource;

impl Resource for NotNullResource {
    type Entity = offer::Entity;
    const SLUG: &'static str = "offers";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Offer").title("name")
    }

    fn fields() -> Vec<Field> {
        offer_fields()
    }

    fn form() -> Form {
        Form::new()
            .schema(
                components![Toggle::make("on_sale"), TextInput::make("name").shown_when(when("on_sale").is_true()),],
            )
    }
}

#[test]
#[should_panic(expected = "input `name` has a condition but its column is not nullable and has no default")]
fn a_conditional_input_needs_a_column_that_can_be_left_unset() {
    Registry::new().register::<NotNullResource>();
}

/// A conditional encrypted field: its value never reaches the conditions.
pub struct SecretCondition;

impl Resource for SecretCondition {
    type Entity = offer::Entity;
    const SLUG: &'static str = "offers";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Offer").title("name")
    }

    fn fields() -> Vec<Field> {
        use offer::Column as C;
        vec![
            Field::column(C::Name).required(),
            Field::column(C::LinkType).required(),
            Field::column(C::Url).encrypted(),
            Field::column(C::OnSale),
        ]
    }

    fn form() -> Form {
        Form::new().schema(components![
            Toggle::make("on_sale"),
            SecretInput::make("url").shown_when(when("on_sale").is_true()),
        ])
    }
}

#[test]
#[should_panic(expected = "encrypted field `url` cannot have a condition")]
fn an_encrypted_field_cannot_have_a_condition() {
    Registry::new().register::<SecretCondition>();
}

db_test!(a_secret_bound_to_a_field_a_condition_resets_is_cleared, |db: TestDb| async move {
    let (app, admin) = setup(db).await;
    let client = app.acting_as(&admin).await;
    let put = |version: String, data: Value| {
        let client = &client;
        let url = app.admin_api("settings/relay");
        async move {
            client.fetch_csrf().await;
            client.send(Method::PUT, &url, Some(&json!({"version": version, "data": data}))).await
        }
    };
    let version = |response: &TestResponse| response.json()["version"].as_str().unwrap().to_owned();
    let saved = put("0".into(), json!({"custom": true, "host": "mail.example", "password": "pw"})).await;
    saved.assert_status(StatusCode::OK);
    // Hiding the host resets it to its default: the password, given for the
    // custom host, is not kept for the default one.
    let saved = put(version(&saved), json!({"custom": false})).await;
    saved.assert_status(StatusCode::OK);
    assert_eq!(saved.json()["secrets"]["password"], json!("unset"));
    let stored: RelaySettings = settings::get(app.state()).await.unwrap();
    assert_eq!(stored.host, "relay.example");

    // A port sent as null while hidden: dropped, no error; shown and null: required.
    let saved = put(version(&saved), json!({"port": null})).await;
    saved.assert_status(StatusCode::OK);
    put(version(&saved), json!({"custom": true, "port": null})).await.assert_validation_error("port");
});

db_test!(a_setting_already_hidden_is_not_reset_again, |db: TestDb| async move {
    use sea_orm::TransactionTrait;
    let (app, admin) = setup(db).await;
    // Stored while hidden (written by the application, not through the page).
    let txn = app.state().db().begin().await.unwrap();
    let values = json!({"custom": false, "host": "kept.example", "port": 25}).as_object().unwrap().clone();
    settings::write(&txn, app.state().keyring(), "relay", 1, values, &Default::default(), None).await.unwrap();
    txn.commit().await.unwrap();
    let client = app.acting_as(&admin).await;
    client.fetch_csrf().await;
    let url = app.admin_api("settings/relay");
    client
        .send(Method::PUT, &url, Some(&json!({"version": "1", "data": {"custom": false}})))
        .await
        .assert_status(StatusCode::OK);
    let stored: RelaySettings = settings::get(app.state()).await.unwrap();
    assert_eq!(stored.host, "kept.example", "hidden before this save: not reset");
});
