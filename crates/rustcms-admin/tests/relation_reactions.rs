//! Form reactions on relationship rows (ADR 0044, part D): the rows a
//! reaction reads (tagged identities, readable inputs only, bounded), and the
//! row patches kept only when the user could make the change, checked against
//! the stored rows read through the child's scope, policy, parent and
//! filters, the field rules of the operation the save would run, and the
//! rows' conditions on authorized values. SQLite, PostgreSQL, MySQL. Nextest
//! runs each test in its own process (the script is a static).

use std::sync::Mutex;
use std::time::Duration;

use rustcms_admin::schema::{FormReactRequest, FormReactResponse};
use rustcms_admin::*;
use rustcms_core::{AppError, AppResult, Principal};
use rustcms_testing::{TestDb, db_test};
use sea_orm::{ConnectionTrait, EntityTrait, QueryFilter, Schema};
use serde_json::{Value, json};
use support::PrincipalExt;

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
        /// 1: a line (`lines`), 2: an extra (`extras`).
        pub kind: Option<i32>,
        pub product: String,
        pub quantity: Option<i32>,
        pub gift_note: Option<String>,
        pub code: Option<String>,
        pub memo: Option<String>,
        pub secret: Option<String>,
        pub locked: Option<String>,
        #[sea_orm(default_value = "big")]
        pub size: Option<String>,
        pub wrap: Option<String>,
        pub ribbon: Option<String>,
        pub position: Option<i32>,
        pub deleted_at: Option<DateTimeUtc>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// The patches the reactions answer with, and the state they last saw.
static SCRIPT: Mutex<Vec<FormPatch>> = Mutex::new(Vec::new());
static SEEN: Mutex<Option<FormState>> = Mutex::new(None);
static SLOW: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn script<'a>(_: &'a Ctx, state: &'a FormState) -> BoxFuture<'a, AppResult<Vec<FormPatch>>> {
    *SEEN.lock().unwrap() = Some(state.clone());
    let patches = SCRIPT.lock().unwrap().clone();
    Box::pin(async move {
        if SLOW.load(std::sync::atomic::Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(400)).await;
        }
        Ok(patches)
    })
}

fn play(patches: Vec<FormPatch>) {
    *SCRIPT.lock().unwrap() = patches;
}

pub struct OrderResource;

impl Resource for OrderResource {
    type Entity = order::Entity;
    const SLUG: &'static str = "orders";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Order").title("number").audit(false)
    }

    fn fields() -> Vec<Field> {
        vec![Field::column(order::Column::Number).required()]
    }

    fn form() -> Form {
        Form::new().schema(components![
            TextInput::make("number").reacts(script),
            // Explicit rows: the gift note shows for gifts.
            Repeater::make("lines")
                .relationship()
                .order_column("position")
                .min_items(1)
                .max_items(9)
                .shown_when(when("number").ne("hide"))
                .schema(components![
                    TextInput::make("product"),
                    NumberInput::make("quantity"),
                    TextInput::make("gift_note").shown_when(when("product").eq("gift")),
                    TextInput::make("code"),
                    TextInput::make("memo"),
                    TextInput::make("secret"),
                    TextInput::make("locked"),
                ])
                .reacts(script)
                .reaction_timeout(Duration::from_millis(200)),
            // Legacy rows (no conditions of their own): the child form's apply.
            Repeater::make("extras")
                .relationship()
                .deletable(false)
                .schema(components![
                    TextInput::make("product"),
                    NumberInput::make("quantity"),
                    TextInput::make("wrap"),
                    TextInput::make("ribbon"),
                ])
                .reacts(script),
        ])
    }

    fn relations() -> Vec<Relation> {
        vec![
            Relation::has_many("lines", "order_lines", "order_id").where_eq("kind", 1),
            Relation::has_many("extras", "order_lines", "order_id").where_eq("kind", 2),
        ]
    }
}

/// Lines: the panel's permissions; "noupdate" may not be updated, "nodelete"
/// not deleted, "secret" not viewed.
struct LinePolicy;

#[async_trait::async_trait]
impl Policy for LinePolicy {
    async fn allows(&self, ctx: &Ctx, ability: &str, record: Option<&Data>) -> bool {
        let product = record.and_then(|r| r.get("product")).and_then(Value::as_str).unwrap_or_default();
        let refused = matches!(
            (ability, product),
            ("update", "noupdate") | ("delete", "nodelete") | ("view", "secret") | ("update" | "delete", "secret")
        );
        ctx.can(&format!("order_lines.{ability}")) && !refused
    }

    async fn admits(&self, ctx: &Ctx, ability: &str) -> bool {
        ctx.can(&format!("order_lines.{ability}"))
    }

    fn view_depends_on_record(&self) -> bool {
        true
    }
}

pub struct LineResource;

impl Resource for LineResource {
    type Entity = line::Entity;
    const SLUG: &'static str = "order_lines";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Line").title("product").soft_deletes().audit(false).lock_parent("order_id", "orders")
    }

    fn fields() -> Vec<Field> {
        use line::Column as C;
        vec![
            Field::column(C::OrderId).required().belongs_to("orders"),
            Field::column(C::Kind),
            Field::column(C::Product).required(),
            Field::column(C::Quantity),
            Field::column(C::GiftNote),
            // Written on create only.
            Field::column(C::Code).updatable(|_| false),
            // Written on update only.
            Field::column(C::Memo).creatable(|_| false),
            // Written, never read.
            Field::column(C::Secret).readable(|_| false),
            // Read, never written.
            Field::column(C::Locked).creatable(|_| false).updatable(|_| false),
            Field::column(C::Size),
            Field::column(C::Wrap),
            Field::column(C::Ribbon),
            Field::column(C::Position).guarded().readable(|_| true),
            Field::column(C::DeletedAt).guarded().readable(|_| true),
        ]
    }

    fn policy() -> std::sync::Arc<dyn Policy> {
        std::sync::Arc::new(LinePolicy)
    }

    // The child's form: what legacy rows' conditions read (inputs outside the rows too).
    fn form() -> Form {
        Form::new().schema(components![
            NumberInput::make("kind"),
            TextInput::make("product"),
            NumberInput::make("quantity"),
            TextInput::make("size"),
            TextInput::make("wrap").shown_when(when("size").eq("big")),
            TextInput::make("ribbon").shown_when(when("kind").eq(2)),
        ])
    }

    // Lines named "hidden" are outside everyone's scope.
    fn scope(query: sea_orm::Select<line::Entity>, _ctx: &Ctx) -> sea_orm::Select<line::Entity> {
        query.filter(sea_orm::ColumnTrait::ne(&line::Column::Product, "hidden"))
    }
}

struct Fixture {
    db: TestDb,
    registry: std::sync::Arc<Registry>,
    order: String,
}

impl Fixture {
    fn ctx(&self, principal: Principal) -> Ctx {
        Ctx::new(self.db.conn().clone(), principal, self.registry.clone())
    }

    /// The key of the row named `product`.
    async fn key(&self, product: &str) -> String {
        line::Entity::find()
            .filter(sea_orm::ColumnTrait::eq(&line::Column::Product, product))
            .one(self.db.conn())
            .await
            .unwrap()
            .unwrap()
            .id
            .to_string()
    }
}

/// Order "A" with the line "a" (created through its repeater) and, written
/// directly, the lines "noupdate", "nodelete", "hidden" (out of scope) and
/// "secret" (not viewable), the extras "small" (size small) and "big"; order
/// "B" with the line "other".
async fn fixture(db: TestDb) -> Fixture {
    let schema = Schema::new(db.conn().get_database_backend());
    rustcms_testing::create_table(db.conn(), &schema.create_table_from_entity(order::Entity)).await;
    rustcms_testing::create_table(db.conn(), &schema.create_table_from_entity(line::Entity)).await;
    let registry = Registry::new().register::<OrderResource>().register::<LineResource>().finish();
    let admin = Ctx::new(db.conn().clone(), support::admin(), registry.clone());
    let orders = registry.get("orders").unwrap();
    let create = |number: &str, product: &str| {
        data(
            json!({"number": number, "lines": {"create": [{"ref": "n1", "data": {"product": product, "quantity": 1}}]}}),
        )
    };
    let order = orders.create(&admin, create("A", "a")).await.unwrap().key;
    let other = orders.create(&admin, create("B", "other")).await.unwrap().key;
    for (kind, product, size, position) in [
        (1, "noupdate", "big", 2),
        (1, "nodelete", "big", 3),
        (1, "hidden", "big", 4),
        (1, "secret", "big", 5),
        (2, "small", "small", 6),
        (2, "big", "big", 7),
    ] {
        db.conn()
            .execute_unprepared(&format!(
                "INSERT INTO order_lines (order_id, kind, product, quantity, size, position) VALUES \
                 ({order}, {kind}, '{product}', 1, '{size}', {position})"
            ))
            .await
            .unwrap();
    }
    assert_ne!(order, other);
    Fixture { db, registry, order }
}

fn data(value: Value) -> Data {
    value.as_object().unwrap().clone()
}

/// Asks for the reaction of `changed` on the edit form of `key` (`None`: the
/// create form) with `state`; the answer's patches as JSON.
async fn react(ctx: &Ctx, key: Option<&str>, changed: &str, state: Value) -> Result<FormReactResponse, AppError> {
    let mut request = json!({"operation": "create", "changed": changed, "state": state, "seq": 1});
    if let Some(key) = key {
        request["operation"] = json!("edit");
        request["key"] = json!(key);
    }
    let request: FormReactRequest = serde_json::from_value(request).unwrap();
    ctx.registry.get("orders").unwrap().react(ctx, request).await
}

async fn patches(ctx: &Ctx, key: Option<&str>, changed: &str, state: Value) -> Value {
    let answer = react(ctx, key, changed, state).await.unwrap();
    assert!(!answer.failed, "{answer:?}");
    serde_json::to_value(&answer.patches).unwrap()
}

/// The rows of `name` the reaction last saw, as `(id, values)`.
fn seen(name: &str) -> Vec<(RowId, Value)> {
    let state = SEEN.lock().unwrap().clone().expect("the reaction ran");
    state.relation_rows(name).iter().map(|r| (r.id().clone(), Value::Object(r.values().clone()))).collect()
}

fn key(k: &str) -> RowId {
    RowId::Key(k.to_owned())
}

fn reference(r: &str) -> RowId {
    RowId::Ref(r.to_owned())
}

db_test!(the_state_holds_the_rows_the_user_may_read, |db: TestDb| async move {
    let f = fixture(db).await;
    let ctx = f.ctx(support::admin());
    let a = f.key("a").await;
    let order = f.order.clone();
    // Tagged identities; only the rows' inputs the user may read, values that decode, canonical.
    patches(
        &ctx,
        Some(&order),
        "number",
        json!({"number": "A", "lines": [
            {"key": a, "data": {"product": "a", "quantity": 7, "secret": "x", "bogus": 1, "locked": "L"}},
            {"ref": "n1", "data": {"product": "new", "quantity": "abc"}},
        ]}),
    )
    .await;
    assert_eq!(
        seen("lines"),
        [
            (key(&a), json!({"product": "a", "quantity": 7, "locked": "L"})),
            (reference("n1"), json!({"product": "new"})),
        ]
    );
    // A row twice, a key that is not one, a bad ref, an unknown shape: the repeater is left out.
    for rows in [
        json!([{"ref": "n1", "data": {}}, {"ref": "n1", "data": {}}]),
        json!([{"key": a, "data": {}}, {"key": a, "data": {}}]),
        json!([{"key": "n1", "data": {}}]),
        json!([{"ref": "x1", "data": {}}]),
        json!([{"ref": "n1234567890", "data": {}}]),
        json!([{"key": a, "ref": "n1", "data": {}}]),
        json!([{"ref": "n1", "data": [1]}]),
        json!({"ref": "n1"}),
    ] {
        patches(&ctx, Some(&order), "number", json!({"number": "A", "lines": rows.clone()})).await;
        assert!(seen("lines").is_empty(), "{rows}");
    }
    // More rows than `[admin] repeater_rows` (50 by default).
    let many: Vec<Value> = (1..=51).map(|i| json!({"ref": format!("n{i}"), "data": {}})).collect();
    patches(&ctx, Some(&order), "number", json!({"number": "A", "lines": many})).await;
    assert!(seen("lines").is_empty());
    // Hidden by its conditions.
    patches(&ctx, Some(&order), "number", json!({"number": "hide", "lines": [{"ref": "n1", "data": {}}]})).await;
    assert!(seen("lines").is_empty());
    // No stored rows on a create form.
    patches(&ctx, None, "number", json!({"number": "A", "lines": [{"key": a, "data": {}}]})).await;
    assert!(seen("lines").is_empty());
    patches(&ctx, None, "number", json!({"number": "A", "lines": [{"ref": "n2", "data": {"product": "p"}}]})).await;
    assert_eq!(seen("lines"), [(reference("n2"), json!({"product": "p"}))]);
    // A user who may not list the child: no rows in the state.
    let blind = f.ctx(Principal { id: "7".into(), guard: "admin".into(), ..Default::default() }.with(&[
        "orders.view_any",
        "orders.view",
        "orders.update",
    ]));
    patches(&blind, Some(&order), "number", json!({"number": "A", "lines": [{"ref": "n1", "data": {}}]})).await;
    assert!(seen("lines").is_empty());
});

db_test!(row_patches_are_kept_when_the_user_could_make_them, |db: TestDb| async move {
    let f = fixture(db).await;
    let ctx = f.ctx(support::admin());
    let order = f.order.clone();
    let [a, noupdate, nodelete, hidden, secret, small, other] = [
        f.key("a").await,
        f.key("noupdate").await,
        f.key("nodelete").await,
        f.key("hidden").await,
        f.key("secret").await,
        f.key("small").await,
        f.key("other").await,
    ];
    let state = json!({"number": "A", "lines": [
        {"key": a, "data": {"product": "a"}},
        {"key": noupdate, "data": {"product": "noupdate"}},
        {"key": nodelete, "data": {"product": "nodelete"}},
        {"key": hidden, "data": {}},
        {"key": other, "data": {}},
        {"key": small, "data": {}},
        {"key": secret, "data": {}},
        {"ref": "n1", "data": {"product": "new"}},
    ]});
    let update = |id: RowId, data: Value| FormPatch::update_relation_row("lines", id, data);
    play(vec![
        update(key(&a), json!({"locked": "x"})),                    // not writable
        update(key(&a), json!({"secret": "x"})),                    // not readable
        update(key(&a), json!({"code": "c"})),                      // written on create only
        update(reference("n1"), json!({"code": "c"})),              // a new row: kept
        update(key(&a), json!({"memo": "m"})),                      // written on update: kept
        update(key(&a), json!({"quantity": "many"})),               // a value its input refuses
        update(key(&a), json!({"position": 1})),                    // the order column
        update(key(&a), json!({"kind": 2})),                        // the relation's filter
        update(key(&a), json!({"order_id": other})),                // the parent key
        update(key(&a), json!({"product": "a"})),                   // no change
        FormPatch::add_relation_row("lines", json!({"memo": "m"})), // written on update only
        FormPatch::add_relation_row_as("lines", "n5", json!({"product": "z", "quantity": 2})),
        FormPatch::add_relation_row_as("lines", "n1", json!({"product": "taken"})), // a ref of the state
        FormPatch::add_relation_row("lines", json!({"product": "y"})),              // over max_items (9)
        update(key(&hidden), json!({"product": "q"})),                              // out of scope
        update(key(&other), json!({"product": "q"})),                               // another parent's
        update(key(&small), json!({"product": "q"})),                               // outside the relation's filter
        update(key(&secret), json!({"product": "q"})),                              // not viewable
        update(key(&noupdate), json!({"quantity": 5})),                             // not updatable
        FormPatch::remove_relation_row("lines", key(&nodelete)),                    // not deletable
        FormPatch::remove_relation_row("lines", key(&noupdate)),                    // deletable: kept
        update(key(&a.to_string()), json!({"quantity": 3})),                        // kept
        update(key("n1"), json!({"product": "k"})),                                 // a key `n1` is not the ref `n1`
        update(reference("n1"), json!({"product": "k"})),                           // kept
        update(reference("n9"), json!({"product": "k"})),                           // no such row
        FormPatch::flag_relation_row("lines", reference("n5"), "Check"),
        FormPatch::flag_relation_row("lines", key(&noupdate), "Gone"), // removed above
        update(key(&a), json!({"product": "gift", "gift_note": "wrap it"})), // shown for gifts
        update(key(&a), json!({"product": "plain", "gift_note": "no"})), // hidden: left out
    ]);
    assert_eq!(
        patches(&ctx, Some(&order), "lines", state).await,
        json!([
            {"op": "update_relation_row", "field": "lines", "id": {"ref": "n1"}, "data": {"code": "c"}},
            {"op": "update_relation_row", "field": "lines", "id": {"key": a}, "data": {"memo": "m"}},
            {"op": "add_relation_row", "field": "lines", "ref": "n5", "data": {"product": "z", "quantity": 2}},
            {"op": "remove_relation_row", "field": "lines", "id": {"key": noupdate}},
            {"op": "update_relation_row", "field": "lines", "id": {"key": a}, "data": {"quantity": 3}},
            {"op": "update_relation_row", "field": "lines", "id": {"ref": "n1"}, "data": {"product": "k"}},
            {"op": "flag_relation_row", "field": "lines", "id": {"ref": "n5"}, "message": "Check"},
            {"op": "update_relation_row", "field": "lines", "id": {"key": a}, "data": {"product": "gift", "gift_note": "wrap it"}},
            {"op": "update_relation_row", "field": "lines", "id": {"key": a}, "data": {"product": "plain"}},
        ])
    );
    // The repeater needs a row: the last one stays, a new one may go first.
    play(vec![
        FormPatch::remove_relation_row("lines", reference("n1")),
        FormPatch::remove_relation_row("lines", key(&a)),
    ]);
    let state = json!({"number": "A", "lines": [{"key": a, "data": {}}, {"ref": "n1", "data": {}}]});
    assert_eq!(
        patches(&ctx, Some(&order), "lines", state).await,
        json!([{"op": "remove_relation_row", "field": "lines", "id": {"ref": "n1"}}])
    );
    // Rows the form did not send, or hides: nothing to patch.
    play(vec![FormPatch::add_relation_row("lines", json!({"product": "p"}))]);
    assert_eq!(patches(&ctx, Some(&order), "number", json!({"number": "A"})).await, json!([]));
    assert_eq!(patches(&ctx, Some(&order), "number", json!({"number": "hide", "lines": []})).await, json!([]));
});

db_test!(row_conditions_read_authorized_values, |db: TestDb| async move {
    let f = fixture(db).await;
    let ctx = f.ctx(support::admin());
    let order = f.order.clone();
    let (small, big) = (f.key("small").await, f.key("big").await);
    let update = |id: &str, data: Value| FormPatch::update_relation_row("extras", key(id), data);
    // Legacy rows: the child form's conditions, over its inputs outside the
    // rows (the stored size; a new row's declared default, big) and the
    // relation's fixed value (kind 2 shows the ribbon).
    play(vec![
        update(&small, json!({"wrap": "w"})),
        update(&small, json!({"wrap": "w", "ribbon": "r", "quantity": 3})),
        update(&big, json!({"wrap": "w"})),
        FormPatch::add_relation_row("extras", json!({"product": "e", "wrap": "w", "ribbon": "r"})),
    ]);
    let state = json!({"number": "A", "extras": [{"key": small, "data": {}}, {"key": big, "data": {}}]});
    assert_eq!(
        patches(&ctx, Some(&order), "extras", state).await,
        json!([
            {"op": "update_relation_row", "field": "extras", "id": {"key": small}, "data": {"ribbon": "r", "quantity": 3}},
            {"op": "update_relation_row", "field": "extras", "id": {"key": big}, "data": {"wrap": "w"}},
            {"op": "add_relation_row", "field": "extras", "data": {"product": "e", "wrap": "w", "ribbon": "r"}},
        ])
    );
});

db_test!(on_a_new_record_rows_are_new_rows, |db: TestDb| async move {
    let f = fixture(db).await;
    let ctx = f.ctx(support::admin());
    let a = f.key("a").await;
    play(vec![
        FormPatch::update_relation_row("lines", key(&a), json!({"product": "x"})), // no stored rows
        FormPatch::remove_relation_row("lines", key(&a)),
        FormPatch::add_relation_row_as("lines", "n7", json!({"product": "p", "code": "c"})),
        FormPatch::update_relation_row("lines", reference("n7"), json!({"memo": "m"})), // update only
        FormPatch::update_relation_row("lines", reference("n7"), json!({"quantity": 4})),
        FormPatch::remove_relation_row("lines", reference("n1")),
        FormPatch::remove_relation_row("lines", reference("n7")), // the repeater needs a row
        FormPatch::add_relation_row("extras", json!({"product": "e"})),
        FormPatch::remove_relation_row("extras", reference("n2")), // extras are not removed
    ]);
    let state = json!({"number": "A", "lines": [{"ref": "n1", "data": {}}], "extras": [{"ref": "n2", "data": {}}]});
    assert_eq!(
        patches(&ctx, None, "number", state).await,
        json!([
            {"op": "add_relation_row", "field": "lines", "ref": "n7", "data": {"product": "p", "code": "c"}},
            {"op": "update_relation_row", "field": "lines", "id": {"ref": "n7"}, "data": {"quantity": 4}},
            {"op": "remove_relation_row", "field": "lines", "id": {"ref": "n1"}},
            {"op": "add_relation_row", "field": "extras", "data": {"product": "e"}},
        ])
    );
});

db_test!(row_patches_follow_what_the_user_may_do_with_the_rows, |db: TestDb| async move {
    let f = fixture(db).await;
    let order = f.order.clone();
    let a = f.key("a").await;
    let orders = ["orders.view_any", "orders.view", "orders.create", "orders.update"];
    let user = |lines: &[&str]| {
        let mut all: Vec<&str> = orders.to_vec();
        all.extend_from_slice(lines);
        f.ctx(Principal { id: "7".into(), guard: "admin".into(), ..Default::default() }.with(&all))
    };
    // Lines updated, not created or deleted.
    let clerk = user(&["order_lines.view_any", "order_lines.view", "order_lines.update"]);
    play(vec![
        FormPatch::add_relation_row("lines", json!({"product": "p"})),
        FormPatch::update_relation_row("lines", reference("n1"), json!({"product": "p"})),
        FormPatch::update_relation_row("lines", key(&a), json!({"quantity": 2})),
        FormPatch::remove_relation_row("lines", key(&a)),
    ]);
    let state = json!({"number": "A", "lines": [{"key": a, "data": {}}, {"ref": "n1", "data": {}}]});
    assert_eq!(
        patches(&clerk, Some(&order), "lines", state.clone()).await,
        json!([{"op": "update_relation_row", "field": "lines", "id": {"key": a}, "data": {"quantity": 2}}])
    );
    // Lines listed only: the rows do not react, and the user's form changes none.
    let reader = user(&["order_lines.view_any", "order_lines.view"]);
    let err = react(&reader, Some(&order), "lines", state.clone()).await.unwrap_err();
    assert!(matches!(err, AppError::Validation(_)), "{err:?}");
    assert_eq!(patches(&reader, Some(&order), "number", state).await, json!([]));
    // The schema says which repeaters react.
    let reacting = |schema: &ResourceSchema| -> Vec<String> {
        let form = serde_json::to_value(&schema.form).unwrap();
        fn walk(value: &Value, out: &mut Vec<String>) {
            if value["reacts"] == json!(true) {
                out.push(value["name"].as_str().unwrap().to_owned());
            }
            for child in value["components"].as_array().into_iter().flatten() {
                walk(child, out);
            }
        }
        let mut out = Vec::new();
        walk(&form, &mut out);
        out
    };
    let orders_resource = f.registry.get("orders").unwrap();
    let admin = f.ctx(support::admin());
    let edit = orders_resource.schema(&admin, SchemaOperation::Edit, Some(&order)).await.unwrap();
    assert_eq!(reacting(&edit), ["number", "lines", "extras"]);
    let edit = orders_resource.schema(&reader, SchemaOperation::Edit, Some(&order)).await.unwrap();
    assert_eq!(reacting(&edit), ["number"]);
    let view = orders_resource.schema(&admin, SchemaOperation::View, Some(&order)).await.unwrap();
    assert!(reacting(&view).is_empty());
});

db_test!(row_patches_share_the_reactions_deadline, |db: TestDb| async move {
    let f = fixture(db).await;
    let ctx = f.ctx(support::admin());
    let order = f.order.clone();
    let a = f.key("a").await;
    SLOW.store(true, std::sync::atomic::Ordering::SeqCst);
    play(vec![FormPatch::update_relation_row("lines", key(&a), json!({"quantity": 2}))]);
    let answer =
        react(&ctx, Some(&order), "lines", json!({"number": "A", "lines": [{"key": a, "data": {}}]})).await.unwrap();
    assert!(answer.failed && answer.patches.is_empty(), "{answer:?}");
});
