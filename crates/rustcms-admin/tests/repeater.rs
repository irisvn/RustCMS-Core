//! JSON repeaters (ADR 0040): rows checked like forms with the rules their
//! inputs declare, errors at `name.index.input`, inputs hidden by their
//! conditions never stored, nesting, settings pages, and repeaters that
//! cannot hold refusing to boot. SQLite, PostgreSQL, MySQL.

use axum::http::{Method, StatusCode};
use rustcms_admin::*;
use rustcms_auth::{ADMIN_GUARD, Actor, AuthProvider, AuthState, RbacService};
use rustcms_core::settings::Settings;
use rustcms_core::{AppBuilder, Config};
use rustcms_testing::{Factory, TestApp, TestClient, TestDb, UserFactory, db_test};
use sea_orm::Schema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

mod support;

pub mod product {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "products")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i32,
        pub name: String,
        pub faq: Option<Json>,
        pub price: Option<i32>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

fn product_fields() -> Vec<Field> {
    use product::Column as C;
    vec![Field::column(C::Name).required(), Field::column(C::Faq), Field::column(C::Price)]
}

fn faq() -> rustcms_admin::ui::Input {
    Repeater::make("faq")
        .schema(components![
            TextInput::make("question").required().max_length(20),
            Textarea::make("answer"),
            SelectInput::make("kind").options([("text", "Text"), ("link", "Link")]),
            TextInput::make("url").shown_when(when("kind").eq("link")),
            NumberInput::make("votes").integer().min(0),
            Toggle::make("pinned"),
            Repeater::make("links").schema(components![TextInput::make("href").required()]).max_items(2),
        ])
        .min_items(1)
        .max_items(3)
        .reorderable()
        .collapsible()
        .item_label("{question}")
        .add_action_label("Add a question")
        .cloneable()
}

pub struct ProductResource;

impl Resource for ProductResource {
    type Entity = product::Entity;
    const SLUG: &'static str = "products";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Product").title("name").exportable().importable()
    }

    fn fields() -> Vec<Field> {
        product_fields()
    }

    fn form() -> Form {
        Form::new().schema(components![TextInput::make("name"), faq(), NumberInput::make("price")])
    }

    fn table() -> Table {
        Table::new().columns(vec![ui::TextColumn::make("name"), ui::TextColumn::make("faq").label("Questions").count()])
    }

    fn infolist() -> Infolist {
        Infolist::new().schema(components![
            TextEntry::make("name"),
            RepeatableEntry::make("faq").label("Questions").schema(components![
                TextEntry::make("question"),
                BooleanEntry::make("pinned").labels("Pinned", "Not pinned"),
                RepeatableEntry::make("links").schema(components![TextEntry::make("href").url()]),
            ]),
        ])
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SocialSettings {
    pub links: Vec<SocialLink>,
    /// Whether links take a note (rows read it: `when_root`).
    pub notes: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SocialLink {
    pub label: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl Settings for SocialSettings {
    const GROUP: &'static str = "social";
}

#[async_trait::async_trait]
impl SettingsPage for SocialSettings {
    fn meta() -> SettingsMeta {
        SettingsMeta::new("Social")
    }

    fn fields() -> Vec<SettingField> {
        vec![
            SettingField::repeater(
                Repeater::make("links")
                    .schema(components![
                        TextInput::make("label").required(),
                        TextInput::make("url").required().max_length(30),
                        TextInput::make("note").shown_when(when_root("notes").is_true()),
                    ])
                    .max_items(2),
            ),
            SettingField::boolean("notes"),
        ]
    }
}

fn build(config: Config) -> AppBuilder {
    // Uploaded CSV files (imports) go to a temporary directory of this app.
    let root = std::env::temp_dir().join(format!("rustcms-repeater-{}", uuid::Uuid::now_v7()));
    let config = config.with("storage.root", root.to_string_lossy()).expect("storage config");
    let mut app = AppBuilder::new(config);
    app.provider(rustcms_core::storage::StorageProvider);
    app.provider(AuthProvider::new());
    app.provider(AdminProvider::new().resource::<ProductResource>().settings::<SocialSettings>());
    app
}

async fn setup(db: TestDb) -> (TestApp, rustcms_core::models::user::Model) {
    db.migrate::<rustcms_migration::Migrator>().await;
    let schema = Schema::new(db.conn().get_database_backend());
    rustcms_testing::create_table(db.conn(), &schema.create_table_from_entity(product::Entity)).await;
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

async fn create(client: &TestClient<'_>, app: &TestApp, faq: Value) -> rustcms_testing::TestResponse {
    client.post_json(&app.admin_api("resources/products/records"), &json!({"name": "Lamp", "faq": faq})).await
}

/// The field issues of a 422, as `{path: [{code, params}]}`.
fn errors(response: rustcms_testing::TestResponse) -> Value {
    response.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
    response.json()["issues"].clone()
}

db_test!(rows_are_stored_without_hidden_inputs, |db: TestDb| async move {
    let (app, admin) = setup(db).await;
    let client = app.acting_as(&admin).await;
    let response = create(
        &client,
        &app,
        json!([
            {"question": "Size?", "kind": "text", "url": "https://hidden", "votes": 3, "pinned": true},
            {"question": "Docs?", "kind": "link", "url": "https://docs", "links": [{"href": "/a"}]},
        ]),
    )
    .await;
    response.assert_status(StatusCode::CREATED);
    let stored = &response.json()["data"]["data"]["faq"];
    // The url of a text answer is hidden by its condition: never stored.
    assert_eq!(
        stored,
        &json!([
            {"question": "Size?", "kind": "text", "votes": 3, "pinned": true},
            {"question": "Docs?", "kind": "link", "url": "https://docs", "links": [{"href": "/a"}]},
        ])
    );
});

db_test!(each_row_is_checked_with_errors_at_its_path, |db: TestDb| async move {
    let (app, admin) = setup(db).await;
    let client = app.acting_as(&admin).await;
    let found = errors(
        create(
            &client,
            &app,
            json!([
                {"answer": "no question"},
                {"question": "A question longer than twenty", "kind": "other", "votes": 1.5, "extra": 1},
                {"question": "Nested", "links": [{"href": "/a"}, {}, {"href": "/c"}]},
            ]),
        )
        .await,
    );
    assert_eq!(found["faq.0.question"][0]["code"], json!("required"));
    assert_eq!(found["faq.1.question"][0]["code"], json!("max_length"));
    assert_eq!(found["faq.1.kind"][0]["code"], json!("not_option"));
    assert_eq!(found["faq.1.votes"][0]["code"], json!("integer"));
    assert_eq!(found["faq.1.extra"][0]["code"], json!("not_allowed"));
    assert_eq!(found["faq.2.links"][0]["code"], json!("max_items"));
    assert_eq!(found["faq.2.links.1.href"][0]["code"], json!("required"));

    // The list itself: its count, its shape; left out of a create, it has no rows.
    let omitted = client.post_json(&app.admin_api("resources/products/records"), &json!({"name": "Lamp"})).await;
    assert_eq!(errors(omitted)["faq"][0]["code"], json!("min_items"));
    assert_eq!(errors(create(&client, &app, Value::Null).await)["faq"][0]["code"], json!("min_items"));
    let found = errors(create(&client, &app, json!([])).await);
    assert_eq!(found["faq"][0]["code"], json!("min_items"));
    assert_eq!(found["faq"][0]["params"]["min"], json!("1"));
    let four = json!([{"question": "a"}, {"question": "b"}, {"question": "c"}, {"question": "d"}]);
    assert_eq!(errors(create(&client, &app, four).await)["faq"][0]["code"], json!("max_items"));
    assert_eq!(errors(create(&client, &app, json!({"question": "a"})).await)["faq"][0]["code"], json!("invalid"));
    assert_eq!(errors(create(&client, &app, json!(["a"])).await)["faq.0"][0]["code"], json!("invalid"));
    // Bounds of a number input in a row.
    let negative = json!([{"question": "a", "votes": -1}]);
    assert_eq!(errors(create(&client, &app, negative).await)["faq.0.votes"][0]["code"], json!("min_value"));
});

db_test!(the_schema_describes_the_rows, |db: TestDb| async move {
    let sqlite = db.backend() == rustcms_testing::Backend::Sqlite;
    let (app, admin) = setup(db).await;
    let client = app.acting_as(&admin).await;
    let schema = client.get(&app.admin_api("resources/products/schema?operation=create")).await.json();
    if sqlite {
        // Shared with the SPA tests.
        support::contract("repeater.schema.create.json", &schema);
    }
    assert_eq!(schema["vocabularyVersion"], json!(24));
    let input = schema["form"]["components"].as_array().unwrap().iter().find(|c| c["name"] == "faq").unwrap().clone();
    assert_eq!(input["type"], json!("repeater"));
    assert_eq!(input["valueType"], json!("json"));
    let repeater = &input["repeater"];
    assert_eq!(repeater["minItems"], json!(1));
    assert_eq!(repeater["maxItems"], json!(3));
    assert_eq!(repeater["reorderable"], json!(true));
    assert_eq!(repeater["collapsible"], json!(true));
    assert_eq!(repeater["itemLabel"], json!("{question}"));
    assert_eq!(repeater["addActionLabel"], json!("Add a question"));
    assert_eq!(repeater["cloneable"], json!(true));
    assert_eq!((repeater["addable"].clone(), repeater["deletable"].clone()), (json!(true), json!(true)));
    let rows = repeater["components"].as_array().unwrap();
    let names: Vec<&str> = rows.iter().map(|c| c["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["question", "answer", "kind", "url", "votes", "pinned", "links"]);
    assert_eq!(rows[0]["rules"], json!([{"rule": "required"}, {"rule": "max_length", "value": 20}]));
    assert_eq!(rows[0]["valueType"], json!("string"));
    assert_eq!(rows[3]["shownWhen"], json!({"op": "eq", "name": "kind", "value": "link"}));
    assert_eq!((rows[4]["valueType"].clone(), rows[4]["integer"].clone()), (json!("f64"), json!(true)));
    assert_eq!(rows[5]["valueType"], json!("bool"));
    assert_eq!(rows[6]["repeater"]["maxItems"], json!(2));
    // A count column: the panel shows the number of rows.
    let column = schema["table"]["columns"].as_array().unwrap().iter().find(|c| c["name"] == "faq").unwrap().clone();
    assert_eq!((column["count"].clone(), column["label"].clone()), (json!(true), json!("Questions")));
});

db_test!(integer_inputs_are_stored_as_integers, |db: TestDb| async move {
    let (app, admin) = setup(db).await;
    let client = app.acting_as(&admin).await;
    // `3.0` is an integer, stored as `3` (a typed reader's `i32` refuses floats).
    let response = create(&client, &app, json!([{"question": "Size?", "votes": 3.0}])).await;
    response.assert_status(StatusCode::CREATED);
    let votes = response.json()["data"]["data"]["faq"][0]["votes"].clone();
    assert!(votes.is_i64() || votes.is_u64(), "{votes}");
    assert_eq!(votes, json!(3));
});

db_test!(csv_files_carry_rows_as_json_text, |db: TestDb| async move {
    let (app, admin) = setup(db).await;
    let client = app.acting_as(&admin).await;
    create(&client, &app, json!([{"question": "Size?", "kind": "text"}])).await.assert_status(StatusCode::CREATED);
    // Exports write the rows as their JSON text (one cell).
    let export = client.get(&app.admin_api("resources/products/export")).await;
    export.assert_status(StatusCode::OK);
    let text = export.text();
    let lines: Vec<&str> = text.split("\r\n").filter(|l| !l.is_empty()).collect();
    assert_eq!(lines[0], "name,faq,price");
    assert!(lines[1].starts_with("Lamp,\"[{") && lines[1].contains("\"\"question\"\":\"\"Size?\"\""), "{lines:?}");

    // Imports read that text back, and check each row as a form would.
    let csv = format!("{}\r\n{}\r\nBad,\"[{{\"\"answer\"\":\"\"x\"\"}}]\",\r\n", lines[0], lines[1]);
    let request = axum::http::Request::builder()
        .method(Method::POST)
        .uri(app.admin_api("resources/products/imports"))
        .header(axum::http::header::CONTENT_TYPE, "text/csv")
        .body(axum::body::Body::from(csv))
        .unwrap();
    let uploaded = client.request(request).await;
    uploaded.assert_status(StatusCode::CREATED);
    let id = uploaded.json()["id"].as_str().unwrap().to_owned();
    let mapping = json!({"mapping": {"name": "name", "faq": "faq"}});
    let preview = client.post_json(&app.admin_api(&format!("imports/{id}/preview")), &mapping).await.json();
    assert_eq!((preview["rows"].clone(), preview["valid"].clone()), (json!(2), json!(1)), "{preview}");
    assert_eq!(preview["report"][0]["row"], json!(3));
    assert_eq!(preview["report"][0]["field"], json!("faq.0.question"));
    assert_eq!(preview["report"][0]["code"], json!("required"));
});

db_test!(repeatable_entries_show_the_rows, |db: TestDb| async move {
    let (app, admin) = setup(db).await;
    let client = app.acting_as(&admin).await;
    let created = create(&client, &app, json!([{"question": "Size?"}])).await.json();
    let key = created["data"]["key"].as_str().unwrap().to_owned();
    let schema =
        client.get(&app.admin_api(&format!("resources/products/schema?operation=view&key={key}"))).await.json();
    let entry = &schema["infolist"]["components"][1];
    assert_eq!(entry["type"], json!("repeatable_entry"));
    assert_eq!(entry["label"], json!("Questions"));
    let rows = entry["components"].as_array().unwrap();
    assert_eq!(
        rows[0],
        json!({"type": "text_entry", "name": "question", "label": "Question", "valueType": "json", "columnSpan": 1})
    );
    assert_eq!(rows[1]["labels"], json!(["Pinned", "Not pinned"]));
    assert_eq!(rows[2]["components"][0]["url"], json!(true));
});

db_test!(settings_pages_take_json_repeaters, |db: TestDb| async move {
    let (app, admin) = setup(db).await;
    let client = app.acting_as(&admin).await;
    let url = app.admin_api("settings/social");
    let page = client.get(&url).await.json();
    assert_eq!(page["form"]["components"][0]["type"], json!("repeater"));
    assert_eq!(page["form"]["components"][0]["repeater"]["maxItems"], json!(2));
    let version = page["version"].as_str().unwrap().to_owned();
    client.fetch_csrf().await;
    let bad =
        json!({"version": version, "data": {"links": [{"label": "X", "url": "https://a-very-long-address.example"}]}});
    let refused = client.send(Method::PUT, &url, Some(&bad)).await;
    assert_eq!(errors(refused)["links.0.url"][0]["code"], json!("max_length"));
    let good = json!({"version": version, "data": {"links": [{"label": "X", "url": "https://x.test"}]}});
    client.send(Method::PUT, &url, Some(&good)).await.assert_status(StatusCode::OK);
    let saved = client.get(&url).await.json();
    assert_eq!(saved["data"]["links"], json!([{"label": "X", "url": "https://x.test"}]));
    // Rows read the page's fields (ADR 0044): a note only where the page takes notes.
    let version = saved["version"].as_str().unwrap().to_owned();
    let noted = json!([{"label": "X", "url": "https://x.test", "note": "n"}]);
    let sent = json!({"version": version, "data": {"links": noted}});
    client.send(Method::PUT, &url, Some(&sent)).await.assert_status(StatusCode::OK);
    let saved = client.get(&url).await.json();
    assert_eq!(saved["data"]["links"], json!([{"label": "X", "url": "https://x.test"}]));
    let version = saved["version"].as_str().unwrap().to_owned();
    let sent = json!({"version": version, "data": {"notes": true, "links": noted}});
    client.send(Method::PUT, &url, Some(&sent)).await.assert_status(StatusCode::OK);
    assert_eq!(client.get(&url).await.json()["data"]["links"], noted);
});

macro_rules! refused_at_boot {
    ($test:ident, $resource:ident, $message:literal, $form:expr) => {
        pub struct $resource;

        impl Resource for $resource {
            type Entity = product::Entity;
            const SLUG: &'static str = "products";

            fn meta() -> ResourceMeta {
                ResourceMeta::new("Product")
            }

            fn fields() -> Vec<Field> {
                product_fields()
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

fn rows(components: Vec<rustcms_admin::ui::Component>) -> Vec<rustcms_admin::ui::Component> {
    components![Repeater::make("faq").schema(components)]
}

refused_at_boot!(an_empty_row, EmptyRow, "the row has no input", components![Repeater::make("faq")]);
refused_at_boot!(
    a_section_in_a_table_row,
    TableSection,
    "table rows hold inputs only",
    components![
        Repeater::make("faq").table().schema(components![Section::make("S").schema(components![TextInput::make("q")])])
    ]
);
refused_at_boot!(
    a_repeater_in_a_table_row,
    TableNested,
    "table rows hold no nested repeater",
    components![
        Repeater::make("faq")
            .table()
            .schema(components![Repeater::make("links").schema(components![TextInput::make("url")])])
    ]
);
refused_at_boot!(
    folding_table_rows,
    TableFolded,
    "table rows do not fold",
    components![Repeater::make("faq").table().collapsible().schema(components![TextInput::make("q")])]
);
refused_at_boot!(
    a_row_reading_no_form_input,
    RootUnknown,
    "uses `when_root(\"colour\")`, which is not an input of that level",
    components![
        Repeater::make("faq").schema(components![TextInput::make("q").shown_when(when_root("colour").filled())])
    ]
);
refused_at_boot!(
    a_media_input_in_a_row,
    MediaRow,
    "media and rich text inputs are not supported in JSON rows",
    rows(components![FileUpload::make("photo")])
);
refused_at_boot!(
    a_select_without_options,
    BareSelect,
    "a select in a row needs options",
    rows(components![SelectInput::make("category")])
);
refused_at_boot!(
    tabs_in_a_row,
    TabsRow,
    "a row holds sections and grids",
    rows(components![Tabs::make().schema(components![Tab::make("A").schema(components![TextInput::make("a")])])])
);
refused_at_boot!(
    an_input_twice,
    Twice,
    "input `a` appears twice in a row",
    rows(components![TextInput::make("a"), TextInput::make("a")])
);
refused_at_boot!(
    too_deep,
    TooDeep,
    "repeaters nest 3 deep at most",
    rows(components![Repeater::make("a").schema(components![
        Repeater::make("b").schema(components![Repeater::make("c").schema(components![TextInput::make("d")])])
    ])])
);
refused_at_boot!(
    an_unknown_item_label,
    UnknownLabel,
    "the item label names `title`",
    components![Repeater::make("faq").schema(components![TextInput::make("question")]).item_label("{title}")]
);
refused_at_boot!(
    min_above_max,
    MinAboveMax,
    "`min_items` is above `max_items`",
    components![Repeater::make("faq").schema(components![TextInput::make("q")]).min_items(3).max_items(2)]
);
refused_at_boot!(
    required_outside_rows,
    RequiredOutside,
    "are for inputs of JSON repeater rows",
    components![TextInput::make("name").required()]
);
refused_at_boot!(
    a_repeater_on_a_text_column,
    TextColumn,
    "repeater `name` needs a JSON column",
    components![Repeater::make("name").schema(components![TextInput::make("q")])]
);
refused_at_boot!(
    a_condition_outside_the_row,
    OutsideCondition,
    "which is not an input (or entry) of the form",
    rows(components![TextInput::make("q").shown_when(when("price").filled())])
);
refused_at_boot!(
    schema_on_a_text_input,
    SchemaOnText,
    "repeater options (`schema`, `min_items`, …) are for repeaters",
    components![TextInput::make("name").max_items(2)]
);

/// A repeatable entry refused at boot.
macro_rules! entry_refused_at_boot {
    ($test:ident, $resource:ident, $message:literal, $infolist:expr) => {
        pub struct $resource;

        impl Resource for $resource {
            type Entity = product::Entity;
            const SLUG: &'static str = "products";

            fn meta() -> ResourceMeta {
                ResourceMeta::new("Product")
            }

            fn fields() -> Vec<Field> {
                product_fields()
            }

            fn infolist() -> Infolist {
                Infolist::new().schema($infolist)
            }
        }

        #[test]
        #[should_panic(expected = $message)]
        fn $test() {
            Registry::new().register::<$resource>();
        }
    };
}

entry_refused_at_boot!(
    a_repeatable_entry_on_text,
    RepeatableOnText,
    "a RepeatableEntry (JSON columns) does not fit",
    components![RepeatableEntry::make("name").schema(components![TextEntry::make("x")])]
);
entry_refused_at_boot!(
    a_repeatable_entry_without_rows,
    RepeatableEmpty,
    "the row has no entry",
    components![RepeatableEntry::make("faq")]
);
entry_refused_at_boot!(
    a_relation_entry_in_a_row,
    RelationInRow,
    "a row shows its keys with text, boolean",
    components![RepeatableEntry::make("faq").schema(components![RelationEntry::make("x")])]
);
entry_refused_at_boot!(
    schema_on_a_text_entry,
    SchemaOnTextEntry,
    "`schema` does not fit a TextEntry",
    components![TextEntry::make("name").schema(components![TextEntry::make("x")])]
);

/// A settings struct whose row needs a value its repeater may leave out.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StrictSettings {
    pub links: Vec<StrictLink>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrictLink {
    pub label: String,
    pub url: String,
}

impl Settings for StrictSettings {
    const GROUP: &'static str = "strict";
}

#[async_trait::async_trait]
impl SettingsPage for StrictSettings {
    fn meta() -> SettingsMeta {
        SettingsMeta::new("Strict")
    }

    fn fields() -> Vec<SettingField> {
        // `url` is optional in the form, but the struct needs it.
        vec![SettingField::repeater(
            Repeater::make("links").schema(components![TextInput::make("label").required(), TextInput::make("url")]),
        )]
    }
}

#[test]
#[should_panic(expected = "must read rows without their optional inputs")]
fn a_settings_row_struct_must_read_rows_without_optional_inputs() {
    let _ = AdminProvider::new().settings::<StrictSettings>();
}
