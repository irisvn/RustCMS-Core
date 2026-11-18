//! Option labels of table columns (vocabulary 20): declared with
//! `TableColumn::options`, else taken from the form's select or checkbox list
//! of the same field (never for `belongs_to` fields, nor with `raw()`),
//! translated as other labels; option and color values compared in canonical
//! wire form; declarations that cannot hold refuse to boot. SQLite,
//! PostgreSQL, MySQL.

use axum::http::StatusCode;
use rustcms_admin::*;
use rustcms_auth::{ADMIN_GUARD, AuthProvider, AuthState, RbacService};
use rustcms_core::{AppBuilder, Config};
use rustcms_testing::{Factory, TestApp, TestDb, UserFactory, db_test};
use sea_orm::Schema;
use serde_json::{Value, json};

pub mod team {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "teams")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i32,
        pub name: String,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod ticket {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "tickets")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i32,
        pub title: String,
        pub status: String,
        pub kind: Option<String>,
        pub tags: Option<Json>,
        pub level: Option<i64>,
        pub active: bool,
        pub team_id: Option<i32>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub struct TeamResource;

impl Resource for TeamResource {
    type Entity = team::Entity;
    const SLUG: &'static str = "teams";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Team").title("name")
    }

    fn fields() -> Vec<Field> {
        vec![Field::column(team::Column::Name).required()]
    }
}

fn ticket_fields() -> Vec<Field> {
    use ticket::Column as C;
    vec![
        Field::column(C::Title).required(),
        Field::column(C::Status).required(),
        Field::column(C::Kind),
        Field::column(C::Tags),
        Field::column(C::Level),
        Field::column(C::Active),
        Field::column(C::TeamId).belongs_to("teams"),
    ]
}

fn ticket_form() -> Form {
    Form::new().schema(components![
        TextInput::make("title"),
        Section::make("More").schema(components![
            SelectInput::make("status").options([("draft", "Draft"), ("live", "Live")]),
            SelectInput::make("kind").options([("bug", "Bug"), ("idea", "Idea")]),
            CheckboxList::make("tags").options([("red", "Red"), ("blue", "Blue")]),
        ]),
        SelectInput::make("level").options([("1", "Low"), ("2", "High")]),
        Toggle::make("active"),
        SelectInput::make("team_id"),
    ])
}

pub struct TicketResource;

impl Resource for TicketResource {
    type Entity = ticket::Entity;
    const SLUG: &'static str = "tickets";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Ticket").title("title")
    }

    fn fields() -> Vec<Field> {
        ticket_fields()
    }

    fn form() -> Form {
        ticket_form()
    }

    fn table() -> Table {
        Table::new().columns(vec![
            TextColumn::make("title"),
            // From the form's select (in a section), colors kept.
            BadgeColumn::make("status").color("live", "success"),
            // Kept as values.
            TextColumn::make("kind").raw(),
            // From the form's checkbox list (a JSON list).
            BadgeColumn::make("tags"),
            // Declared: over the form's, in other spellings (an i64 is a string).
            BadgeColumn::make("level").options([("01", "Minor"), ("2", "Major")]).color("1", "warning"),
            BooleanColumn::make("active"),
            // Related records by title: never the select's options.
            TextColumn::make("team_id"),
        ])
    }
}

const VI: &str = r#"
[tickets]
"Draft" = "Nháp"
"Minor" = "Nhỏ"
"#;

fn build(config: Config) -> AppBuilder {
    let mut app = AppBuilder::new(config);
    app.provider(AuthProvider::new());
    app.provider(
        AdminProvider::new().resource::<TeamResource>().resource::<TicketResource>().translations("vi", VI).dashboard(
            dashboard::Dashboard::new()
                .widget(dashboard::Widget::recent("latest_tickets", "tickets").columns(&["status", "kind", "level"])),
        ),
    );
    app
}

async fn setup(db: TestDb) -> (TestApp, rustcms_core::models::user::Model) {
    db.migrate::<rustcms_migration::Migrator>().await;
    let schema = Schema::new(db.conn().get_database_backend());
    for statement in [schema.create_table_from_entity(team::Entity), schema.create_table_from_entity(ticket::Entity)] {
        rustcms_testing::create_table(db.conn(), &statement).await;
    }
    let app = TestApp::new(build, db).await;
    let auth = app.state().service::<AuthState>().unwrap();
    let db = app.state().db();
    let rbac = RbacService::new(db, &auth.resolver);
    rbac.sync_permissions(&auth.permissions.all()).await.unwrap();
    let admin = UserFactory::new().name("Admin").create(db).await;
    let super_admin = rustcms_auth::entities::role::super_admin_id(db, ADMIN_GUARD).await.unwrap().unwrap();
    rbac.assign_role(rustcms_auth::Actor::System, admin.id, super_admin).await.unwrap();
    (app, admin)
}

/// The table column named `name` of a schema.
fn column<'a>(schema: &'a Value, name: &str) -> &'a Value {
    schema["table"]["columns"].as_array().unwrap().iter().find(|c| c["name"] == name).unwrap()
}

db_test!(columns_show_option_labels, |db: TestDb| async move {
    let (app, admin) = setup(db).await;
    let url = app.admin_api("resources/tickets/schema");
    let en = app.acting_as(&admin).await.languages("en");
    let response = en.get(&url).await;
    response.assert_status(StatusCode::OK);
    let schema = response.json();

    assert_eq!(
        column(&schema, "status")["options"],
        json!([{"value": "draft", "label": "Draft"}, {"value": "live", "label": "Live"}])
    );
    assert_eq!(column(&schema, "status")["colors"], json!([{"value": "live", "color": "success"}]));
    assert_eq!(
        column(&schema, "tags")["options"],
        json!([{"value": "red", "label": "Red"}, {"value": "blue", "label": "Blue"}])
    );
    // Declared options win; values and colors canonical ("01" → "1").
    assert_eq!(
        column(&schema, "level")["options"],
        json!([{"value": "1", "label": "Minor"}, {"value": "2", "label": "Major"}])
    );
    assert_eq!(column(&schema, "level")["colors"], json!([{"value": "1", "color": "warning"}]));
    for name in ["title", "kind", "active", "team_id"] {
        assert_eq!(column(&schema, name).get("options"), None, "column `{name}`");
    }

    // The dashboard's recent records label values as the table does: `raw()` too.
    let recent = en.get(&app.admin_api("dashboard/widgets/latest_tickets")).await.json();
    let options = |name: &str| {
        recent["columns"].as_array().unwrap().iter().find(|c| c["name"] == name).unwrap().get("options").cloned()
    };
    assert_eq!(options("status"), Some(column(&schema, "status")["options"].clone()));
    assert_eq!(options("level"), Some(column(&schema, "level")["options"].clone()));
    assert_eq!(options("kind"), None, "a raw column keeps the values");

    // Labels in the user's language; values never change.
    let vi = app.acting_as(&admin).await.languages("vi");
    let schema = vi.get(&url).await.json();
    assert_eq!(column(&schema, "status")["options"][0], json!({"value": "draft", "label": "Nháp"}));
    assert_eq!(column(&schema, "status")["options"][1], json!({"value": "live", "label": "Live"}));
    assert_eq!(column(&schema, "level")["options"][0], json!({"value": "1", "label": "Nhỏ"}));
});

#[test]
fn missing_translations_list_column_option_labels() {
    use rustcms_core::i18n::Translator;
    let registry = registry::Registry::new().register::<TeamResource>().register::<TicketResource>().finish();
    let mut translator = Translator::default();
    translator.add("vi", VI).unwrap();
    let panel = AdminPanel {
        brand: "x".into(),
        registry,
        translator: std::sync::Arc::new(translator),
        settings: Default::default(),
        custom: std::sync::Arc::new(custom::CustomPanel::empty("/admin")),
        dashboard: Default::default(),
    };
    let missing = panel.missing_translations("vi", &[]);
    assert!(missing.contains(&"Major".to_owned()), "{missing:?}");
    assert!(!missing.contains(&"Minor".to_owned()), "{missing:?}");
}

/// A ticket resource with `table`, to check its declaration at boot.
macro_rules! refused_at_boot {
    ($test:ident, $resource:ident, $message:literal, $columns:expr) => {
        pub struct $resource;

        impl Resource for $resource {
            type Entity = ticket::Entity;
            const SLUG: &'static str = "tickets";

            fn meta() -> ResourceMeta {
                ResourceMeta::new("Ticket")
            }

            fn fields() -> Vec<Field> {
                ticket_fields()
            }

            fn form() -> Form {
                ticket_form()
            }

            fn table() -> Table {
                Table::new().columns($columns)
            }
        }

        #[test]
        #[should_panic(expected = $message)]
        fn $test() {
            registry::Registry::new().register::<$resource>();
        }
    };
}

refused_at_boot!(
    options_on_a_boolean_column,
    BooleanOptions,
    "column `active`: options need a text or badge column",
    vec![BooleanColumn::make("active").options([(true, "On")])]
);

refused_at_boot!(
    raw_on_a_boolean_column,
    RawBoolean,
    "column `active`: raw() needs a text or badge column",
    vec![BooleanColumn::make("active").raw()]
);

refused_at_boot!(
    options_on_a_count_column,
    CountOptions,
    "count column `tags` takes no options",
    vec![TextColumn::make("tags").count().options([("red", "Red")])]
);

refused_at_boot!(
    options_on_a_related_record,
    RelationOptions,
    "column `team_id` shows related records by their title and takes no options",
    vec![TextColumn::make("team_id").options([(1, "One")])]
);

refused_at_boot!(
    options_and_raw,
    OptionsAndRaw,
    "column `status` takes options or raw(), not both",
    vec![TextColumn::make("status").options([("draft", "Draft")]).raw()]
);

refused_at_boot!(
    an_option_of_another_kind,
    WrongKind,
    "option 1 of column `level` is not a value of its column (i64)",
    vec![TextColumn::make("level").options([(1, "One")])]
);

refused_at_boot!(
    a_color_of_another_kind,
    WrongColor,
    "option 1 of column `level` is not a value of its column (i64)",
    vec![BadgeColumn::make("level").color(1, "success")]
);

refused_at_boot!(
    an_option_twice,
    Twice,
    "column `level` has option \"1\" twice",
    vec![TextColumn::make("level").options([("1", "One"), ("01", "Again")])]
);

refused_at_boot!(
    a_color_out_of_the_options,
    StrayColor,
    "column `status` colors \"gone\", which is not one of its options",
    vec![BadgeColumn::make("status").options([("draft", "Draft")]).color("gone", "danger")]
);
