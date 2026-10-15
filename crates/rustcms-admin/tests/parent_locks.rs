//! `ResourceMeta::lock_parent`: writes to a child lock its parent first, so a
//! rule over a parent's children holds under concurrency, writes never
//! deadlock with the parent's own deletion, and nothing moves a record to a
//! parent that was not locked. SQLite, PostgreSQL and MySQL.

use std::sync::Arc;

use axum::http::{Method, StatusCode};
use rustcms_admin::{Action, Ctx, Data, Field, Policy, Registry, Resource, ResourceMeta};
use rustcms_core::{AppResult, ValidationErrors};
use rustcms_testing::{Factory, TestApp, TestClient, TestDb, TestResponse, UserFactory, db_test};
use sea_orm::sea_query::{ColumnDef, ForeignKey, ForeignKeyAction, Table};
use sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseTransaction, DbBackend, EntityTrait, PaginatorTrait, QueryFilter,
    QuerySelect, Set,
};
use serde_json::json;

pub mod shelf {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "shelves")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i32,
        pub name: String,
        pub capacity: i32,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod book {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "books")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i32,
        pub shelf_id: i32,
        pub title: String,
        pub deleted_at: Option<DateTimeUtc>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

struct Shelves;

impl Resource for Shelves {
    type Entity = shelf::Entity;
    const SLUG: &'static str = "shelves";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Shelf").title("name")
    }

    fn fields() -> Vec<Field> {
        vec![Field::column(shelf::Column::Name).required(), Field::column(shelf::Column::Capacity).required()]
    }
}

/// Books on a shelf, at most `capacity` (active ones).
struct Books;

/// Allows everything; updating a book titled `drift` first moves it to
/// another shelf from another connection (before the engine locks anything).
struct DriftPolicy;

#[async_trait::async_trait]
impl Policy for DriftPolicy {
    async fn allows(&self, ctx: &Ctx, ability: &str, record: Option<&Data>) -> bool {
        // `race`: while this update waits for its shelf, the shelf is hidden
        // (its owner changes) and committed; the scope, checked again under
        // the shelf's lock, no longer admits the book.
        if ability == "update"
            && ctx.txn.is_some()
            && let Some(record) = record
            && record["title"] == "race"
        {
            let shelf: i32 = record["shelf_id"].as_i64().unwrap() as i32;
            let db = ctx.db.clone();
            let (locked, ready) = tokio::sync::oneshot::channel();
            tokio::spawn(async move {
                use sea_orm::TransactionTrait;
                let txn = db.begin().await.unwrap();
                shelf::Entity::find_by_id(shelf).lock_exclusive().one(&txn).await.unwrap();
                locked.send(()).ok();
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                shelf::Entity::update_many()
                    .col_expr(shelf::Column::Name, sea_orm::sea_query::Expr::value("hidden"))
                    .filter(shelf::Column::Id.eq(shelf))
                    .exec(&txn)
                    .await
                    .unwrap();
                txn.commit().await.unwrap();
            });
            ready.await.ok();
        }
        // Only inside the update's transaction (not for a response's abilities),
        // and once: the record is not on the target shelf yet.
        if ability == "update"
            && ctx.txn.is_some()
            && let Some(record) = record
            && record["title"] == "drift"
        {
            let id: i32 =
                record["id"].as_str().map_or_else(|| record["id"].as_i64().unwrap() as i32, |s| s.parse().unwrap());
            let db = ctx.db.clone();
            tokio::spawn(async move {
                let target = shelf::Entity::find().filter(shelf::Column::Name.eq("target")).one(&db).await.unwrap();
                let to = target.unwrap().id;
                let current = book::Entity::find_by_id(id).one(&db).await.unwrap().unwrap();
                if current.shelf_id == to {
                    return;
                }
                book::Entity::update_many()
                    .col_expr(book::Column::ShelfId, sea_orm::sea_query::Expr::value(to))
                    .filter(book::Column::Id.eq(id))
                    .exec(&db)
                    .await
                    .unwrap();
            })
            .await
            .unwrap();
        }
        true
    }
}

async fn ensure_room(ctx: &Ctx, txn: &DatabaseTransaction, shelf_id: i64, except: Option<i32>) -> AppResult<()> {
    let shelf = shelf::Entity::find_by_id(shelf_id as i32).one(txn).await?;
    let Some(shelf) = shelf else { return Ok(()) };
    let mut books =
        book::Entity::find().filter(book::Column::ShelfId.eq(shelf.id)).filter(book::Column::DeletedAt.is_null());
    if let Some(id) = except {
        books = books.filter(book::Column::Id.ne(id));
    }
    // A pause between reading and writing widens the race the lock closes.
    let taken = books.count(txn).await?;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    if taken >= shelf.capacity as u64 {
        return Err(ValidationErrors::field("shelf_id", ctx.t("is full")).into());
    }
    Ok(())
}

fn int(data: &Data, field: &str) -> Option<i64> {
    match data.get(field)? {
        serde_json::Value::String(text) => text.parse().ok(),
        value => value.as_i64(),
    }
}

#[async_trait::async_trait]
impl Resource for Books {
    type Entity = book::Entity;
    const SLUG: &'static str = "books";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Book").title("title").soft_deletes().lock_parent("shelf_id", "shelves")
    }

    fn fields() -> Vec<Field> {
        vec![
            Field::column(book::Column::ShelfId).belongs_to("shelves").required(),
            Field::column(book::Column::Title).required(),
        ]
    }

    fn policy() -> Arc<dyn Policy> {
        Arc::new(DriftPolicy)
    }

    /// Books on hidden shelves are out of reach (ownership read through the parent).
    fn scope(query: sea_orm::Select<book::Entity>, _ctx: &Ctx) -> sea_orm::Select<book::Entity> {
        use sea_orm::sea_query::{Expr, ExprTrait, Query};
        let hidden = Query::select()
            .column(shelf::Column::Id)
            .from(shelf::Entity)
            .and_where(Expr::col(shelf::Column::Name).eq("hidden"))
            .to_owned();
        query.filter(Expr::col((book::Entity, book::Column::ShelfId)).not_in_subquery(hidden))
    }

    fn actions() -> Vec<Action<book::Entity>> {
        vec![
            // Moves records itself: refused, the target shelf is not locked.
            Action::<book::Entity>::new("restack", |_ctx, txn, books, _data| {
                Box::pin(async move {
                    for book in books {
                        let mut active: book::ActiveModel = book.into();
                        active.shelf_id = Set(active.shelf_id.unwrap() + 1);
                        sea_orm::ActiveModelTrait::update(active, txn).await?;
                    }
                    Ok(())
                })
            })
            .bulk(),
            // Changes nothing but the title: fine.
            Action::<book::Entity>::new("rename", |_ctx, txn, books, _data| {
                Box::pin(async move {
                    for book in books {
                        let mut active: book::ActiveModel = book.into();
                        active.title = Set(format!("{} (renamed)", active.title.unwrap()));
                        sea_orm::ActiveModelTrait::update(active, txn).await?;
                    }
                    Ok(())
                })
            })
            .bulk(),
        ]
    }

    async fn mutate_before_create(ctx: &Ctx, txn: &DatabaseTransaction, mut data: Data) -> AppResult<Data> {
        let shelf = int(&data, "shelf_id").unwrap_or_default();
        ensure_room(ctx, txn, shelf, None).await?;
        if data["title"] == "sneak" {
            // A hook putting the record under a parent that was not locked.
            data.insert("shelf_id".into(), json!(shelf + 1));
        }
        Ok(data)
    }

    async fn mutate_before_update(
        ctx: &Ctx,
        txn: &DatabaseTransaction,
        record: &book::Model,
        data: Data,
    ) -> AppResult<Data> {
        // Like a seat check: the hook reads its shelf under a row lock. Without
        // the parent locked first, that lock (child, then parent) deadlocks
        // with deleting the shelf (parent, then its children by cascade).
        let mut current = shelf::Entity::find_by_id(record.shelf_id);
        if txn.get_database_backend() != DbBackend::Sqlite {
            current = current.lock_exclusive();
        }
        current.one(txn).await?;
        if let Some(shelf) = int(&data, "shelf_id")
            && shelf != i64::from(record.shelf_id)
        {
            ensure_room(ctx, txn, shelf, Some(record.id)).await?;
        }
        Ok(data)
    }
}

/// Books whose shelf may be left out: the column's default (shelf 1) is
/// filled in by the database, not locked; refused.
struct Drafts;

impl Resource for Drafts {
    type Entity = book::Entity;
    const SLUG: &'static str = "drafts";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Draft").title("title").soft_deletes().lock_parent("shelf_id", "shelves")
    }

    fn fields() -> Vec<Field> {
        vec![Field::column(book::Column::ShelfId).belongs_to("shelves"), Field::column(book::Column::Title).required()]
    }
}

fn build(config: rustcms_core::Config) -> rustcms_core::AppBuilder {
    let mut app = rustcms_core::AppBuilder::new(config);
    app.provider(rustcms_auth::AuthProvider::new());
    app.provider(rustcms_admin::AdminProvider::new().resource::<Shelves>().resource::<Books>().resource::<Drafts>());
    app
}

async fn setup(db: TestDb) -> (TestApp, rustcms_core::models::user::Model) {
    db.migrate::<rustcms_migration::Migrator>().await;
    let shelves = Table::create()
        .table(shelf::Entity)
        .col(ColumnDef::new(shelf::Column::Id).integer().not_null().auto_increment().primary_key())
        .col(ColumnDef::new(shelf::Column::Name).string().not_null())
        .col(ColumnDef::new(shelf::Column::Capacity).integer().not_null())
        .to_owned();
    // Deleting a shelf deletes its books (a cascade: parent, then children).
    let books = Table::create()
        .table(book::Entity)
        .col(ColumnDef::new(book::Column::Id).integer().not_null().auto_increment().primary_key())
        .col(ColumnDef::new(book::Column::ShelfId).integer().not_null().default(1))
        .col(ColumnDef::new(book::Column::Title).string().not_null())
        .col(ColumnDef::new(book::Column::DeletedAt).timestamp_with_time_zone().null())
        .foreign_key(
            ForeignKey::create()
                .name("books_shelf_id_fkey")
                .from(book::Entity, book::Column::ShelfId)
                .to(shelf::Entity, shelf::Column::Id)
                .on_delete(ForeignKeyAction::Cascade),
        )
        .to_owned();
    rustcms_testing::create_table(db.conn(), &shelves).await;
    rustcms_testing::create_table(db.conn(), &books).await;
    let app = TestApp::new(build, db).await;
    let admin = UserFactory::new().create(app.state().db()).await;
    app.make_super_admin(&admin).await;
    (app, admin)
}

async fn shelf(app: &TestApp, capacity: i32) -> i32 {
    let model = shelf::ActiveModel { name: Set("s".into()), capacity: Set(capacity), ..Default::default() };
    sea_orm::ActiveModelTrait::insert(model, app.state().db()).await.unwrap().id
}

async fn create(app: &TestApp, client: &TestClient<'_>, shelf: i32, title: &str) -> TestResponse {
    let body = json!({"shelf_id": shelf, "title": title});
    client.post_json(&app.admin_api("resources/books/records"), &body).await
}

fn record(app: &TestApp, key: &str) -> String {
    app.admin_api(&format!("resources/books/records/{key}"))
}

fn key(response: &TestResponse) -> String {
    response.json()["data"]["key"].as_str().unwrap().to_owned()
}

async fn idempotent(app: &TestApp, client: &TestClient<'_>, shelf: i32) -> TestResponse {
    let request = axum::http::Request::post(app.admin_api("resources/books/records"))
        .header("content-type", "application/json")
        .header("idempotency-key", "same-key")
        .body(axum::body::Body::from(json!({"shelf_id": shelf, "title": "once"}).to_string()))
        .unwrap();
    client.request(request).await
}

fn sorted(a: &TestResponse, b: &TestResponse) -> [StatusCode; 2] {
    let mut statuses = [a.status, b.status];
    statuses.sort();
    statuses
}

db_test!(children_of_one_parent_are_written_one_at_a_time, |db: TestDb| async move {
    let (app, admin) = setup(db).await;
    let (one, other) = (app.acting_as(&admin).await, app.acting_as(&admin).await);

    // The last place on a shelf goes to one of two creates.
    let s = shelf(&app, 1).await;
    let (a, b) = tokio::join!(create(&app, &one, s, "a"), create(&app, &other, s, "b"));
    assert_eq!(sorted(&a, &b), [StatusCode::CREATED, StatusCode::UNPROCESSABLE_ENTITY], "{} / {}", a.text(), b.text());

    // Moving into a shelf races with creating on it: both parents are locked.
    let (from, to) = (shelf(&app, 5).await, shelf(&app, 1).await);
    let moving = key(&create(&app, &one, from, "moving").await);
    let url = record(&app, &moving);
    let moved = json!({"shelf_id": to});
    let (a, b) = tokio::join!(one.send(Method::PATCH, &url, Some(&moved)), create(&app, &other, to, "c"));
    let mut statuses = [a.status, b.status];
    statuses.sort();
    assert_eq!(statuses[1], StatusCode::UNPROCESSABLE_ENTITY, "{} / {}", a.text(), b.text());
    assert!(statuses[0].is_success(), "{} / {}", a.text(), b.text());
    let on_target = book::Entity::find().filter(book::Column::ShelfId.eq(to)).count(app.state().db()).await.unwrap();
    assert_eq!(on_target, 1);

    // An idempotent create retried at once: one book, the same answer.
    let s = shelf(&app, 5).await;
    let (a, b) = tokio::join!(idempotent(&app, &one, s), idempotent(&app, &one, s));
    let codes = [a.status, b.status];
    assert!(
        codes.iter().all(|c| *c == StatusCode::CREATED || *c == StatusCode::CONFLICT),
        "{} / {}",
        a.text(),
        b.text()
    );
    assert_eq!(book::Entity::find().filter(book::Column::Title.eq("once")).count(app.state().db()).await.unwrap(), 1);
});

db_test!(deleting_a_parent_while_its_children_change_never_deadlocks, |db: TestDb| async move {
    let (app, admin) = setup(db).await;
    let (one, other) = (app.acting_as(&admin).await, app.acting_as(&admin).await);
    for round in 0..6 {
        let s = shelf(&app, 10).await;
        let mut books = Vec::new();
        for i in 0..3 {
            books.push(key(&create(&app, &one, s, &format!("r{round}b{i}")).await));
        }
        let shelf_url = app.admin_api(&format!("resources/shelves/records/{s}"));
        let renames = async {
            let mut out = Vec::new();
            for book in &books {
                let body = json!({"title": format!("{book} renamed")});
                out.push(one.send(Method::PATCH, &record(&app, book), Some(&body)).await);
            }
            out
        };
        let (renamed, deleted) = tokio::join!(renames, other.send(Method::DELETE, &shelf_url, None));
        deleted.assert_status(StatusCode::NO_CONTENT);
        for response in renamed {
            // Before the shelf went: renamed; after: gone. Never a deadlock (409) or an error.
            assert!(
                response.status == StatusCode::OK || response.status == StatusCode::NOT_FOUND,
                "round {round}: {} {}",
                response.status,
                response.text()
            );
        }
    }
});

db_test!(nothing_moves_a_record_to_a_parent_that_was_not_locked, |db: TestDb| async move {
    let (app, admin) = setup(db).await;
    let client = app.acting_as(&admin).await;
    let (s, _next) = (shelf(&app, 5).await, shelf(&app, 5).await);

    // A hook: the create is refused and rolled back.
    create(&app, &client, s, "sneak").await.assert_status(StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(book::Entity::find().filter(book::Column::Title.eq("sneak")).count(app.state().db()).await.unwrap(), 0);

    // An action handler: refused, nothing changed; one that keeps parents runs.
    let book = key(&create(&app, &client, s, "stay").await);
    let action = |name: &str| app.admin_api(&format!("resources/books/actions/{name}"));
    let body = json!({"keys": [book.clone()], "data": {}});
    client.post_json(&action("restack"), &body).await.assert_status(StatusCode::INTERNAL_SERVER_ERROR);
    let stored = book::Entity::find_by_id(book.parse::<i32>().unwrap()).one(app.state().db()).await.unwrap().unwrap();
    assert_eq!((stored.shelf_id, stored.title.as_str()), (s, "stay"));
    client.post_json(&action("rename"), &body).await.assert_status(StatusCode::OK);

    // Soft delete, restore and force delete go through the same locks.
    client.send(Method::DELETE, &record(&app, &book), None).await.assert_status(StatusCode::NO_CONTENT);
    client.post_json(&format!("{}/restore", record(&app, &book)), &json!({})).await.assert_status(StatusCode::OK);
    client.send(Method::DELETE, &record(&app, &book), None).await.assert_status(StatusCode::NO_CONTENT);
    client
        .send(Method::DELETE, &format!("{}/force", record(&app, &book)), None)
        .await
        .assert_status(StatusCode::NO_CONTENT);
});

db_test!(a_record_moved_before_its_lock_is_a_conflict, |db: TestDb| async move {
    let (app, admin) = setup(db).await;
    // SQLite runs one writer at a time: the move cannot happen in between.
    if app.state().db().get_database_backend() == DbBackend::Sqlite {
        return;
    }
    let client = app.acting_as(&admin).await;
    let s = shelf(&app, 5).await;
    let target = shelf::ActiveModel { name: Set("target".into()), capacity: Set(5), ..Default::default() };
    let target = sea_orm::ActiveModelTrait::insert(target, app.state().db()).await.unwrap().id;
    let book = key(&create(&app, &client, s, "drift").await);
    let changed = client.send(Method::PATCH, &record(&app, &book), Some(&json!({"title": "kept?"}))).await;
    changed.assert_status(StatusCode::CONFLICT);
    assert_eq!(changed.json()["code"], json!("write_conflict"));
    let stored = book::Entity::find_by_id(book.parse::<i32>().unwrap()).one(app.state().db()).await.unwrap().unwrap();
    assert_eq!((stored.shelf_id, stored.title.as_str()), (target, "drift"), "nothing written");
});

#[test]
fn parent_locks_are_checked_at_boot() {
    struct ToNowhere;
    impl Resource for ToNowhere {
        type Entity = book::Entity;
        const SLUG: &'static str = "nowhere";
        fn meta() -> ResourceMeta {
            ResourceMeta::new("x").lock_parent("shelf_id", "missing")
        }
        fn fields() -> Vec<Field> {
            vec![Field::column(book::Column::ShelfId)]
        }
    }
    struct NotLinked;
    impl Resource for NotLinked {
        type Entity = book::Entity;
        const SLUG: &'static str = "unlinked";
        fn meta() -> ResourceMeta {
            ResourceMeta::new("x").lock_parent("shelf_id", "shelves")
        }
        fn fields() -> Vec<Field> {
            vec![Field::column(book::Column::ShelfId)]
        }
    }
    struct Chained;
    impl Resource for Chained {
        type Entity = book::Entity;
        const SLUG: &'static str = "chained";
        fn meta() -> ResourceMeta {
            ResourceMeta::new("x").lock_parent("shelf_id", "books")
        }
        fn fields() -> Vec<Field> {
            vec![Field::column(book::Column::ShelfId).belongs_to("books")]
        }
    }
    let fails = |registry: fn() -> Registry| std::panic::catch_unwind(move || registry().finish()).is_err();
    assert!(fails(|| Registry::new().register::<ToNowhere>()));
    assert!(fails(|| Registry::new().register::<Shelves>().register::<NotLinked>()));
    assert!(fails(|| Registry::new().register::<Shelves>().register::<Books>().register::<Chained>()));
    assert!(!fails(|| Registry::new().register::<Shelves>().register::<Books>()));
}

db_test!(a_default_parent_is_never_written_unlocked, |db: TestDb| async move {
    let (app, admin) = setup(db).await;
    let client = app.acting_as(&admin).await;
    let first = shelf(&app, 5).await;
    assert_eq!(first, 1, "the column default points here");
    let drafts = app.admin_api("resources/drafts/records");
    // Left out: the database fills in shelf 1, which nothing locked. Refused, rolled back.
    client.post_json(&drafts, &json!({"title": "defaulted"})).await.assert_status(StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(book::Entity::find().count(app.state().db()).await.unwrap(), 0);
    // Named: locked first, written.
    client.post_json(&drafts, &json!({"title": "named", "shelf_id": first})).await.assert_status(StatusCode::CREATED);
});

db_test!(an_owner_change_while_waiting_for_the_parent_is_seen, |db: TestDb| async move {
    let (app, admin) = setup(db).await;
    // SQLite runs one writer at a time: nothing can change in between.
    if app.state().db().get_database_backend() == DbBackend::Sqlite {
        return;
    }
    let client = app.acting_as(&admin).await;
    let s = shelf(&app, 5).await;
    let book = key(&create(&app, &client, s, "race").await);
    // The shelf is hidden while the update waits for it: out of scope under the lock.
    let changed = client.send(Method::PATCH, &record(&app, &book), Some(&json!({"title": "too late"}))).await;
    changed.assert_status(StatusCode::NOT_FOUND);
    let stored = book::Entity::find_by_id(book.parse::<i32>().unwrap()).one(app.state().db()).await.unwrap().unwrap();
    assert_eq!(stored.title, "race", "nothing written");
});
