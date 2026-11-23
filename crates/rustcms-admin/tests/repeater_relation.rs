//! Relationship repeaters (ADR 0040, part 2): a parent's write changes its
//! children (create, update, delete, order) with the child's own checks;
//! rows not named are never touched; rows hidden by the child's scope keep
//! their place; a refusal anywhere rolls back the parent and every row.
//! SQLite, PostgreSQL, MySQL.

use axum::http::StatusCode;
use rustcms_admin::*;
use rustcms_auth::{ADMIN_GUARD, Actor, AuthProvider, AuthState, RbacService};
use rustcms_core::{AppBuilder, AppError, AppResult, Config};
use rustcms_testing::{Factory, TestApp, TestClient, TestDb, TestResponse, UserFactory, db_test};
use sea_orm::{ConnectionTrait, DatabaseTransaction, EntityTrait, QueryFilter, QueryOrder, Schema};
use serde_json::{Value, json};

mod support;

pub mod order {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "orders")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i32,
        pub number: String,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod line {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "order_lines")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i32,
        pub order_id: i32,
        pub product: String,
        pub quantity: i32,
        pub position: i32,
        pub deleted_at: Option<DateTimeUtc>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

fn lines() -> rustcms_admin::ui::Input {
    Repeater::make("lines")
        .relationship()
        .order_column("position")
        .min_items(1)
        .max_items(4)
        .schema(components![TextInput::make("product"), NumberInput::make("quantity")])
        .item_label("{product}")
}

pub struct OrderResource;

impl Resource for OrderResource {
    type Entity = order::Entity;
    const SLUG: &'static str = "orders";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Order").title("number")
    }

    fn fields() -> Vec<Field> {
        vec![Field::column(order::Column::Number).required()]
    }

    fn form() -> Form {
        Form::new().schema(components![TextInput::make("number"), lines()])
    }

    fn table() -> Table {
        Table::new().columns(vec![TextColumn::make("number"), TextColumn::make("lines_count").counts("lines")])
    }

    fn relations() -> Vec<Relation> {
        vec![Relation::has_many("lines", "order_lines", "order_id")]
    }
}

/// Lines: the panel's permissions, and lines named "secret" visible to no one
/// (decided per record once [`PER_RECORD`] is set; nextest runs each test in
/// its own process).
struct LinePolicy;

static PER_RECORD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[async_trait::async_trait]
impl Policy for LinePolicy {
    async fn allows(&self, ctx: &Ctx, ability: &str, record: Option<&Data>) -> bool {
        let secret = record.is_some_and(|r| r.get("product") == Some(&json!("secret")));
        ctx.can(&format!("order_lines.{ability}")) && !(secret && PER_RECORD.load(std::sync::atomic::Ordering::SeqCst))
    }

    async fn admits(&self, ctx: &Ctx, ability: &str) -> bool {
        ctx.can(&format!("order_lines.{ability}"))
    }

    fn view_depends_on_record(&self) -> bool {
        PER_RECORD.load(std::sync::atomic::Ordering::SeqCst)
    }
}

pub struct LineResource;

#[async_trait::async_trait]
impl Resource for LineResource {
    type Entity = line::Entity;
    const SLUG: &'static str = "order_lines";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Line").title("product").soft_deletes().lock_parent("order_id", "orders")
    }

    fn fields() -> Vec<Field> {
        use line::Column as C;
        vec![
            Field::column(C::OrderId).required().belongs_to("orders"),
            Field::column(C::Product).required().max_length(20),
            Field::column(C::Quantity).required(),
            Field::column(C::Position).guarded().readable(|_| true),
            Field::column(C::DeletedAt).guarded().readable(|_| true),
        ]
    }

    fn policy() -> std::sync::Arc<dyn Policy> {
        std::sync::Arc::new(LinePolicy)
    }

    // Lines named "hidden" are outside everyone's scope.
    fn scope(query: sea_orm::Select<line::Entity>, _ctx: &Ctx) -> sea_orm::Select<line::Entity> {
        query.filter(sea_orm::ColumnTrait::ne(&line::Column::Product, "hidden"))
    }

    async fn mutate_before_create(_ctx: &Ctx, _txn: &DatabaseTransaction, data: Data) -> AppResult<Data> {
        if data.get("product") == Some(&json!("boom")) {
            return Err(rustcms_core::ValidationErrors::field("product", "is refused by a hook").into());
        }
        Ok(data)
    }
}

fn build(config: Config) -> AppBuilder {
    let mut app = AppBuilder::new(config);
    app.provider(AuthProvider::new());
    app.provider(AdminProvider::new().resource::<OrderResource>().resource::<LineResource>());
    app
}

struct Users {
    admin: rustcms_core::models::user::Model,
    /// Orders, and lines without `delete`.
    clerk: rustcms_core::models::user::Model,
}

async fn setup(db: TestDb) -> (TestApp, Users) {
    db.migrate::<rustcms_migration::Migrator>().await;
    let schema = Schema::new(db.conn().get_database_backend());
    rustcms_testing::create_table(db.conn(), &schema.create_table_from_entity(order::Entity)).await;
    rustcms_testing::create_table(db.conn(), &schema.create_table_from_entity(line::Entity)).await;
    // Positions are unique per order: reordering writes in two passes.
    db.conn()
        .execute_unprepared("CREATE UNIQUE INDEX order_lines_position ON order_lines (order_id, position)")
        .await
        .unwrap();
    let app = TestApp::new(build, db).await;
    let auth = app.state().service::<AuthState>().unwrap();
    let db = app.state().db();
    let rbac = RbacService::new(db, &auth.resolver);
    rbac.sync_permissions(&auth.permissions.all()).await.unwrap();
    let admin = UserFactory::new().name("Admin").create(db).await;
    let super_admin = rustcms_auth::entities::role::super_admin_id(db, ADMIN_GUARD).await.unwrap().unwrap();
    rbac.assign_role(Actor::System, admin.id, super_admin).await.unwrap();
    let clerk = UserFactory::new().name("Clerk").create(db).await;
    let permissions: Vec<String> = ["orders.view_any", "orders.view", "orders.create", "orders.update"]
        .into_iter()
        .chain(["order_lines.view_any", "order_lines.view", "order_lines.create", "order_lines.update"])
        .map(str::to_owned)
        .collect();
    let role = rbac.create_role(Actor::System, ADMIN_GUARD, "clerk", &permissions).await.unwrap();
    rbac.assign_role(Actor::System, clerk.id, role.id).await.unwrap();
    (app, Users { admin, clerk })
}

/// The order's active lines as `(product, quantity, position)`, by position.
async fn stored(app: &TestApp, order: &str) -> Vec<(String, i32, i32)> {
    line::Entity::find()
        .filter(sea_orm::ColumnTrait::eq(&line::Column::OrderId, order.parse::<i32>().unwrap()))
        .filter(sea_orm::ColumnTrait::is_null(&line::Column::DeletedAt))
        .order_by_asc(line::Column::Position)
        .all(app.state().db())
        .await
        .unwrap()
        .into_iter()
        .map(|l| (l.product, l.quantity, l.position))
        .collect()
}

async fn line_key(app: &TestApp, order: &str, product: &str) -> String {
    line::Entity::find()
        .filter(sea_orm::ColumnTrait::eq(&line::Column::OrderId, order.parse::<i32>().unwrap()))
        .filter(sea_orm::ColumnTrait::eq(&line::Column::Product, product))
        .one(app.state().db())
        .await
        .unwrap()
        .unwrap()
        .id
        .to_string()
}

async fn create_order(client: &TestClient<'_>, app: &TestApp, lines: Value) -> TestResponse {
    client.post_json(&app.admin_api("resources/orders/records"), &json!({"number": "A-1", "lines": lines})).await
}

async fn update_order(client: &TestClient<'_>, app: &TestApp, order: &str, body: Value) -> TestResponse {
    client.fetch_csrf().await;
    client
        .send(axum::http::Method::PATCH, &app.admin_api(&format!("resources/orders/records/{order}")), Some(&body))
        .await
}

fn issues(response: TestResponse) -> Value {
    response.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
    response.json()["issues"].clone()
}

/// An order with lines "a", "b", "c" at positions 1, 2, 3, and a line outside the scope at 4.
async fn order_with_lines(app: &TestApp, client: &TestClient<'_>) -> String {
    let created = create_order(
        client,
        app,
        json!({
            "create": [
                {"ref": "n1", "data": {"product": "a", "quantity": 1}},
                {"ref": "n2", "data": {"product": "b", "quantity": 2}},
                {"ref": "n3", "data": {"product": "c", "quantity": 3}},
            ],
            "order": [{"new": "n1"}, {"new": "n2"}, {"new": "n3"}],
        }),
    )
    .await;
    created.assert_status(StatusCode::CREATED);
    let order = created.json()["data"]["key"].as_str().unwrap().to_owned();
    app.state()
        .db()
        .execute_unprepared(&format!(
            "INSERT INTO order_lines (order_id, product, quantity, position) VALUES ({order}, 'hidden', 9, 4)"
        ))
        .await
        .unwrap();
    order
}

db_test!(a_create_writes_its_rows_in_order, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    let admin = app.acting_as(&users.admin).await;
    let order = order_with_lines(&app, &admin).await;
    assert_eq!(
        stored(&app, &order).await,
        [("a".into(), 1, 1), ("b".into(), 2, 2), ("c".into(), 3, 3), ("hidden".into(), 9, 4)]
    );
    // The parent's history lists the rows; each row has its own entry.
    let history = admin.get(&app.admin_api(&format!("resources/orders/records/{order}/history"))).await.json();
    let rows = history["data"].as_array().unwrap().iter().find(|e| e["event"] == "rows_changed").unwrap().clone();
    assert_eq!(rows["meta"]["relation"], json!("lines"));
    // Counts only: the rows themselves are the child's history.
    assert_eq!(rows["meta"]["created"], json!(3));
    let key = line_key(&app, &order, "a").await;
    let child = admin.get(&app.admin_api(&format!("resources/order_lines/records/{key}/history"))).await.json();
    assert_eq!(child["data"][0]["event"], json!("created"));
});

db_test!(an_update_changes_only_the_rows_it_names, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    let admin = app.acting_as(&users.admin).await;
    let order = order_with_lines(&app, &admin).await;
    let (a, b, c) =
        (line_key(&app, &order, "a").await, line_key(&app, &order, "b").await, line_key(&app, &order, "c").await);
    let response = update_order(
        &admin,
        &app,
        &order,
        json!({"lines": {
            "update": [{"key": b, "data": {"quantity": 20}}],
            "delete": [a],
            "create": [{"ref": "n1", "data": {"product": "d", "quantity": 4}}],
            "order": [{"new": "n1"}, c, b],
        }}),
    )
    .await;
    response.assert_status(StatusCode::OK);
    // d, c, b take the visible rows' places (2, 3 and a new one); the hidden row keeps 4.
    let rows = stored(&app, &order).await;
    assert_eq!(rows, [("d".into(), 4, 2), ("c".into(), 3, 3), ("hidden".into(), 9, 4), ("b".into(), 20, 5)]);
    // The deleted line is in the trash, not gone.
    let trashed = line::Entity::find_by_id(a.parse::<i32>().unwrap()).one(app.state().db()).await.unwrap().unwrap();
    assert!(trashed.deleted_at.is_some());
});

db_test!(rows_of_another_parent_or_hidden_are_refused, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    let admin = app.acting_as(&users.admin).await;
    let first = order_with_lines(&app, &admin).await;
    let second = order_with_lines(&app, &admin).await;
    let foreign = line_key(&app, &first, "a").await;
    let hidden = line_key(&app, &second, "hidden").await;
    let found = issues(
        update_order(
            &admin,
            &app,
            &second,
            json!({"lines": {"update": [{"key": foreign, "data": {"quantity": 5}}], "delete": [hidden]}}),
        )
        .await,
    );
    assert_eq!(found[format!("lines.{foreign}")][0]["code"], json!("not_a_row"));
    assert!(found.get(format!("lines.{hidden}")).is_some());
    // An order that leaves out a row the user sees.
    let (a, b) = (line_key(&app, &second, "a").await, line_key(&app, &second, "b").await);
    let missing = issues(update_order(&admin, &app, &second, json!({"lines": {"order": [b, a]}})).await);
    assert!(missing.get("lines").is_some());
    assert_eq!(stored(&app, &first).await.len(), 4);
});

db_test!(row_errors_are_keyed_by_row_and_roll_everything_back, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    let admin = app.acting_as(&users.admin).await;
    let found = issues(
        create_order(
            &admin,
            &app,
            json!({"create": [{"ref": "n1", "data": {"product": "a"}}, {"ref": "n2", "data": {"product": "boom", "quantity": 1}}]}),
        )
        .await,
    );
    assert_eq!(found["lines.n1.quantity"][0]["code"], json!("required"));
    // A child hook's refusal rolls back the parent too.
    let refused =
        create_order(&admin, &app, json!({"create": [{"ref": "n1", "data": {"product": "boom", "quantity": 1}}]}))
            .await;
    assert!(issues(refused).get("lines.n1.product").is_some());
    assert!(order::Entity::find().all(app.state().db()).await.unwrap().is_empty());
    // The parent key and the order column are the server's.
    let order = order_with_lines(&app, &admin).await;
    let a = line_key(&app, &order, "a").await;
    let keyed = issues(
        update_order(&admin, &app, &order, json!({"lines": {"update": [{"key": a, "data": {"position": 9}}]}})).await,
    );
    assert!(keyed.get(format!("lines.{a}.position")).is_some());
    // At most four rows (the hidden one counts: it is a row of the order).
    let fifth = update_order(
        &admin,
        &app,
        &order,
        json!({"lines": {"create": [{"ref": "n9", "data": {"product": "e", "quantity": 1}}]}}),
    )
    .await;
    assert_eq!(issues(fifth)["lines"][0]["code"], json!("max_items"));
    // Shapes that cannot hold.
    for bad in [json!([]), json!({"remove": []}), json!({"create": [{"ref": "Bad Ref", "data": {}}]})] {
        assert_eq!(
            issues(update_order(&admin, &app, &order, json!({"lines": bad})).await)["lines"][0]["code"],
            json!("invalid")
        );
    }
});

db_test!(each_part_needs_its_ability_on_the_child, |db: TestDb| async move {
    let sqlite = db.backend() == rustcms_testing::Backend::Sqlite;
    let (app, users) = setup(db).await;
    let admin = app.acting_as(&users.admin).await;
    let order = order_with_lines(&app, &admin).await;
    let a = line_key(&app, &order, "a").await;
    let clerk = app.acting_as(&users.clerk).await;
    // No `order_lines.delete`: refused before anything is read or written.
    let refused = update_order(
        &clerk,
        &app,
        &order,
        json!({"number": "A-2", "lines": {"delete": [a], "create": [{"ref": "n1", "data": {"product": "x", "quantity": 1}}]}}),
    )
    .await;
    refused.assert_status(StatusCode::FORBIDDEN);
    assert_eq!(stored(&app, &order).await.len(), 4);
    let fresh = order::Entity::find_by_id(order.parse::<i32>().unwrap()).one(app.state().db()).await.unwrap().unwrap();
    assert_eq!(fresh.number, "A-1");
    // Updating is allowed.
    let b = line_key(&app, &order, "b").await;
    update_order(&clerk, &app, &order, json!({"lines": {"update": [{"key": b, "data": {"quantity": 7}}]}}))
        .await
        .assert_status(StatusCode::OK);

    // The schema says what each user may do.
    let url = app.admin_api(&format!("resources/orders/schema?operation=edit&key={order}"));
    let for_clerk = clerk.get(&url).await.json();
    if sqlite {
        // Shared with the SPA tests: the admin's edit form and the order's rows.
        support::contract("repeater-relation.schema.edit.json", &admin.get(&url).await.json());
        let rows =
            admin.get(&app.admin_api(&format!("resources/orders/records/{order}/relations/lines?per_page=100"))).await;
        support::contract("repeater-relation.rows.json", &rows.json());
    }
    let input =
        for_clerk["form"]["components"].as_array().unwrap().iter().find(|c| c["name"] == "lines").unwrap().clone();
    assert_eq!(input["valueType"], json!("rows"));
    assert_eq!(
        input["repeater"]["relationship"],
        json!({
            "resource": "order_lines", "relation": "lines", "create": true, "update": true, "delete": false,
            "orderColumn": "position", "rowLimit": 50,
        })
    );
    let rows: Vec<&str> =
        input["repeater"]["components"].as_array().unwrap().iter().map(|c| c["name"].as_str().unwrap()).collect();
    assert_eq!(rows, ["product", "quantity"]);
    assert_eq!(input["repeater"]["reorderable"], json!(true));
    // Never on view pages (the relation section shows the rows).
    let view = admin.get(&app.admin_api(&format!("resources/orders/schema?operation=view&key={order}"))).await.json();
    assert!(view["form"]["components"].as_array().unwrap().iter().all(|c| c["name"] != "lines"));
});

db_test!(the_minimum_and_the_inputs_hold_on_every_save, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    let admin = app.acting_as(&users.admin).await;
    // A new order has only the rows it creates: none, sent or not, is under the minimum.
    let none = create_order(&admin, &app, json!({"create": []})).await;
    assert!(issues(none)["lines"].is_array());
    let omitted = admin.post_json(&app.admin_api("resources/orders/records"), &json!({"number": "A-9"})).await;
    assert!(issues(omitted)["lines"].is_array());
    let order = order_with_lines(&app, &admin).await;
    // Rows edit the repeater's inputs only: another field of the line is refused.
    let a = line_key(&app, &order, "a").await;
    let sent = json!({"lines": {"update": [{"key": a, "data": {"deleted_at": "2026-01-01T00:00:00Z"}}]}});
    let refused = update_order(&admin, &app, &order, sent).await;
    assert!(issues(refused)[format!("lines.{a}.deleted_at")].is_array());
    let sent =
        json!({"lines": {"create": [{"ref": "n9", "data": {"product": "x", "quantity": 1, "deleted_at": null}}]}});
    assert!(issues(update_order(&admin, &app, &order, sent).await)["lines.n9.deleted_at"].is_array());
});

macro_rules! refused_at_boot {
    ($test:ident, $resource:ident, $message:literal, $input:expr) => {
        pub struct $resource;

        impl Resource for $resource {
            type Entity = order::Entity;
            const SLUG: &'static str = "orders";

            fn meta() -> ResourceMeta {
                ResourceMeta::new("Order")
            }

            fn fields() -> Vec<Field> {
                vec![Field::column(order::Column::Number).required()]
            }

            fn form() -> Form {
                Form::new().schema(components![TextInput::make("number"), $input])
            }

            fn relations() -> Vec<Relation> {
                vec![Relation::has_many("lines", "order_lines", "order_id")]
            }
        }

        #[test]
        #[should_panic(expected = $message)]
        fn $test() {
            Registry::new().register::<$resource>().register::<LineResource>().finish();
        }
    };
}

refused_at_boot!(
    an_unknown_relation,
    UnknownRelation,
    "needs a has-many relation named `items`",
    Repeater::make("items").relationship().schema(components![TextInput::make("product")])
);
refused_at_boot!(
    an_input_of_no_child_field,
    NoChildField,
    "row input `colour` is not a field of `order_lines`",
    Repeater::make("lines").relationship().schema(components![TextInput::make("colour")])
);
refused_at_boot!(
    a_text_order_column,
    TextOrder,
    "the order column `product` must be an integer field",
    Repeater::make("lines").relationship().order_column("product").schema(components![NumberInput::make("quantity")])
);
refused_at_boot!(
    the_parent_key_as_input,
    ParentKeyInput,
    "the parent key `order_id` cannot be an input",
    Repeater::make("lines").relationship().schema(components![NumberInput::make("order_id")])
);
refused_at_boot!(
    a_row_condition_naming_no_row_input,
    RowConditions,
    "names `quantity`, which is not an input (or entry) of the form",
    Repeater::make("lines")
        .relationship()
        .schema(components![TextInput::make("product").shown_when(when("quantity").filled())])
);
refused_at_boot!(
    a_row_condition_naming_no_form_input,
    RowRootConditions,
    "uses `when_root(\"status\")`, which is not an input of that level",
    Repeater::make("lines")
        .relationship()
        .schema(components![TextInput::make("product").shown_when(when_root("status").filled())])
);
refused_at_boot!(
    a_row_condition_reading_a_parent_row,
    RowParentConditions,
    "uses `when_parent(\"number\")`, which reads the enclosing row",
    Repeater::make("lines")
        .relationship()
        .schema(components![TextInput::make("product").shown_when(when_parent("number").filled())])
);
refused_at_boot!(
    clearing_a_relationship_repeater,
    ClearedRepeater,
    "`clear_when_hidden` does not apply (a hidden relationship repeater keeps its rows)",
    Repeater::make("lines")
        .relationship()
        .shown_when(when("number").filled())
        .clear_when_hidden()
        .schema(components![TextInput::make("product")])
);
refused_at_boot!(
    clearing_in_relationship_rows,
    ClearedRow,
    "`clear_when_hidden` does not apply inside relationship rows",
    Repeater::make("lines").relationship().schema(components![
        TextInput::make("product").shown_when(when("quantity").filled()).clear_when_hidden(),
        NumberInput::make("quantity"),
    ])
);
refused_at_boot!(
    a_json_rule_in_relationship_rows,
    JsonRule,
    "the child's field decides",
    Repeater::make("lines").relationship().schema(components![TextInput::make("product").required()])
);

macro_rules! column_refused_at_boot {
    ($test:ident, $resource:ident, $message:literal, $column:expr) => {
        pub struct $resource;

        impl Resource for $resource {
            type Entity = order::Entity;
            const SLUG: &'static str = "orders";

            fn meta() -> ResourceMeta {
                ResourceMeta::new("Order")
            }

            fn fields() -> Vec<Field> {
                vec![Field::column(order::Column::Number).required()]
            }

            fn table() -> Table {
                Table::new().columns(vec![$column])
            }

            fn relations() -> Vec<Relation> {
                vec![Relation::has_many("lines", "order_lines", "order_id")]
            }
        }

        #[test]
        #[should_panic(expected = $message)]
        fn $test() {
            Registry::new().register::<$resource>().register::<LineResource>().finish();
        }
    };
}

column_refused_at_boot!(
    a_count_of_an_unknown_relation,
    CountUnknown,
    "counts `items`, which is not a has-many relation",
    TextColumn::make("items_count").counts("items")
);
column_refused_at_boot!(
    a_count_column_named_as_a_field,
    CountFieldName,
    "count column `number` has a field's name",
    TextColumn::make("number").counts("lines")
);
column_refused_at_boot!(
    an_item_count_of_a_text_field,
    CountText,
    "counts list items and needs a JSON field",
    TextColumn::make("number").count()
);

#[allow(dead_code)]
fn unused(_: AppError) {}

refused_at_boot!(
    own_options_on_a_row_input,
    RowOptions,
    "the child's form decides options and number bounds",
    Repeater::make("lines").relationship().schema(components![SelectInput::make("product").options([("a", "A")])])
);

/// Two repeaters of one parent on the same child resource: one locking phase
/// per child resource (ADR 0041).
pub struct TwoRepeaters;

impl Resource for TwoRepeaters {
    type Entity = order::Entity;
    const SLUG: &'static str = "orders";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Order")
    }

    fn fields() -> Vec<Field> {
        vec![Field::column(order::Column::Number).required()]
    }

    fn form() -> Form {
        Form::new().schema(components![
            TextInput::make("number"),
            Repeater::make("lines").relationship().schema(components![TextInput::make("product")]),
            Repeater::make("extra").relationship().schema(components![TextInput::make("product")]),
        ])
    }

    fn relations() -> Vec<Relation> {
        vec![
            Relation::has_many("lines", "order_lines", "order_id"),
            Relation::has_many("extra", "order_lines", "order_id"),
        ]
    }
}

#[test]
#[should_panic(expected = "is also edited by repeater `extra`, whose relation may hold the same rows")]
fn two_repeaters_on_one_child_resource() {
    Registry::new().register::<TwoRepeaters>().register::<LineResource>().finish();
}

db_test!(new_rows_are_tagged_in_the_order, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    let admin = app.acting_as(&users.admin).await;
    // A reference written as a key: never read as one (refused, nothing created).
    let created = create_order(
        &admin,
        &app,
        json!({"create": [{"ref": "n1", "data": {"product": "a", "quantity": 1}}], "order": ["n1"]}),
    )
    .await;
    assert_eq!(issues(created)["lines"][0]["code"], json!("invalid"));
    assert!(order::Entity::find().all(app.state().db()).await.unwrap().is_empty());
});

db_test!(positions_used_up_are_a_validation_error, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    let admin = app.acting_as(&users.admin).await;
    let order = order_with_lines(&app, &admin).await;
    // The largest position at the column's limit (i32): no room for a new row.
    app.state()
        .db()
        .execute_unprepared(&format!(
            "UPDATE order_lines SET position = {} WHERE order_id = {order} AND product = 'hidden'",
            i32::MAX
        ))
        .await
        .unwrap();
    let added = update_order(
        &admin,
        &app,
        &order,
        json!({"lines": {"create": [{"ref": "n9", "data": {"product": "z", "quantity": 1}}]}}),
    )
    .await;
    assert_eq!(issues(added)["lines"][0]["code"], json!("positions_exhausted"));
});

db_test!(concurrent_saves_of_one_family_never_hang_nor_fail, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    let admin = app.acting_as(&users.admin).await;
    let clerk = app.acting_as(&users.clerk).await;
    let order = order_with_lines(&app, &admin).await;
    let (a, b, c) =
        (line_key(&app, &order, "a").await, line_key(&app, &order, "b").await, line_key(&app, &order, "c").await);
    clerk.fetch_csrf().await;
    let line_url = app.admin_api(&format!("resources/order_lines/records/{a}"));
    for round in 0..5 {
        // The parent reorders while the child is written directly: the family
        // serializes on the order's row (ADR 0041).
        let sequence = if round % 2 == 0 { [&b, &a, &c] } else { [&a, &b, &c] };
        let order_body = json!({"lines": {"order": sequence}});
        let line_body = json!({"quantity": round + 10});
        let (parent, child) = tokio::join!(
            update_order(&admin, &app, &order, order_body),
            clerk.send(axum::http::Method::PATCH, &line_url, Some(&line_body)),
        );
        for response in [&parent, &child] {
            assert!(
                [StatusCode::OK, StatusCode::CONFLICT].contains(&response.status),
                "round {round}: {} {}",
                response.status,
                response.text()
            );
        }
    }
});

/// Notes of an order with an `i64` order column and a media field the rows do
/// not edit (a hook adding it is refused).
pub mod note {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "order_notes")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i32,
        pub order_id: i32,
        pub title: String,
        pub position: i64,
        pub photo: Option<Uuid>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub struct NoteOrderResource;

impl Resource for NoteOrderResource {
    type Entity = order::Entity;
    const SLUG: &'static str = "note_orders";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Order").title("number")
    }

    fn fields() -> Vec<Field> {
        vec![Field::column(order::Column::Number).required()]
    }

    fn form() -> Form {
        Form::new().schema(components![
            TextInput::make("number"),
            Repeater::make("notes")
                .relationship()
                .order_column("position")
                .deletable(false)
                .schema(components![TextInput::make("title")]),
        ])
    }

    fn relations() -> Vec<Relation> {
        vec![Relation::has_many("notes", "order_notes", "order_id")]
    }
}

pub struct NoteResource;

#[async_trait::async_trait]
impl Resource for NoteResource {
    type Entity = note::Entity;
    const SLUG: &'static str = "order_notes";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Note").title("title").lock_parent("order_id", "note_orders")
    }

    fn fields() -> Vec<Field> {
        use note::Column as C;
        vec![
            Field::column(C::OrderId).required().belongs_to("note_orders"),
            Field::column(C::Title).required(),
            Field::column(C::Position).guarded().readable(|_| true),
            Field::column(C::Photo).media(),
        ]
    }

    /// A note titled "photo" gets a photo from the hook: refused in a row.
    async fn mutate_before_create(_ctx: &Ctx, _txn: &DatabaseTransaction, mut data: Data) -> AppResult<Data> {
        if data.get("title") == Some(&json!("photo")) {
            data.insert("photo".into(), json!("0c7e1d2a-5b3f-4e6a-9d8c-1a2b3c4d5e6f"));
        }
        Ok(data)
    }
}

fn build_notes(config: Config) -> AppBuilder {
    let mut app = AppBuilder::new(config);
    app.provider(AuthProvider::new());
    app.provider(AdminProvider::new().resource::<NoteOrderResource>().resource::<NoteResource>());
    app
}

db_test!(i64_positions_deletable_off_and_hook_added_media, |db: TestDb| async move {
    db.migrate::<rustcms_migration::Migrator>().await;
    let schema = Schema::new(db.conn().get_database_backend());
    rustcms_testing::create_table(db.conn(), &schema.create_table_from_entity(order::Entity)).await;
    rustcms_testing::create_table(db.conn(), &schema.create_table_from_entity(note::Entity)).await;
    let app = TestApp::new(build_notes, db).await;
    let auth = app.state().service::<AuthState>().unwrap();
    let conn = app.state().db();
    let rbac = RbacService::new(conn, &auth.resolver);
    rbac.sync_permissions(&auth.permissions.all()).await.unwrap();
    let admin = UserFactory::new().name("Admin").create(conn).await;
    let super_admin = rustcms_auth::entities::role::super_admin_id(conn, ADMIN_GUARD).await.unwrap().unwrap();
    rbac.assign_role(Actor::System, admin.id, super_admin).await.unwrap();
    let admin = app.acting_as(&admin).await;
    let url = app.admin_api("resources/note_orders/records");

    // i64 positions: written as the column's kind, read back in order.
    let created = admin
        .post_json(
            &url,
            &json!({"number": "N-1", "notes": {
                "create": [{"ref": "a", "data": {"title": "first"}}, {"ref": "b", "data": {"title": "second"}}],
                "order": [{"new": "b"}, {"new": "a"}],
            }}),
        )
        .await;
    created.assert_status(StatusCode::CREATED);
    let order = created.json()["data"]["key"].as_str().unwrap().to_owned();
    let mut notes = note::Entity::find().all(app.state().db()).await.unwrap();
    notes.sort_by_key(|n| n.position);
    assert_eq!(notes.iter().map(|n| n.title.as_str()).collect::<Vec<_>>(), ["second", "first"]);

    // `deletable(false)`: a delete part is refused by the server.
    admin.fetch_csrf().await;
    let first = notes.iter().find(|n| n.title == "first").unwrap().id.to_string();
    let refused = admin
        .send(axum::http::Method::PATCH, &format!("{url}/{order}"), Some(&json!({"notes": {"delete": [first]}})))
        .await;
    assert_eq!(issues(refused)["notes"][0]["code"], json!("rows_not_deletable"));

    // A hook adding media to a row: refused (no media lock inside rows), nothing saved.
    let photo = admin
        .send(
            axum::http::Method::PATCH,
            &format!("{url}/{order}"),
            Some(&json!({"notes": {"create": [{"ref": "p", "data": {"title": "photo"}}]}})),
        )
        .await;
    photo.assert_status(StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(note::Entity::find().all(app.state().db()).await.unwrap().len(), 2);
});
