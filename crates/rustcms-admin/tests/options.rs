//! Static options of selects and checkbox lists bound client writes to a
//! resource (API create and update, relation creates, CSV imports), compared
//! in canonical wire form; hooks, actions and `belongs_to` keep their own
//! rules; declarations that cannot hold refuse to boot. SQLite, PostgreSQL,
//! MySQL.

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use rustcms_admin::*;
use rustcms_auth::{ADMIN_GUARD, Actor, AuthProvider, AuthState, RbacService};
use rustcms_core::{AppBuilder, AppResult, Config};
use rustcms_testing::{Factory, TestApp, TestClient, TestDb, TestResponse, UserFactory, db_test};
use sea_orm::{ActiveModelTrait, ActiveValue::Set, EntityTrait, IntoActiveModel, PaginatorTrait, Schema};
use serde_json::{Value, json};

pub mod palette {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "palettes")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i32,
        pub name: String,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod swatch {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "swatches")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i32,
        #[sea_orm(unique)]
        pub name: String,
        pub status: String,
        pub colors: Option<Json>,
        pub serial: Option<i64>,
        #[sea_orm(column_type = "Decimal(Some((10, 2)))", nullable)]
        pub price: Option<Decimal>,
        pub token: Option<Uuid>,
        pub at: Option<DateTimeUtc>,
        pub weight: Option<f32>,
        pub local_at: Option<DateTime>,
        /// Never written by clients.
        pub tier: Option<String>,
        /// Written, never read back.
        pub code: Option<String>,
        pub palette_id: Option<i32>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod vault {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "vaults")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i32,
        #[sea_orm(column_type = "Text", nullable)]
        pub token: Option<rustcms_core::crypto::Encrypted<String>>,
        #[sea_orm(column_type = "Text", nullable)]
        pub env: Option<rustcms_core::crypto::Encrypted<std::collections::BTreeMap<String, String>>>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

const TOKEN: &str = "67e55044-10b1-426f-9247-bb680e5fe0c8";

pub struct PaletteResource;

impl Resource for PaletteResource {
    type Entity = palette::Entity;
    const SLUG: &'static str = "palettes";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Palette").title("name")
    }

    fn fields() -> Vec<Field> {
        vec![Field::column(palette::Column::Name).required()]
    }

    fn relations() -> Vec<Relation> {
        vec![Relation::has_many("swatches", "swatches", "palette_id")]
    }
}

fn swatch_fields() -> Vec<Field> {
    use swatch::Column as C;
    vec![
        Field::column(C::Name).required(),
        Field::column(C::Status).required(),
        Field::column(C::Colors),
        Field::column(C::Serial),
        Field::column(C::Price),
        Field::column(C::Token),
        Field::column(C::At),
        Field::column(C::Weight),
        Field::column(C::LocalAt),
        Field::column(C::Tier).creatable(|_| false).updatable(|_| false),
        Field::column(C::Code).readable(|_| false),
        Field::column(C::PaletteId).belongs_to("palettes"),
    ]
}

pub struct SwatchResource;

#[async_trait::async_trait]
impl Resource for SwatchResource {
    type Entity = swatch::Entity;
    const SLUG: &'static str = "swatches";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Swatch").title("name").unique("swatches_name_key", &["name"], "is taken").import_key("name")
    }

    fn fields() -> Vec<Field> {
        swatch_fields()
    }

    fn form() -> Form {
        Form::new().schema(components![
            TextInput::make("name"),
            SelectInput::make("status").options([("draft", "Draft"), ("live", "Live")]),
            Section::make("More").schema(components![
                CheckboxList::make("colors").options([("red", "Red"), ("green", "Green"), ("blue", "Blue")]),
                // The same options again, in another order and with other labels.
                SelectInput::make("status").options([("live", "Online"), ("draft", "Draft")]),
            ]),
            // Declared in other spellings: compared and shown canonical.
            SelectInput::make("serial").options([("0012", "Twelve"), ("7", "Seven")]),
            SelectInput::make("price").options([("10.5", "Cheap"), ("20", "Dear")]),
            SelectInput::make("token").options([(TOKEN.to_uppercase(), "First")]),
            SelectInput::make("at")
                .options([("2026-01-01T02:00:00+02:00", "New year"), ("2026-01-01T00:00:00.123456Z", "Just after")]),
            // Values an `f32` and timestamps (microseconds) store exactly.
            SelectInput::make("weight").options([(0.5, "Half"), (0.25, "Quarter")]),
            SelectInput::make("local_at").options([("2026-01-01T10:00:00.123456", "Morning")]),
            SelectInput::make("tier").options([("gold", "Gold")]),
            SelectInput::make("code").options([("x1", "X1")]),
            // A relation: its key is checked against the relation, never these.
            SelectInput::make("palette_id").options([(999_999, "Stray")]),
        ])
    }

    fn actions() -> Vec<Action<swatch::Entity>> {
        vec![
            Action::<swatch::Entity>::new("archive", |_, txn, swatches, _| {
                Box::pin(async move {
                    for swatch in swatches {
                        let mut active = swatch.into_active_model();
                        active.status = Set("archived".into());
                        active.update(txn).await?;
                    }
                    Ok(())
                })
            })
            .ability("update"),
        ]
    }

    /// A swatch named `hooked` is written out of the options by the hook.
    async fn mutate_before_create(_ctx: &Ctx, _txn: &sea_orm::DatabaseTransaction, mut data: Data) -> AppResult<Data> {
        if data.get("name") == Some(&json!("hooked")) {
            data.insert("status".into(), json!("archived"));
            data.insert("colors".into(), json!(["pink", "pink"]));
        }
        Ok(data)
    }
}

fn build(config: Config) -> AppBuilder {
    let root = std::env::temp_dir().join(format!("rustcms-options-{}", uuid::Uuid::now_v7()));
    let config = config.with("storage.root", root.to_string_lossy()).expect("storage config");
    let mut app = AppBuilder::new(config);
    app.provider(rustcms_core::storage::StorageProvider);
    app.provider(AuthProvider::new());
    app.provider(AdminProvider::new().resource::<PaletteResource>().resource::<SwatchResource>());
    app
}

async fn setup(db: TestDb) -> (TestApp, rustcms_core::models::user::Model) {
    db.migrate::<rustcms_migration::Migrator>().await;
    let schema = Schema::new(db.conn().get_database_backend());
    for statement in [schema.create_table_from_entity(palette::Entity), schema.create_table_from_entity(swatch::Entity)]
    {
        rustcms_testing::create_table(db.conn(), &statement).await;
    }
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

/// The first issue code of `field` in a 422.
fn code(response: &TestResponse, field: &str) -> Value {
    response.assert_validation_error(field);
    response.json()["issues"][field][0]["code"].clone()
}

async fn count(app: &TestApp) -> u64 {
    swatch::Entity::find().count(app.state().db()).await.unwrap()
}

async fn stored(app: &TestApp, key: &str) -> swatch::Model {
    swatch::Entity::find_by_id(key.parse::<i32>().unwrap()).one(app.state().db()).await.unwrap().unwrap()
}

async fn create(client: &TestClient<'_>, app: &TestApp, body: Value) -> TestResponse {
    client.post_json(&app.admin_api("resources/swatches/records"), &body).await
}

db_test!(creates_take_only_the_options, |db: TestDb| async move {
    let (app, admin) = setup(db).await;
    let client = app.acting_as(&admin).await;

    let refused = [
        (json!({"status": "pink"}), "status", "not_option"),
        (json!({"colors": ["red", "pink"]}), "colors", "not_option"),
        (json!({"colors": ["red", "red"]}), "colors", "given_twice"),
        (json!({"colors": "red"}), "colors", "invalid"),
        (json!({"colors": {"red": true}}), "colors", "invalid"),
        (json!({"serial": "13"}), "serial", "not_option"),
        (json!({"price": "10.51"}), "price", "not_option"),
        (json!({"token": "00000000-0000-0000-0000-000000000000"}), "token", "not_option"),
        (json!({"at": "2026-01-01T00:00:01Z"}), "at", "not_option"),
        (json!({"code": "x2"}), "code", "not_option"),
        // Denied fields stay denied, whatever the value.
        (json!({"tier": "gold"}), "tier", "not_allowed"),
        (json!({"status": null}), "status", "required"),
    ];
    for (patch, field, expected) in refused {
        let mut body = json!({"name": "a", "status": "draft"});
        body.as_object_mut().unwrap().extend(patch.as_object().unwrap().clone());
        let response = create(&client, &app, body.clone()).await;
        assert_eq!(code(&response, field), json!(expected), "{body}");
    }
    // The wire types stay strict: a number is not an `i64` value.
    let wrong = create(&client, &app, json!({"name": "a", "status": "draft", "serial": 12})).await;
    assert_ne!(code(&wrong, "serial"), json!("not_option"));
    assert_eq!(count(&app).await, 0, "nothing written");

    // Every spelling of an option, `[]` and `null`.
    let body = json!({
        "name": "a",
        "status": "live",
        "colors": ["blue", "red"],
        "serial": "12",
        "price": "10.500",
        "token": TOKEN.to_uppercase(),
        "at": "2026-01-01T00:00:00Z",
        "code": "x1",
    });
    let created = create(&client, &app, body).await;
    created.assert_status(StatusCode::CREATED);
    let key = created.json()["data"]["key"].as_str().unwrap().to_owned();
    let row = stored(&app, &key).await;
    assert_eq!((row.status.as_str(), row.serial, row.colors), ("live", Some(12), Some(json!(["blue", "red"]))));
    assert_eq!(row.price.map(|p| p.to_string().parse::<f64>().unwrap()), Some(10.5));
    assert_eq!(row.token.map(|t| t.to_string()).as_deref(), Some(TOKEN));
    assert_eq!(row.at.map(|t| t.to_rfc3339()).as_deref(), Some("2026-01-01T00:00:00+00:00"));
    let others = json!({"name": "b", "status": "draft", "colors": [], "serial": "0007", "price": "20", "at": null});
    create(&client, &app, others).await.assert_status(StatusCode::CREATED);
    let empty = json!({"name": "c", "status": "draft", "colors": null, "token": null});
    create(&client, &app, empty).await.assert_status(StatusCode::CREATED);
    let offset = json!({"name": "d", "status": "draft", "at": "2026-01-01T02:00:00+02:00"});
    create(&client, &app, offset).await.assert_status(StatusCode::CREATED);
});

db_test!(updates_take_only_the_options_and_keep_what_they_omit, |db: TestDb| async move {
    let (app, admin) = setup(db).await;
    let client = app.acting_as(&admin).await;
    // A record stored before the options were checked.
    let legacy = swatch::ActiveModel {
        name: Set("legacy".into()),
        status: Set("retired".into()),
        colors: Set(Some(json!(["pink"]))),
        ..Default::default()
    };
    let legacy = legacy.insert(app.state().db()).await.unwrap();
    let uri = app.admin_api(&format!("resources/swatches/records/{}", legacy.id));

    let refused = client.send(Method::PATCH, &uri, Some(&json!({"status": "retired"}))).await;
    assert_eq!(code(&refused, "status"), json!("not_option"));
    let refused = client.send(Method::PATCH, &uri, Some(&json!({"name": "renamed", "colors": ["pink"]}))).await;
    assert_eq!(code(&refused, "colors"), json!("not_option"));
    let refused = client.send(Method::PATCH, &uri, Some(&json!({"colors": ["green", "green"]}))).await;
    assert_eq!(code(&refused, "colors"), json!("given_twice"));
    let row = stored(&app, &legacy.id.to_string()).await;
    assert_eq!((row.name.as_str(), row.status.as_str()), ("legacy", "retired"), "nothing written");

    // Omitted fields keep their stored values.
    let renamed = client.send(Method::PATCH, &uri, Some(&json!({"name": "renamed"}))).await;
    renamed.assert_status(StatusCode::OK);
    let row = stored(&app, &legacy.id.to_string()).await;
    assert_eq!((row.name.as_str(), row.status.as_str(), row.colors), ("renamed", "retired", Some(json!(["pink"]))));
    let fixed = client.send(Method::PATCH, &uri, Some(&json!({"status": "draft", "colors": ["green"]}))).await;
    fixed.assert_status(StatusCode::OK);
    assert_eq!(stored(&app, &legacy.id.to_string()).await.status, "draft");
});

db_test!(hooks_actions_and_relations_keep_their_own_rules, |db: TestDb| async move {
    let (app, admin) = setup(db).await;
    let client = app.acting_as(&admin).await;
    // Hooks may write any value.
    let hooked = create(&client, &app, json!({"name": "hooked", "status": "draft"})).await;
    hooked.assert_status(StatusCode::CREATED);
    let key = hooked.json()["data"]["key"].as_str().unwrap().to_owned();
    let row = stored(&app, &key).await;
    assert_eq!((row.status.as_str(), row.colors), ("archived", Some(json!(["pink", "pink"]))));
    // So may action handlers.
    let plain = create(&client, &app, json!({"name": "plain", "status": "draft"})).await;
    let plain = plain.json()["data"]["key"].as_str().unwrap().to_owned();
    let run = json!({"keys": [plain], "data": {}});
    client.post_json(&app.admin_api("resources/swatches/actions/archive"), &run).await.assert_status(StatusCode::OK);
    assert_eq!(stored(&app, &plain).await.status, "archived");

    // A `belongs_to` key is checked against the relation, never static options.
    let palette = client.post_json(&app.admin_api("resources/palettes/records"), &json!({"name": "Warm"})).await.json();
    let palette = palette["data"]["key"].as_str().unwrap().to_owned();
    let linked = json!({"name": "linked", "status": "draft", "palette_id": palette.parse::<i32>().unwrap()});
    create(&client, &app, linked).await.assert_status(StatusCode::CREATED);
    let missing = create(&client, &app, json!({"name": "lost", "status": "draft", "palette_id": 999_999})).await;
    missing.assert_validation_error("palette_id");

    // Creates through a relation take the same path.
    let nested = app.admin_api(&format!("resources/palettes/records/{palette}/relations/swatches"));
    let refused = client.post_json(&nested, &json!({"name": "n", "status": "pink"})).await;
    assert_eq!(code(&refused, "status"), json!("not_option"));
    let refused = client.post_json(&nested, &json!({"name": "n", "status": "draft", "colors": ["red", "red"]})).await;
    assert_eq!(code(&refused, "colors"), json!("given_twice"));
    client.post_json(&nested, &json!({"name": "n", "status": "live"})).await.assert_status(StatusCode::CREATED);
});

/// The stored price of `key`, as the wire shows it.
async fn stored_price(app: &TestApp, key: &str) -> Option<String> {
    stored(app, key).await.price.map(|p| {
        let mut p = p.round_dp(2);
        p.rescale(2);
        p.to_string()
    })
}

db_test!(selects_store_the_option_they_matched, |db: TestDb| async move {
    let (app, admin) = setup(db).await;
    let client = app.acting_as(&admin).await;
    // A decimal rounded to the column's scale matches an option: that option is stored.
    for (name, price) in [("a", "10.504"), ("b", "10.505"), ("c", "10.5")] {
        let created = create(&client, &app, json!({"name": name, "status": "draft", "price": price})).await;
        created.assert_status(StatusCode::CREATED);
        let key = created.json()["data"]["key"].as_str().unwrap().to_owned();
        assert_eq!(stored_price(&app, &key).await.as_deref(), Some("10.50"), "{price}");
        let row = stored(&app, &key).await;
        assert_eq!(row.price.map(|p| p * rust_decimal::Decimal::from(1000)), Some(rust_decimal::Decimal::from(10_500)));
    }
    let created = create(&client, &app, json!({"name": "d", "status": "draft", "price": "20"})).await;
    let key = created.json()["data"]["key"].as_str().unwrap().to_owned();
    let uri = app.admin_api(&format!("resources/swatches/records/{key}"));
    client.send(Method::PATCH, &uri, Some(&json!({"price": "10.505"}))).await.assert_status(StatusCode::OK);
    let row = stored(&app, &key).await;
    assert_eq!(row.price.map(|p| p * rust_decimal::Decimal::from(1000)), Some(rust_decimal::Decimal::from(10_500)));

    // Every option reads back as itself: sent again, it is still accepted.
    let body = json!({
        "name": "e",
        "status": "draft",
        "price": "20.00",
        "weight": 0.25,
        "at": "2026-01-01T00:00:00.123456Z",
        "local_at": "2026-01-01T10:00:00.123456",
    });
    let created = create(&client, &app, body).await;
    created.assert_status(StatusCode::CREATED);
    let key = created.json()["data"]["key"].as_str().unwrap().to_owned();
    let uri = app.admin_api(&format!("resources/swatches/records/{key}"));
    let record = client.get(&uri).await.json()["data"]["data"].clone();
    assert_eq!(
        (&record["price"], &record["weight"], &record["at"], &record["local_at"]),
        (&json!("20.00"), &json!(0.25), &json!("2026-01-01T00:00:00.123456Z"), &json!("2026-01-01T10:00:00.123456")),
        "{record}"
    );
    let again = json!({
        "price": record["price"],
        "weight": record["weight"],
        "at": record["at"],
        "local_at": record["local_at"],
    });
    client.send(Method::PATCH, &uri, Some(&again)).await.assert_status(StatusCode::OK);
    let refused = client.send(Method::PATCH, &uri, Some(&json!({"weight": 0.3}))).await;
    assert_eq!(code(&refused, "weight"), json!("not_option"));
});

db_test!(the_schema_shows_the_canonical_options, |db: TestDb| async move {
    let (app, admin) = setup(db).await;
    let client = app.acting_as(&admin).await;
    let schema = client.get(&app.admin_api("resources/swatches/schema")).await.json();
    fn options(node: &Value, name: &str, out: &mut Vec<Value>) {
        match node {
            Value::Object(map) => {
                if map.get("name") == Some(&json!(name)) && map.contains_key("options") {
                    out.push(Value::Array(
                        map["options"].as_array().unwrap().iter().map(|o| o["value"].clone()).collect(),
                    ));
                }
                map.values().for_each(|v| options(v, name, out));
            }
            Value::Array(items) => items.iter().for_each(|v| options(v, name, out)),
            _ => {}
        }
    }
    let shown = |name: &str| {
        let mut out = Vec::new();
        options(&schema["form"], name, &mut out);
        out
    };
    assert_eq!(shown("serial"), [json!(["12", "7"])]);
    assert_eq!(shown("price"), [json!(["10.50", "20.00"])]);
    assert_eq!(shown("token"), [json!([TOKEN])]);
    assert_eq!(shown("at"), [json!(["2026-01-01T00:00:00Z", "2026-01-01T00:00:00.123456Z"])]);
    assert_eq!(shown("status"), [json!(["draft", "live"]), json!(["live", "draft"])]);
    assert_eq!(shown("colors"), [json!(["red", "green", "blue"])]);
});

db_test!(imports_store_the_option_they_matched, |db: TestDb| async move {
    let (app, admin) = setup(db).await;
    let client = app.acting_as(&admin).await;
    create(&client, &app, json!({"name": "kept", "status": "draft", "price": "20"}))
        .await
        .assert_status(StatusCode::CREATED);
    let csv = "name,status,price\r\nfresh,draft,10.504\r\nkept,draft,10.505\r\n";
    let request = Request::builder()
        .method(Method::POST)
        .uri(app.admin_api("resources/swatches/imports"))
        .header(header::CONTENT_TYPE, "text/csv")
        .body(Body::from(csv))
        .unwrap();
    let uploaded = client.request(request).await;
    uploaded.assert_status(StatusCode::CREATED);
    let id = uploaded.json()["id"].as_str().unwrap().to_owned();
    let mapping = json!({"mapping": {"name": "name", "status": "status", "price": "price"}});
    client.post_json(&app.admin_api(&format!("imports/{id}/run")), &mapping).await.assert_status(StatusCode::OK);
    rustcms_core::queue::run_until_idle(app.state(), None, 10).await.unwrap();
    let shown = client.get(&app.admin_api(&format!("imports/{id}"))).await.json();
    assert_eq!((shown["created"].clone(), shown["updated"].clone()), (json!(1), json!(1)), "{shown}");
    for row in swatch::Entity::find().all(app.state().db()).await.unwrap() {
        let price = row.price.map(|p| p * rust_decimal::Decimal::from(1000));
        assert_eq!(price, Some(rust_decimal::Decimal::from(10_500)), "{}", row.name);
    }
});

db_test!(imports_report_rows_out_of_the_options, |db: TestDb| async move {
    let (app, admin) = setup(db).await;
    let client = app.acting_as(&admin).await;
    create(&client, &app, json!({"name": "kept", "status": "draft"})).await.assert_status(StatusCode::CREATED);
    let csv = "name,status,colors\r\n\
               fresh,live,\"[\"\"red\"\"]\"\r\n\
               pinkish,pink,\r\n\
               kept,live,\"[\"\"red\"\",\"\"red\"\"]\"\r\n\
               kept,retired,\r\n\
               plain,draft,\r\n";
    let request = Request::builder()
        .method(Method::POST)
        .uri(app.admin_api("resources/swatches/imports"))
        .header(header::CONTENT_TYPE, "text/csv")
        .body(Body::from(csv))
        .unwrap();
    let uploaded = client.request(request).await;
    uploaded.assert_status(StatusCode::CREATED);
    let id = uploaded.json()["id"].as_str().unwrap().to_owned();
    let mapping = json!({"mapping": {"name": "name", "status": "status", "colors": "colors"}});
    let preview = client.post_json(&app.admin_api(&format!("imports/{id}/preview")), &mapping).await;
    preview.assert_status(StatusCode::OK);
    let preview = preview.json();
    assert_eq!((preview["rows"].clone(), preview["valid"].clone()), (json!(5), json!(2)), "{preview}");
    let run = client.post_json(&app.admin_api(&format!("imports/{id}/run")), &mapping).await;
    run.assert_status(StatusCode::OK);
    rustcms_core::queue::run_until_idle(app.state(), None, 10).await.unwrap();
    let shown = client.get(&app.admin_api(&format!("imports/{id}"))).await.json();
    assert_eq!(
        (shown["status"].clone(), shown["created"].clone(), shown["updated"].clone(), shown["failed"].clone()),
        (json!("done"), json!(2), json!(0), json!(3)),
        "{shown}"
    );
    let report: Vec<(Value, Value, Value)> = shown["report"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| (r["row"].clone(), r["field"].clone(), r["code"].clone()))
        .collect();
    // Rows are numbered as lines of the file (the header is line 1).
    assert_eq!(
        report,
        [
            (json!(3), json!("status"), json!("not_option")),
            (json!(4), json!("colors"), json!("given_twice")),
            (json!(5), json!("status"), json!("not_option")),
        ]
    );
    let kept = swatch::Entity::find().all(app.state().db()).await.unwrap();
    let kept = kept.iter().find(|s| s.name == "kept").unwrap();
    assert_eq!((kept.status.as_str(), &kept.colors), ("draft", &None), "the refused updates wrote nothing");
});

/// A resource on the swatches with `form`, to check its declaration at boot.
macro_rules! refused_at_boot {
    ($test:ident, $resource:ident, $message:literal, $form:expr) => {
        pub struct $resource;

        impl Resource for $resource {
            type Entity = swatch::Entity;
            const SLUG: &'static str = "swatches";

            fn meta() -> ResourceMeta {
                ResourceMeta::new("Swatch")
            }

            fn fields() -> Vec<Field> {
                swatch_fields()
            }

            fn form() -> Form {
                Form::new().schema($form)
            }
        }

        #[test]
        #[should_panic(expected = $message)]
        fn $test() {
            Registry::new().register::<$resource>();
        }
    };
}

refused_at_boot!(
    other_options_for_the_same_field,
    OtherOptions,
    "field `status` appears in several inputs that do not offer the same options",
    components![
        SelectInput::make("status").options([("draft", "Draft"), ("live", "Live")]),
        SelectInput::make("status").options([("draft", "Draft")]),
    ]
);

refused_at_boot!(
    a_free_input_beside_options,
    FreeInput,
    "field `status` appears in several inputs that do not offer the same options",
    components![TextInput::make("status"), SelectInput::make("status").options([("draft", "Draft")])]
);

refused_at_boot!(
    a_select_and_a_checkbox_list_of_one_field,
    SelectAndList,
    "field `colors` appears in several inputs that do not offer the same options",
    components![
        SelectInput::make("colors").options([("red", "Red")]),
        CheckboxList::make("colors").options([("red", "Red")]),
    ]
);

refused_at_boot!(
    an_option_of_another_kind,
    WrongKind,
    "option 12 of select `serial` is not a value of its column (i64)",
    components![SelectInput::make("serial").options([(12, "Twelve")])]
);

refused_at_boot!(
    an_option_stored_as_another_value,
    Lossy,
    "option \"10.555\" of select `price` would be stored as \"10.56\"",
    components![SelectInput::make("price").options([("10.555", "Odd")])]
);

refused_at_boot!(
    an_option_an_f32_does_not_keep,
    LossyFloat,
    "option 0.1 of select `weight` would be stored as 0.10000000149011612 (an f32 column)",
    components![SelectInput::make("weight").options([(0.1, "Tenth")])]
);

refused_at_boot!(
    an_instant_finer_than_microseconds,
    NanoInstant,
    "option \"2026-01-01T00:00:00.1234567Z\" of select `at` has a fraction of a second finer than microseconds",
    components![SelectInput::make("at").options([("2026-01-01T00:00:00.1234567Z", "Odd")])]
);

refused_at_boot!(
    a_local_time_finer_than_microseconds,
    NanoLocal,
    "of select `local_at` has a fraction of a second finer than microseconds",
    components![SelectInput::make("local_at").options([("2026-01-01T10:00:00.000000001", "Odd")])]
);

refused_at_boot!(
    a_decimal_over_its_precision,
    WideDecimal,
    "option \"100000000.00\" of select `price` has more than 10 digits",
    components![SelectInput::make("price").options([("99999999.99", "Most"), ("100000000.00", "Too much")])]
);

refused_at_boot!(
    a_checkbox_list_on_a_text_column,
    TextList,
    "checkbox list `status` needs a JSON column",
    components![CheckboxList::make("status").options([("draft", "Draft")])]
);

pub struct SecretOptions;

impl Resource for SecretOptions {
    type Entity = vault::Entity;
    const SLUG: &'static str = "vaults";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Vault")
    }

    fn fields() -> Vec<Field> {
        vec![Field::column(vault::Column::Token).encrypted()]
    }

    fn form() -> Form {
        Form::new().schema(components![SelectInput::make("token").options([("a", "A")])])
    }
}

#[test]
#[should_panic(expected = "input `token` must be a SecretInput")]
fn encrypted_fields_have_no_options() {
    Registry::new().register::<SecretOptions>();
}

/// A vault whose form is `form`, to check encrypted inputs at boot.
macro_rules! vault_refused_at_boot {
    ($test:ident, $resource:ident, $message:literal, $form:expr) => {
        pub struct $resource;

        impl Resource for $resource {
            type Entity = vault::Entity;
            const SLUG: &'static str = "vaults";

            fn meta() -> ResourceMeta {
                ResourceMeta::new("Vault")
            }

            fn fields() -> Vec<Field> {
                vec![Field::column(vault::Column::Token).encrypted(), Field::column(vault::Column::Env).encrypted_map()]
            }

            fn form() -> Form {
                Form::new().schema($form)
            }
        }

        #[test]
        #[should_panic(expected = $message)]
        fn $test() {
            Registry::new().register::<$resource>();
        }
    };
}

vault_refused_at_boot!(
    a_secret_input_with_options,
    SecretInputOptions,
    "encrypted field `token` takes no options",
    components![SecretInput::make("token").options([("a", "A")]), SecretMapInput::make("env")]
);

vault_refused_at_boot!(
    a_secret_map_input_with_options,
    SecretMapOptions,
    "encrypted field `env` takes no options",
    components![SecretInput::make("token"), SecretMapInput::make("env").options([("a", "A")])]
);
