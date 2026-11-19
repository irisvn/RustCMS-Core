//! Date formats (vocabulary 21): `TableColumn::date_format` and
//! `Entry::date_format` reach the schemas as `format`; a generated view page
//! has none (the panel's applies); patterns that are not patterns, or do not
//! fit their field, refuse to boot, as do the panel's
//! (`AdminProvider::date_format`). SQLite, PostgreSQL, MySQL.

use axum::http::StatusCode;
use rustcms_admin::*;
use rustcms_auth::{ADMIN_GUARD, AuthProvider, AuthState, RbacService};
use rustcms_core::{AppBuilder, Config};
use rustcms_testing::{Factory, TestApp, TestDb, UserFactory, db_test};
use sea_orm::{ActiveModelTrait, Schema, Set};
use serde_json::{Value, json};

pub mod event {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "events")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i32,
        pub title: String,
        pub day: Option<Date>,
        pub local_at: Option<DateTime>,
        pub happened_at: Option<DateTimeUtc>,
        pub tags: Option<Json>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

fn event_fields() -> Vec<Field> {
    use event::Column as C;
    vec![
        Field::column(C::Title).required(),
        Field::column(C::Day),
        Field::column(C::LocalAt),
        Field::column(C::HappenedAt),
        Field::column(C::Tags),
    ]
}

pub struct EventResource;

impl Resource for EventResource {
    type Entity = event::Entity;
    const SLUG: &'static str = "events";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Event").title("title")
    }

    fn fields() -> Vec<Field> {
        event_fields()
    }

    fn table() -> Table {
        Table::new().columns(vec![
            TextColumn::make("title"),
            DateColumn::make("day").date_format("EEE dd/MM/yyyy"),
            DateColumn::make("local_at"),
            TextColumn::make("happened_at").date_format("HH:mm z, dd/MM/yyyy"),
        ])
    }

    fn infolist() -> Infolist {
        Infolist::new().schema(components![
            TextEntry::make("title"),
            TextEntry::make("day").date_format("d MMMM yyyy"),
            TextEntry::make("happened_at").date_format("'at' h:mm a"),
            TextEntry::make("local_at"),
        ])
    }
}

/// The same events with the view page made from the form.
pub struct PlainEventResource;

impl Resource for PlainEventResource {
    type Entity = event::Entity;
    const SLUG: &'static str = "plain-events";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Plain event").title("title")
    }

    fn fields() -> Vec<Field> {
        event_fields()
    }
}

fn build(config: Config) -> AppBuilder {
    let mut app = AppBuilder::new(config);
    app.provider(AuthProvider::new());
    app.provider(
        AdminProvider::new()
            .resource::<EventResource>()
            .resource::<PlainEventResource>()
            .date_format("dd/MM/yyyy")
            .datetime_format("HH:mm dd/MM/yyyy"),
    );
    app
}

async fn setup(db: TestDb) -> (TestApp, rustcms_core::models::user::Model) {
    db.migrate::<rustcms_migration::Migrator>().await;
    let schema = Schema::new(db.conn().get_database_backend());
    rustcms_testing::create_table(db.conn(), &schema.create_table_from_entity(event::Entity)).await;
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

/// The item named `name` of `list` (columns, components).
fn named<'a>(list: &'a Value, name: &str) -> &'a Value {
    list.as_array()
        .unwrap()
        .iter()
        .find_map(|c| if c["name"] == name { Some(c) } else { c.get("components").map(|inner| named(inner, name)) })
        .unwrap_or(&Value::Null)
}

db_test!(schemas_carry_the_patterns, |db: TestDb| async move {
    let (app, admin) = setup(db).await;
    let client = app.acting_as(&admin).await;
    let day = chrono::NaiveDate::from_ymd_opt(2026, 11, 18).unwrap();
    let event = event::ActiveModel { title: Set("Launch".into()), day: Set(Some(day)), ..Default::default() }
        .insert(app.state().db())
        .await
        .unwrap();

    let list = client.get(&app.admin_api("resources/events/schema")).await;
    list.assert_status(StatusCode::OK);
    let columns = &list.json()["table"]["columns"];
    assert_eq!(named(columns, "day")["format"], json!("EEE dd/MM/yyyy"));
    assert_eq!(named(columns, "happened_at")["format"], json!("HH:mm z, dd/MM/yyyy"));
    for name in ["title", "local_at"] {
        assert_eq!(named(columns, name).get("format"), None, "{name}");
    }

    let view = client.get(&app.admin_api(&format!("resources/events/schema?operation=view&key={}", event.id))).await;
    let entries = &view.json()["infolist"]["components"];
    assert_eq!(named(entries, "day")["format"], json!("d MMMM yyyy"));
    assert_eq!(named(entries, "happened_at")["format"], json!("'at' h:mm a"));
    assert_eq!(named(entries, "local_at").get("format"), None);

    // A view page made from the form shows dates with the panel's patterns.
    let plain =
        client.get(&app.admin_api(&format!("resources/plain-events/schema?operation=view&key={}", event.id))).await;
    let plain = plain.json();
    assert_eq!(plain["infolist"]["fromForm"], json!(true));
    assert!(!plain["infolist"].to_string().contains("\"format\""), "{plain}");
});

/// An events resource with `table` and `infolist`, to check them at boot.
macro_rules! refused_at_boot {
    ($test:ident, $resource:ident, $message:literal, $columns:expr, $infolist:expr) => {
        pub struct $resource;

        impl Resource for $resource {
            type Entity = event::Entity;
            const SLUG: &'static str = "events";

            fn meta() -> ResourceMeta {
                ResourceMeta::new("Event")
            }

            fn fields() -> Vec<Field> {
                event_fields()
            }

            fn table() -> Table {
                Table::new().columns($columns)
            }

            fn infolist() -> Infolist {
                Infolist::new().schema($infolist)
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
    a_pattern_that_is_not_one,
    NotAPattern,
    "column `day`: date format \"dd/MM/YYYY\": `Y` is not a field",
    vec![DateColumn::make("day").date_format("dd/MM/YYYY")],
    components![]
);

refused_at_boot!(
    a_pattern_on_text,
    OnText,
    "column `title`: date format \"dd/MM/yyyy\": a string field is not a date",
    vec![TextColumn::make("title").date_format("dd/MM/yyyy")],
    components![]
);

refused_at_boot!(
    a_time_on_a_date,
    TimeOnDate,
    "column `day`: date format \"HH:mm dd/MM\": a date has no time (`HH`)",
    vec![DateColumn::make("day").date_format("HH:mm dd/MM")],
    components![]
);

refused_at_boot!(
    a_zone_on_a_local_time,
    ZoneOnLocal,
    "column `local_at`: date format \"HH:mm z\": a local date-time has no time zone (`z`)",
    vec![DateColumn::make("local_at").date_format("HH:mm z")],
    components![]
);

refused_at_boot!(
    a_pattern_on_a_count,
    OnCount,
    "column `tags`: a date format needs a date or text column",
    vec![TextColumn::make("tags").count().date_format("yyyy")],
    components![]
);

refused_at_boot!(
    a_pattern_on_a_custom_cell,
    OnCell,
    "column `day`: a custom cell draws the value itself and takes no date format",
    vec![DateColumn::make("day").cell("acme/when").date_format("yyyy")],
    components![]
);

refused_at_boot!(
    a_pattern_on_a_boolean_column,
    OnBoolean,
    "column `day`: a date format needs a date or text column",
    vec![BooleanColumn::make("day").date_format("yyyy")],
    components![]
);

refused_at_boot!(
    an_entry_pattern_on_text,
    EntryOnText,
    "entry `title`: date format \"yyyy\": a string field is not a date",
    vec![TextColumn::make("title")],
    components![TextEntry::make("title").date_format("yyyy")]
);

refused_at_boot!(
    an_entry_time_on_a_date,
    EntryTimeOnDate,
    "entry `day`: date format \"d/M h a\": a date has no time (`h`)",
    vec![TextColumn::make("title")],
    components![TextEntry::make("day").date_format("d/M h a")]
);

refused_at_boot!(
    a_pattern_on_a_key_value_entry,
    OnKeyValue,
    "entry `tags`: `date_format` does not fit a",
    vec![TextColumn::make("title")],
    components![KeyValueEntry::make("tags").date_format("yyyy")]
);

refused_at_boot!(
    a_pattern_in_a_row,
    InRow,
    "entry `at`: a date format needs a date field",
    vec![TextColumn::make("title")],
    components![RepeatableEntry::make("tags").schema(components![TextEntry::make("at").date_format("yyyy")])]
);

#[test]
#[should_panic(expected = "admin panel: date format \"HH:mm\": a date has no time (`HH`)")]
fn the_panel_date_format_shows_no_time() {
    let _ = AdminProvider::new().date_format("HH:mm");
}

#[test]
#[should_panic(expected = "admin panel: date format \"HH:mm z\": a local date-time has no time zone (`z`)")]
fn the_panel_datetime_format_shows_no_zone() {
    let _ = AdminProvider::new().datetime_format("HH:mm z");
}

#[test]
#[should_panic(expected = "admin panel: date format \"yyyy-MM-dd hhh\": `hhh` is not a field")]
fn the_panel_datetime_format_is_a_pattern() {
    let _ = AdminProvider::new().datetime_format("yyyy-MM-dd hhh");
}
