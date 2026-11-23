//! `[admin] repeater_rows` (ADR 0044): the most rows of one parent a
//! relationship repeater reads and orders, and the most entries of each part
//! of its change (default 50, 1 to 500, checked at boot); sent to the panel.
//! SQLite, PostgreSQL, MySQL.

use axum::http::StatusCode;
use rustcms_admin::*;
use rustcms_auth::{ADMIN_GUARD, Actor, AuthProvider, AuthState, RbacService};
use rustcms_core::{AppBuilder, Config};
use rustcms_testing::{Factory, TestApp, TestClient, TestDb, TestResponse, UserFactory, db_test};
use sea_orm::{ConnectionTrait, EntityTrait, QueryFilter, QueryOrder, Schema};
use serde_json::{Value, json};

pub mod list {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "lists")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i32,
        pub name: String,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod item {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "list_items")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i32,
        pub list_id: i32,
        pub label: String,
        pub position: i32,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub struct ListResource;

impl Resource for ListResource {
    type Entity = list::Entity;
    const SLUG: &'static str = "lists";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("List").title("name")
    }

    fn fields() -> Vec<Field> {
        vec![Field::column(list::Column::Name).required()]
    }

    fn form() -> Form {
        Form::new().schema(components![
            TextInput::make("name"),
            Repeater::make("items")
                .relationship()
                .order_column("position")
                .schema(components![TextInput::make("label")]),
        ])
    }

    fn relations() -> Vec<Relation> {
        vec![Relation::has_many("items", "list_items", "list_id")]
    }
}

pub struct ItemResource;

impl Resource for ItemResource {
    type Entity = item::Entity;
    const SLUG: &'static str = "list_items";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Item").title("label").lock_parent("list_id", "lists")
    }

    fn fields() -> Vec<Field> {
        use item::Column as C;
        vec![
            Field::column(C::ListId).required().belongs_to("lists"),
            Field::column(C::Label).required(),
            Field::column(C::Position).guarded().readable(|_| true),
        ]
    }
}

fn build(config: Config) -> AppBuilder {
    let mut app = AppBuilder::new(config);
    app.provider(AuthProvider::new());
    app.provider(AdminProvider::new().resource::<ListResource>().resource::<ItemResource>());
    app
}

/// The app with `[admin] repeater_rows` set to `rows` (`None`: the default).
async fn setup(db: TestDb, rows: Option<u64>) -> (TestApp, rustcms_core::models::user::Model) {
    db.migrate::<rustcms_migration::Migrator>().await;
    let schema = Schema::new(db.conn().get_database_backend());
    rustcms_testing::create_table(db.conn(), &schema.create_table_from_entity(list::Entity)).await;
    rustcms_testing::create_table(db.conn(), &schema.create_table_from_entity(item::Entity)).await;
    let mut config = rustcms_testing::test_config(db.url());
    if let Some(rows) = rows {
        config = config.with("admin.repeater_rows", rows).unwrap();
    }
    let app = TestApp::with_config(build, config, db).await;
    let auth = app.state().service::<AuthState>().unwrap();
    let db = app.state().db();
    let rbac = RbacService::new(db, &auth.resolver);
    rbac.sync_permissions(&auth.permissions.all()).await.unwrap();
    let admin = UserFactory::new().name("Admin").create(db).await;
    let super_admin = rustcms_auth::entities::role::super_admin_id(db, ADMIN_GUARD).await.unwrap().unwrap();
    rbac.assign_role(Actor::System, admin.id, super_admin).await.unwrap();
    (app, admin)
}

fn creates(n: usize) -> Vec<Value> {
    (0..n).map(|i| json!({"ref": format!("n{i}"), "data": {"label": format!("l{i}")}})).collect()
}

async fn create_list(client: &TestClient<'_>, app: &TestApp, items: Value) -> TestResponse {
    client.post_json(&app.admin_api("resources/lists/records"), &json!({"name": "L", "items": items})).await
}

async fn update_list(client: &TestClient<'_>, app: &TestApp, list: &str, items: Value) -> TestResponse {
    client.fetch_csrf().await;
    client
        .send(
            axum::http::Method::PATCH,
            &app.admin_api(&format!("resources/lists/records/{list}")),
            Some(&json!({"items": items})),
        )
        .await
}

/// A list with `n` items stored at positions 1 to `n`; its key and the items' keys in that order.
async fn list_with(app: &TestApp, n: usize) -> (String, Vec<String>) {
    let db = app.state().db();
    db.execute_unprepared("INSERT INTO lists (name) VALUES ('L')").await.unwrap();
    let list = list::Entity::find().order_by_desc(list::Column::Id).one(db).await.unwrap().unwrap().id;
    let values: Vec<String> = (1..=n).map(|i| format!("({list}, 'l{i}', {i})")).collect();
    db.execute_unprepared(&format!("INSERT INTO list_items (list_id, label, position) VALUES {}", values.join(", ")))
        .await
        .unwrap();
    (list.to_string(), keys(app, &list.to_string()).await)
}

/// The list's items' keys, by position.
async fn keys(app: &TestApp, list: &str) -> Vec<String> {
    item::Entity::find()
        .filter(sea_orm::ColumnTrait::eq(&item::Column::ListId, list.parse::<i32>().unwrap()))
        .order_by_asc(item::Column::Position)
        .all(app.state().db())
        .await
        .unwrap()
        .into_iter()
        .map(|i| i.id.to_string())
        .collect()
}

/// The refusal over the limit: `too_many_rows` at the repeater, naming the limit.
fn assert_too_many(response: TestResponse, max: &str) {
    response.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
    let issues = response.json()["issues"].clone();
    assert_eq!(issues["items"][0]["code"], json!("too_many_rows"), "{issues}");
    assert_eq!(issues["items"][0]["params"]["max"], json!(max), "{issues}");
}

db_test!(the_default_refuses_51_rows_in_a_part_or_to_order, |db: TestDb| async move {
    let (app, admin) = setup(db, None).await;
    let client = app.acting_as(&admin).await;
    // The panel reads 50 rows.
    let schema = client.get(&app.admin_api("resources/lists/schema?operation=edit")).await.json();
    assert_eq!(schema["form"]["components"][1]["repeater"]["relationship"]["rowLimit"], json!(50));
    // 50 entries in a part pass; 51 are refused, nothing saved.
    create_list(&client, &app, json!({"create": creates(50)})).await.assert_status(StatusCode::CREATED);
    assert_too_many(create_list(&client, &app, json!({"create": creates(51)})).await, "50");
    assert_eq!(list::Entity::find().all(app.state().db()).await.unwrap().len(), 1);
    // 51 stored rows: an order naming them all is over the limit, and so is
    // ordering a parent with more live rows than it (whatever the order names).
    let (list, items) = list_with(&app, 51).await;
    let mut reversed = items.clone();
    reversed.reverse();
    assert_too_many(update_list(&client, &app, &list, json!({"order": reversed})).await, "50");
    assert_too_many(update_list(&client, &app, &list, json!({"order": [items[0]]})).await, "50");
    let updates: Vec<Value> = items.iter().map(|k| json!({"key": k, "data": {"label": "x"}})).collect();
    assert_too_many(update_list(&client, &app, &list, json!({"update": updates})).await, "50");
    assert_too_many(update_list(&client, &app, &list, json!({"delete": items})).await, "50");
    // Updates and deletes name stored rows together: 30 + 30 is over 50.
    let mixed = json!({"update": updates[..30].to_vec(), "delete": items[30..].to_vec()});
    assert_too_many(update_list(&client, &app, &list, mixed).await, "50");
    assert_eq!(keys(&app, &list).await, items, "nothing reordered");
});

db_test!(a_raised_limit_orders_more_rows, |db: TestDb| async move {
    let (app, admin) = setup(db, Some(150)).await;
    let client = app.acting_as(&admin).await;
    let schema = client.get(&app.admin_api("resources/lists/schema?operation=edit")).await.json();
    assert_eq!(schema["form"]["components"][1]["repeater"]["relationship"]["rowLimit"], json!(150));
    create_list(&client, &app, json!({"create": creates(51)})).await.assert_status(StatusCode::CREATED);
    let (list, items) = list_with(&app, 120).await;
    let mut reversed = items.clone();
    reversed.reverse();
    update_list(&client, &app, &list, json!({"order": reversed})).await.assert_status(StatusCode::OK);
    assert_eq!(keys(&app, &list).await, reversed);
    // The relation's list serves the panel's page: every row at once, beyond the resource's 100.
    let page =
        client.get(&app.admin_api(&format!("resources/lists/records/{list}/relations/items?per_page=150"))).await;
    page.assert_status(StatusCode::OK);
    assert_eq!(page.json()["data"].as_array().unwrap().len(), 120);
    // Other lists keep the resource's page size.
    let page = client.get(&app.admin_api("resources/list_items/records?per_page=150")).await.json();
    assert_eq!(page["meta"]["perPage"], json!(100), "{}", page["meta"]);
    // 151 rows: over again.
    let (list, items) = list_with(&app, 151).await;
    assert_too_many(update_list(&client, &app, &list, json!({"order": [items[0]]})).await, "150");
});

#[test]
fn an_out_of_range_limit_stops_the_boot() {
    for value in [0u64, 501] {
        let failed = std::panic::catch_unwind(|| {
            let config = rustcms_testing::test_config("sqlite::memory:").with("admin.repeater_rows", value).unwrap();
            build(config);
        });
        let message = failed.err().and_then(|p| p.downcast_ref::<String>().cloned()).unwrap_or_default();
        assert!(message.contains("admin.repeater_rows must be 1 to 500"), "{value}: {message:?}");
    }
}
