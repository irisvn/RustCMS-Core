//! Contract fixtures covering the whole vocabulary and every wire kind
//! (`fixtures/contract/kitchen-sink.*.json`), shared with the admin SPA tests.
//! Regenerate with `UPDATE_CONTRACT=1 cargo test -p rustcms-admin --test contract`.

mod support;

use std::sync::Arc;

use rustcms_admin::*;
use rustcms_testing::{TestDb, db_test};
use sea_orm::{ConnectionTrait, Schema};
use serde_json::json;
use support::{CategoryResource, admin, category};

pub mod gadget {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "gadgets")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub name: String,
        pub notes: Option<String>,
        pub count: i32,
        pub serial: i64,
        pub ratio: f64,
        #[sea_orm(column_type = "Decimal(Some((10, 2)))")]
        pub price: Decimal,
        #[sea_orm(default_value = true)]
        pub active: bool,
        pub agreed: bool,
        #[sea_orm(default_value = "[\"red\"]")]
        pub colors: Json,
        pub status: String,
        pub category_id: Option<i32>,
        pub day: Date,
        pub local_at: DateTime,
        pub happened_at: DateTimeUtc,
        pub token: Uuid,
        #[sea_orm(default_value = "factory-secret")]
        pub secret: String,
        pub deleted_at: Option<DateTimeUtc>,
        pub specs: Option<Json>,
        pub photo: Option<Uuid>,
        pub accent: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub api_token: Option<rustcms_core::crypto::Encrypted<String>>,
        #[sea_orm(column_type = "Text", nullable)]
        pub env: Option<rustcms_core::crypto::Encrypted<std::collections::BTreeMap<String, String>>>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub struct GadgetResource;

impl Resource for GadgetResource {
    type Entity = gadget::Entity;
    const SLUG: &'static str = "gadgets";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Gadget").title("name").icon("box").group("Catalog").soft_deletes()
    }

    fn fields() -> Vec<Field> {
        use gadget::Column as C;
        vec![
            Field::column(C::Name).required().searchable().sortable().max_length(60),
            Field::column(C::Notes),
            Field::column(C::Count).sortable(),
            Field::column(C::Serial),
            Field::column(C::Ratio),
            Field::column(C::Price),
            Field::column(C::Active).filterable(),
            Field::column(C::Agreed).required(),
            Field::column(C::Colors),
            Field::column(C::Status).filterable(),
            Field::column(C::CategoryId).belongs_to("categories").filterable(),
            Field::column(C::Day).filterable(),
            Field::column(C::LocalAt),
            Field::column(C::HappenedAt),
            Field::column(C::Token),
            // Write-only: never read back.
            Field::column(C::Secret).readable(|_| false),
            Field::column(C::DeletedAt).guarded().readable(|_| true),
            Field::column(C::Specs).max_items(5).max_length(40),
            Field::column(C::Photo).media().accept(&["image/*"]).max_size(1_000_000),
            Field::column(C::Accent).max_length(7),
            Field::column(C::ApiToken).encrypted(),
            Field::column(C::Env).encrypted_map().max_items(10),
        ]
    }

    fn form() -> Form {
        // A split (vocabulary 11): the main column, and an aside with a
        // compact section that folds; a full-width page.
        Form::new().columns(2).width(FormWidth::Full).schema(components![
            Split::make()
                .main(components![
                    Section::make("Basics").columns(2).schema(components![
                        TextInput::make("name")
                            .placeholder("Name")
                            .help("Shown in lists")
                            .prefix("/gadgets/")
                            .suffix(".html"),
                        Textarea::make("notes").column_span(2),
                    ]),
                    Grid::make(3).schema(components![
                        NumberInput::make("count"),
                        NumberInput::make("serial"),
                        NumberInput::make("ratio"),
                        NumberInput::make("price"),
                    ]),
                    CheckboxList::make("colors").options([("red", "Red"), ("green", "Green"), ("blue", "Blue")]),
                    SelectInput::make("status").options([("draft", "Draft"), ("live", "Live")]),
                    SelectInput::make("category_id"),
                    Tabs::make().schema(components![
                        Tab::make("Schedule").columns(2).schema(components![
                            DatePicker::make("day"),
                            DateTimePicker::make("local_at"),
                            DateTimePicker::make("happened_at"),
                        ]),
                        Tab::make("Specs").schema(components![
                            KeyValue::make("specs"),
                            FileUpload::make("photo"),
                            CustomInput::make("accent", "acme/color").props(json!({"palette": ["#ff0000", "#00ff00"]})),
                        ]),
                    ]),
                    Hidden::make("token"),
                    TextInput::make("secret").password(),
                    SecretInput::make("api_token"),
                    SecretMapInput::make("env"),
                ])
                .aside(components![
                    Section::make("Flags")
                        .compact()
                        .collapsible(false)
                        .schema(components![Toggle::make("active"), Checkbox::make("agreed").label("Terms agreed"),])
                ])
                .aside_first_on_mobile()
        ])
    }

    /// An action form with every kind the vocabulary has for it.
    fn actions() -> Vec<Action<gadget::Entity>> {
        vec![
            Action::<gadget::Entity>::new("schedule", |_, _, _, _| Box::pin(async { Ok(()) })).label("Schedule…").form(
                vec![
                    ActionField::datetime("at").label("Run at").required(),
                    ActionField::date("day"),
                    ActionField::text("note").max_length(20),
                ],
            ),
        ]
    }

    fn table() -> Table {
        Table::new()
            .columns(vec![
                TextColumn::make("name"),
                BadgeColumn::make("status").color("draft", "gray").color("live", "success"),
                BooleanColumn::make("active"),
                DateColumn::make("day"),
                DateColumn::make("happened_at"),
                TextColumn::make("category_id"),
                TextColumn::make("price").cell("acme/money").props(serde_json::json!({"currency": "VND"})),
                TextColumn::make("serial"),
            ])
            .filters(vec![
                TernaryFilter::make("active"),
                SelectFilter::make("status").options([("draft", "Draft"), ("live", "Live")]),
                SelectFilter::make("category_id"),
                DateRangeFilter::make("day"),
            ])
            .default_sort("name")
    }
}

async fn registry(db: &TestDb) -> Arc<Registry> {
    let schema = Schema::new(db.conn().get_database_backend());
    for statement in [
        schema.create_table_from_entity(category::Entity),
        schema.create_table_from_entity(gadget::Entity),
        schema.create_table_from_entity(rustcms_admin::media::entity::Entity),
        schema.create_table_from_entity(rustcms_core::models::audit_log::Entity).if_not_exists().to_owned(),
        schema.create_table_from_entity(support::note::Entity),
    ] {
        if db.backend() == rustcms_testing::Backend::Mysql {
            // MySQL only takes an expression default on a JSON column.
            let sql = statement.to_string(sea_orm::sea_query::MysqlQueryBuilder);
            let sql = sql
                .replace("DEFAULT '[\\\"red\\\"]'", "DEFAULT ('[\\\"red\\\"]')")
                .replace(" timestamp", " datetime(6)");
            db.conn().execute_unprepared(&sql).await.unwrap();
            continue;
        }
        db.conn().execute(&statement).await.unwrap();
    }
    Registry::new()
        .register::<CategoryResource>()
        .register::<GadgetResource>()
        .register::<rustcms_admin::media::MediaResource>()
        .register::<support::NoteResource>()
        .finish()
}

db_test!(kitchen_sink_contract, |db: TestDb| async move {
    let registry = registry(&db).await;
    let ctx = Ctx::new(db.conn().clone(), admin(), registry.clone());
    let categories = registry.get("categories").unwrap();
    let gadgets = registry.get("gadgets").unwrap();
    let category =
        categories.create(&ctx, json!({"name": "Tools", "slug": "tools"}).as_object().unwrap().clone()).await.unwrap();
    let input = json!({
        "name": "Widget",
        "notes": "Line one\nLine two",
        "count": 3,
        "serial": "9007199254740993",
        "ratio": 0.5,
        "price": "10.5",
        "active": true,
        "agreed": false,
        "colors": ["red", "blue"],
        "status": "live",
        "category_id": category.key.parse::<i32>().unwrap(),
        "day": "2026-10-12",
        "local_at": "2026-10-12T09:30:00",
        "happened_at": "2026-10-12T09:30:00+07:00",
        "token": "0199d3a4-1b2c-7d3e-8f40-123456789abc",
        "secret": "hunter2",
        "specs": {"size": "M", "material": "steel"},
    });
    let created = gadgets.create(&ctx, input.as_object().unwrap().clone()).await.unwrap();

    let create = gadgets.schema(&ctx, SchemaOperation::Create, None).await.unwrap();
    let edit = gadgets.schema(&ctx, SchemaOperation::Edit, Some(&created.key)).await.unwrap();
    let view = gadgets.schema(&ctx, SchemaOperation::View, None).await.unwrap();
    let page = gadgets.list(&ctx, &ListQuery::default()).await.unwrap();
    // JSON defaults are not announced (their text is ambiguous); an untouched
    // field is omitted and the database default applies.
    let colors = create.form.components.iter().find_map(|c| match c {
        rustcms_admin::schema::ComponentSchema::Input(i) if i.name == "colors" => i.default.clone(),
        _ => None,
    });
    assert_eq!(colors, None);
    let mut minimal = input.as_object().unwrap().clone();
    minimal.remove("colors");
    minimal.insert("name".into(), json!("Defaults"));
    let stored = gadgets.create(&ctx, minimal).await.unwrap();
    assert_eq!(stored.data["colors"], json!(["red"]));
    gadgets.delete(&ctx, &stored.key).await.unwrap();
    let trash = gadgets.list(&ctx, &ListQuery { trashed: rustcms_admin::Trashed::Only, ..Default::default() }).await;
    let mut trash = serde_json::to_value(trash.unwrap()).unwrap();
    // The deletion time varies: the contract keeps its shape.
    trash["data"][0]["data"]["deleted_at"] = json!("2026-10-12T09:30:00Z");
    // A write-only field's declared default is data the user may not read.
    fn find(
        components: &[rustcms_admin::schema::ComponentSchema],
        name: &str,
    ) -> Option<rustcms_admin::schema::InputSchema> {
        components.iter().find_map(|c| match c {
            rustcms_admin::schema::ComponentSchema::Input(i) if i.name == name => Some(i.clone()),
            rustcms_admin::schema::ComponentSchema::Input(_) => None,
            rustcms_admin::schema::ComponentSchema::Layout(l) => find(&l.components, name),
        })
    }
    let secret = find(&create.form.components, "secret");
    let secret = secret.expect("secret input");
    assert!(secret.write_only && secret.default.is_none(), "{secret:?}");
    assert!(!serde_json::to_string(&create).unwrap().contains("factory-secret"));
    // Every vocabulary member appears.
    let text = serde_json::to_string(&create).unwrap();
    for kind in [
        "text_input",
        "textarea",
        "number_input",
        "select",
        "toggle",
        "checkbox",
        "checkbox_list",
        "date_picker",
        "date_time_picker",
        "hidden",
        "key_value",
        "file_upload",
        "custom",
        "section",
        "grid",
        "tabs",
        "tab",
        "badge",
        "boolean",
        "date",
        "ternary",
        "date_range",
    ] {
        assert!(text.contains(&format!("\"type\":\"{kind}\"")), "{kind} missing");
    }
    if db.backend() == rustcms_testing::Backend::Sqlite {
        // One copy is enough (the backends render the same contract; checked below).
        support::contract("kitchen-sink.schema.create.json", &serde_json::to_value(&create).unwrap());
        support::contract("kitchen-sink.schema.edit.json", &serde_json::to_value(&edit).unwrap());
        support::contract("kitchen-sink.schema.view.json", &serde_json::to_value(&view).unwrap());
        support::contract("kitchen-sink.records.json", &serde_json::to_value(&page).unwrap());
        support::contract("kitchen-sink.trash.json", &trash);
    } else {
        support::contract_matches("kitchen-sink.records.json", &serde_json::to_value(&page).unwrap());
    }
});
