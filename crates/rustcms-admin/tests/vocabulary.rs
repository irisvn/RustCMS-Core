//! Vocabulary 2: key/value inputs (validated on every write) and tabs.

mod support;

use rustcms_admin::{
    Ctx, Data, Field, Form, KeyValue, Registry, Resource, ResourceMeta, SchemaOperation, Tab, Tabs, TextInput,
    components,
};
use rustcms_core::AppError;
use rustcms_testing::{TestDb, db_test};
use serde_json::{Value, json};
use support::admin;

pub mod item {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "items")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i32,
        pub name: String,
        pub specs: Option<Json>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

struct Items;

#[async_trait::async_trait]
impl Resource for Items {
    type Entity = item::Entity;
    const SLUG: &'static str = "items";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Item")
    }

    fn fields() -> Vec<Field> {
        vec![Field::make("name"), Field::make("specs").max_items(2).max_length(5)]
    }

    fn form() -> Form {
        Form::new().schema(components![Tabs::make().schema(components![
            Tab::make("Main").schema(components![TextInput::make("name")]),
            Tab::make("Specs & sizes").schema(components![KeyValue::make("specs")]),
        ])])
    }

    async fn mutate_before_create(
        _ctx: &Ctx,
        _txn: &sea_orm::DatabaseTransaction,
        mut data: Data,
    ) -> rustcms_core::AppResult<Data> {
        if data.get("name") == Some(&json!("hook")) {
            data.insert("specs".into(), json!({"a": "too long value"}));
        }
        Ok(data)
    }
}

fn data(value: Value) -> Data {
    value.as_object().expect("object").clone()
}

async fn items(db: &TestDb) -> (Ctx, std::sync::Arc<dyn rustcms_admin::DynResource>) {
    use sea_orm::Schema;
    let backend = db.conn().get_database_backend();
    rustcms_testing::create_table(db.conn(), &Schema::new(backend).create_table_from_entity(item::Entity)).await;
    rustcms_testing::create_table(
        db.conn(),
        &Schema::new(backend)
            .create_table_from_entity(rustcms_core::models::audit_log::Entity)
            .if_not_exists()
            .to_owned(),
    )
    .await;
    let registry = Registry::new().register::<Items>().finish();
    let ctx = Ctx::new(db.conn().clone(), admin(), registry.clone());
    (ctx, registry.get("items").unwrap().clone())
}

fn messages(err: AppError) -> Vec<(String, Vec<String>)> {
    match err {
        AppError::Validation(v) => v.errors.into_iter().collect(),
        other => panic!("expected validation error, got {other:?}"),
    }
}

db_test!(key_value_inputs_hold_bounded_text_pairs, |db: TestDb| async move {
    let (ctx, items) = items(&db).await;
    let ok = items.create(&ctx, data(json!({"name": "a", "specs": {"size": "M", "ü": "ok"}}))).await.unwrap();
    assert_eq!(ok.data["specs"], json!({"size": "M", "ü": "ok"}));
    items.create(&ctx, data(json!({"name": "b", "specs": {}}))).await.unwrap();
    items.create(&ctx, data(json!({"name": "c", "specs": null}))).await.unwrap();
    // Five characters, not bytes.
    items.create(&ctx, data(json!({"name": "d", "specs": {"k": "üüüüü"}}))).await.unwrap();

    for (specs, message) in [
        (json!(["a"]), "must be an object of text values"),
        (json!({"a": 1}), "must be an object of text values"),
        (json!({"a": "1", "b": "2", "c": "3"}), "may not have more than 2 items"),
        (json!({" ": "x"}), "may not have an empty key"),
        (json!({"a": "123456"}), "may not have values longer than 5 characters"),
        (json!({"k".repeat(256): "x"}), "may not have keys longer than 255 characters"),
    ] {
        let err = items.create(&ctx, data(json!({"name": "x", "specs": specs}))).await.unwrap_err();
        assert_eq!(messages(err), [("specs".to_owned(), vec![message.to_owned()])], "{message}");
    }
    // Values written by hooks are held to the same bounds.
    let err = items.create(&ctx, data(json!({"name": "hook"}))).await.unwrap_err();
    assert_eq!(messages(err)[0].0, "specs");
});

db_test!(tabs_and_key_values_in_the_schema, |db: TestDb| async move {
    let (ctx, items) = items(&db).await;
    let schema = serde_json::to_value(items.schema(&ctx, SchemaOperation::Create, None).await.unwrap()).unwrap();
    assert_eq!(schema["vocabularyVersion"], json!(4));
    let tabs = &schema["form"]["components"][0];
    assert_eq!(tabs["type"], json!("tabs"));
    let ids: Vec<&Value> = tabs["components"].as_array().unwrap().iter().map(|t| &t["id"]).collect();
    assert_eq!(ids, [&json!("main"), &json!("specs-sizes")]);
    let specs = &tabs["components"][1]["components"][0];
    assert_eq!(specs["type"], json!("key_value"));
    assert_eq!(specs["rules"], json!([{"rule": "max_items", "value": 2}, {"rule": "max_length", "value": 5}]));
});

struct NonJsonKeyValue;

impl Resource for NonJsonKeyValue {
    type Entity = item::Entity;
    const SLUG: &'static str = "non_json";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Item")
    }

    fn fields() -> Vec<Field> {
        vec![Field::make("name")]
    }

    fn form() -> Form {
        Form::new().schema(components![KeyValue::make("name")])
    }
}

#[test]
#[should_panic(expected = "needs a JSON column")]
fn key_values_need_a_json_column() {
    let _ = Registry::new().register::<NonJsonKeyValue>();
}

struct LooseTab;

impl Resource for LooseTab {
    type Entity = item::Entity;
    const SLUG: &'static str = "loose_tab";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Item")
    }

    fn fields() -> Vec<Field> {
        vec![Field::make("name")]
    }

    fn form() -> Form {
        Form::new().schema(components![Tab::make("Alone").schema(components![TextInput::make("name")])])
    }
}

#[test]
#[should_panic(expected = "belongs directly in `Tabs`")]
fn tabs_contain_only_tabs() {
    let _ = Registry::new().register::<LooseTab>();
}

struct TwinTabs;

impl Resource for TwinTabs {
    type Entity = item::Entity;
    const SLUG: &'static str = "twin_tabs";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Item")
    }

    fn fields() -> Vec<Field> {
        vec![Field::make("name")]
    }

    fn form() -> Form {
        Form::new().schema(components![
            Tabs::make()
                .schema(
                    components![Tab::make("Main").schema(components![TextInput::make("name")]), Tab::make("main"),]
                )
        ])
    }
}

#[test]
#[should_panic(expected = "declared twice")]
fn tab_ids_are_unique() {
    let _ = Registry::new().register::<TwinTabs>();
}

struct ApiOnlyKeyValue;

impl Resource for ApiOnlyKeyValue {
    type Entity = item::Entity;
    const SLUG: &'static str = "api_only";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Item")
    }

    fn fields() -> Vec<Field> {
        vec![Field::make("name"), Field::make("specs").max_items(1)]
    }

    fn form() -> Form {
        Form::new().schema(components![TextInput::make("name")])
    }
}

db_test!(key_value_bounds_do_not_depend_on_the_form, |db: TestDb| async move {
    use sea_orm::Schema;
    let backend = db.conn().get_database_backend();
    rustcms_testing::create_table(db.conn(), &Schema::new(backend).create_table_from_entity(item::Entity)).await;
    rustcms_testing::create_table(
        db.conn(),
        &Schema::new(backend)
            .create_table_from_entity(rustcms_core::models::audit_log::Entity)
            .if_not_exists()
            .to_owned(),
    )
    .await;
    let registry = Registry::new().register::<ApiOnlyKeyValue>().finish();
    let ctx = Ctx::new(db.conn().clone(), admin(), registry.clone());
    let items = registry.get("api_only").unwrap();
    let err = items.create(&ctx, data(json!({"name": "x", "specs": {"a": "1", "b": "2"}}))).await.unwrap_err();
    assert_eq!(messages(err)[0].0, "specs");
    let err = items.create(&ctx, data(json!({"name": "x", "specs": {"a": 1}}))).await.unwrap_err();
    assert_eq!(messages(err)[0].0, "specs");
    items.create(&ctx, data(json!({"name": "x", "specs": {"a": "1"}}))).await.unwrap();
});

struct BadWidget;

impl Resource for BadWidget {
    type Entity = item::Entity;
    const SLUG: &'static str = "bad_widget";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Item")
    }

    fn fields() -> Vec<Field> {
        vec![Field::make("name")]
    }

    fn form() -> Form {
        Form::new().schema(components![rustcms_admin::CustomInput::make("name", "Acme/Color")])
    }
}

#[test]
#[should_panic(expected = "invalid widget name")]
fn widget_names_are_checked() {
    let _ = Registry::new().register::<BadWidget>();
}

struct HeavyProps;

impl Resource for HeavyProps {
    type Entity = item::Entity;
    const SLUG: &'static str = "heavy_props";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Item")
    }

    fn fields() -> Vec<Field> {
        vec![Field::make("name")]
    }

    fn form() -> Form {
        let props = json!({"blob": "x".repeat(5000)});
        Form::new().schema(components![rustcms_admin::CustomInput::make("name", "acme/color").props(props)])
    }
}

#[test]
#[should_panic(expected = "exceed 4 KiB")]
fn widget_props_are_bounded() {
    let _ = Registry::new().register::<HeavyProps>();
}

pub mod defaulted {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "defaulted")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i32,
        /// A JSON default: never shown as a form default, but still stored.
        #[sea_orm(default_value = "{\"type\":\"doc\",\"content\":[]}")]
        pub body: Option<Json>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

struct DefaultedRichText;

impl Resource for DefaultedRichText {
    type Entity = defaulted::Entity;
    const SLUG: &'static str = "defaulted";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Defaulted")
    }

    fn fields() -> Vec<Field> {
        vec![Field::make("body").rich_text()]
    }
}

#[test]
#[should_panic(expected = "may not have a column default")]
fn rich_text_columns_have_no_default_of_any_kind() {
    let _ = Registry::new().register::<DefaultedRichText>();
}
