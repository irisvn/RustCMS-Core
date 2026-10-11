//! Test entities, resources and per-test databases.
#![allow(dead_code)]

use std::sync::Arc;

use rustcms_admin::{Action, Ctx, Data, DynResource, Field, KeyStrategy, Policy, Registry, Resource, ResourceMeta};
use rustcms_core::{AppError, AppResult, Principal};
use rustcms_testing::TestDb;
use sea_orm::entity::prelude::*;
use sea_orm::sea_query::Value as SqlValue;
use sea_orm::{ActiveValue::Set, DatabaseTransaction, IntoActiveModel, Schema};
use serde_json::json;

pub mod category {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "categories")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i32,
        pub name: String,
        #[sea_orm(unique)]
        pub slug: String,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod post {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "posts")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        pub title: String,
        pub category_id: Option<i32>,
        pub author_id: i64,
        #[sea_orm(column_type = "Decimal(Some((10, 2)))")]
        pub price: Option<Decimal>,
        #[sea_orm(default_value = false)]
        pub published: bool,
        pub published_at: Option<DateTimeUtc>,
        #[sea_orm(default_value = "")]
        pub secret: String,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod tag {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "tags")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub slug: String,
        pub label: String,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub struct CategoryResource;

impl Resource for CategoryResource {
    type Entity = category::Entity;
    const SLUG: &'static str = "categories";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Category").title("name").unique("categories_slug_key", &["slug"], "has already been taken")
    }

    fn fields() -> Vec<Field> {
        vec![
            Field::column(category::Column::Name).required().searchable().sortable(),
            Field::column(category::Column::Slug).required().filterable(),
        ]
    }

    fn scope(query: Select<category::Entity>, ctx: &Ctx) -> Select<category::Entity> {
        if ctx.can("categories.view_hidden") { query } else { query.filter(category::Column::Name.not_like("Hidden%")) }
    }
}

pub struct PostPolicy;

#[async_trait::async_trait]
impl Policy for PostPolicy {
    async fn allows(&self, ctx: &Ctx, ability: &str, record: Option<&Data>) -> bool {
        let granted = ctx.can(&format!("posts.{ability}"));
        match (ability, record) {
            ("publish", Some(r)) => granted && !r["title"].as_str().unwrap_or_default().starts_with("locked"),
            _ => granted,
        }
    }
}

pub struct PostResource;

#[async_trait::async_trait]
impl Resource for PostResource {
    type Entity = post::Entity;
    const SLUG: &'static str = "posts";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Post").title("title").key(KeyStrategy::AppGenerated(|| SqlValue::Uuid(Some(Uuid::now_v7()))))
    }

    fn fields() -> Vec<Field> {
        vec![
            Field::column(post::Column::Title).required().searchable().sortable(),
            Field::column(post::Column::CategoryId).belongs_to("categories").filterable(),
            Field::column(post::Column::AuthorId).guarded().readable(|_| true),
            Field::column(post::Column::Price),
            Field::column(post::Column::Published)
                .creatable(|ctx| ctx.can("posts.publish"))
                .updatable(|ctx| ctx.can("posts.publish"))
                .filterable(),
            Field::column(post::Column::PublishedAt),
        ]
    }

    fn policy() -> Arc<dyn Policy> {
        Arc::new(PostPolicy)
    }

    fn scope(query: Select<post::Entity>, ctx: &Ctx) -> Select<post::Entity> {
        if ctx.can("posts.manage_all") {
            query
        } else {
            query.filter(post::Column::AuthorId.eq(ctx.principal.id.parse::<i64>().unwrap_or(-1)))
        }
    }

    fn actions() -> Vec<Action<post::Entity>> {
        vec![Action::new("publish", |_ctx, txn, models: Vec<post::Model>, _data| {
            Box::pin(async move {
                for model in models {
                    let mut active = model.into_active_model();
                    active.published = Set(true);
                    active.update(txn).await?;
                }
                Ok(())
            })
        })]
    }

    async fn mutate_before_create(ctx: &Ctx, _txn: &DatabaseTransaction, mut data: Data) -> AppResult<Data> {
        data.insert("author_id".into(), json!(ctx.principal.id));
        Ok(data)
    }
}

pub struct TagResource;

#[async_trait::async_trait]
impl Resource for TagResource {
    type Entity = tag::Entity;
    const SLUG: &'static str = "tags";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Tag").title("label").key(KeyStrategy::Natural)
    }

    fn fields() -> Vec<Field> {
        vec![Field::column(tag::Column::Slug).required(), Field::column(tag::Column::Label).required()]
    }

    async fn after_save(_ctx: &Ctx, _txn: &DatabaseTransaction, record: &tag::Model) -> AppResult<()> {
        if record.label == "boom" { Err(AppError::internal("boom")) } else { Ok(()) }
    }
}

pub fn admin() -> Principal {
    Principal { id: "1".into(), guard: "admin".into(), super_admin: true, ..Default::default() }
}

pub fn editor(id: &str) -> Principal {
    let permissions = [
        "posts.view_any",
        "posts.view",
        "posts.create",
        "posts.update",
        "posts.delete",
        "categories.view_any",
        "categories.view",
        "tags.view_any",
        "tags.view",
    ];
    Principal { id: id.into(), guard: "admin".into(), ..Default::default() }.with(&permissions)
}

pub trait PrincipalExt {
    fn with(self, permissions: &[&str]) -> Self;
    fn without_all(self) -> Self;
}

impl PrincipalExt for Principal {
    fn with(mut self, permissions: &[&str]) -> Self {
        self.permissions.extend(permissions.iter().map(|p| String::from(*p)));
        self
    }

    fn without_all(mut self) -> Self {
        self.permissions.clear();
        self
    }
}

pub struct Fixture {
    pub db: TestDb,
    registry: Arc<Registry>,
}

impl Fixture {
    /// Creates the test tables on `db` and registers the resources.
    pub async fn new(db: TestDb) -> Self {
        let conn = db.conn();
        let schema = Schema::new(conn.get_database_backend());
        for statement in [
            schema.create_table_from_entity(category::Entity),
            schema.create_table_from_entity(post::Entity),
            schema.create_table_from_entity(tag::Entity),
        ] {
            conn.execute(&statement).await.expect("create table");
        }
        let registry = Registry::new()
            .register::<CategoryResource>()
            .register::<PostResource>()
            .register::<TagResource>()
            .finish();
        Self { db, registry }
    }

    pub fn ctx(&self, principal: Principal) -> Ctx {
        Ctx { db: self.db.conn().clone(), principal, registry: self.registry.clone() }
    }

    pub fn resource(&self, slug: &str) -> Arc<dyn DynResource> {
        self.registry.get(slug).expect("registered resource").clone()
    }

    /// Creates a category as admin and returns its key.
    pub async fn seed_category(&self, name: &str) -> String {
        let input = json!({"name": name, "slug": name.to_lowercase().replace(' ', "-")});
        self.resource("categories").create(&self.ctx(admin()), input.as_object().unwrap().clone()).await.unwrap().key
    }
}
