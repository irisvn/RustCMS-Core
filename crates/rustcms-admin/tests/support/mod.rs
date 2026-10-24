//! Test entities, resources and per-test databases.
#![allow(dead_code)]

pub mod app;

use std::sync::Arc;

use rustcms_admin::{
    Action, BooleanColumn, Ctx, Data, DateColumn, DateTimePicker, DynResource, Field, Form, KeyStrategy, NumberInput,
    Policy, Registry, Resource, ResourceMeta, Section, SelectFilter, SelectInput, Table, TernaryFilter, TextColumn,
    TextInput, Toggle, components,
};
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
        // `Text`: the tests store 60 KiB labels (MySQL makes a bare `String` `varchar(255)`).
        #[sea_orm(column_type = "Text")]
        pub label: String,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// Pivot of posts ↔ tags: `position` (default 0), `note` (read with
/// `posts.read_notes`), `secret` never declared.
pub mod post_tag {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "post_tags")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub post_id: Uuid,
        #[sea_orm(primary_key, auto_increment = false)]
        pub tag_slug: String,
        #[sea_orm(default_value = 0)]
        pub position: i32,
        pub note: Option<String>,
        pub secret: Option<String>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {
        #[sea_orm(
            belongs_to = "super::post::Entity",
            from = "Column::PostId",
            to = "super::post::Column::Id",
            on_delete = "Cascade"
        )]
        Post,
        #[sea_orm(
            belongs_to = "super::tag::Entity",
            from = "Column::TagSlug",
            to = "super::tag::Column::Slug",
            on_delete = "Cascade"
        )]
        Tag,
    }

    impl ActiveModelBehavior for ActiveModel {}
}

/// A pivot of tags ↔ posts the other way round (boot checks only; no table).
pub mod tag_post {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "tag_posts")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub tag_slug: String,
        #[sea_orm(primary_key, auto_increment = false)]
        pub post_id: Uuid,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod note {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "notes")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i32,
        #[sea_orm(unique)]
        pub title: String,
        pub category_id: Option<i32>,
        pub deleted_at: Option<DateTimeUtc>,
        pub cover: Option<Uuid>,
        pub attachments: Option<Json>,
        pub body: Option<Json>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// Soft deletes, default permission policy (`notes.*`).
pub struct NoteResource;

/// What `NoteResource`'s trash, restore and force-delete hooks saw:
/// `"{hook}:{title}:{trashed}"`.
#[allow(dead_code)]
pub static NOTE_HOOKS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// Notes whose title starts with `stuck-{hook}` make that hook fail.
fn note_hook(hook: &str, record: &note::Model) -> AppResult<()> {
    NOTE_HOOKS.lock().unwrap().push(format!("{hook}:{}:{}", record.title, record.deleted_at.is_some()));
    if record.title.starts_with(&format!("stuck-{hook}")) { Err(AppError::internal(hook.to_owned())) } else { Ok(()) }
}

#[async_trait::async_trait]
impl Resource for NoteResource {
    type Entity = note::Entity;
    const SLUG: &'static str = "notes";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Note")
            .title("title")
            .soft_deletes()
            .unique("notes_title_key", &["title"], "is taken")
            .import_key("title")
    }

    fn fields() -> Vec<Field> {
        vec![
            Field::column(note::Column::Title).required().searchable(),
            Field::column(note::Column::CategoryId).belongs_to("categories"),
            Field::column(note::Column::DeletedAt).guarded().readable(|_| true),
            Field::column(note::Column::Cover).media().accept(&["image/*"]).max_size(10_000),
            Field::column(note::Column::Attachments).media_list(2),
            Field::column(note::Column::Body).rich_text(),
        ]
    }

    fn actions() -> Vec<Action<note::Entity>> {
        vec![
            Action::<note::Entity>::new("touch", |_, _, _, _| Box::pin(async { Ok(()) })).bulk().ability("update"),
            // Renames the notes, then asks the panel to open `url` (checked before the commit).
            // Also a bulk action: `open` is refused on runs on several records.
            Action::<note::Entity>::with_outcome("open", |_ctx, txn, notes, data| {
                Box::pin(async move {
                    for note in notes {
                        let title = format!("opened {}", note.title);
                        let mut active = note.into_active_model();
                        active.title = Set(title);
                        active.update(txn).await?;
                    }
                    let url = data["url"].as_str().unwrap_or_default().to_owned();
                    Ok(rustcms_admin::ActionOutcome::open(url).with_message("Opening the page"))
                })
            })
            .ability("update")
            .bulk()
            .form(vec![rustcms_admin::ActionField::text("url").required().max_length(3000)]),
        ]
    }

    async fn after_save(_ctx: &Ctx, _txn: &DatabaseTransaction, record: &note::Model) -> AppResult<()> {
        note_hook("save", record)
    }

    async fn after_trash(_ctx: &Ctx, _txn: &DatabaseTransaction, record: &note::Model) -> AppResult<()> {
        note_hook("trash", record)
    }

    /// Overridden: replaces the default call to `after_save`.
    async fn after_restore(_ctx: &Ctx, _txn: &DatabaseTransaction, record: &note::Model) -> AppResult<()> {
        note_hook("restore", record)
    }

    async fn after_force_delete(_ctx: &Ctx, txn: &DatabaseTransaction, record: &note::Model) -> AppResult<()> {
        // Still in the delete's transaction: the row is gone here.
        assert!(note::Entity::find_by_id(record.id).one(txn).await?.is_none(), "after the delete");
        note_hook("force", record)
    }
}

pub struct CategoryResource;

impl Resource for CategoryResource {
    type Entity = category::Entity;
    const SLUG: &'static str = "categories";

    fn relations() -> Vec<rustcms_admin::Relation> {
        vec![rustcms_admin::Relation::has_many("notes", "notes", "category_id").label("Notes")]
    }

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Category").title("name").unique("categories_slug_key", &["slug"], "has already been taken")
    }

    fn fields() -> Vec<Field> {
        vec![
            Field::column(category::Column::Name).required().searchable().sortable(),
            Field::column(category::Column::Slug).required().filterable(),
        ]
    }

    fn policy() -> Arc<dyn Policy> {
        Arc::new(CategoryPolicy)
    }

    fn scope(query: Select<category::Entity>, ctx: &Ctx) -> Select<category::Entity> {
        if ctx.can("categories.view_hidden") { query } else { query.filter(category::Column::Name.not_like("Hidden%")) }
    }
}

/// Default permissions, but `secret-*` categories need `categories.view_secret`.
pub struct CategoryPolicy;

#[async_trait::async_trait]
impl Policy for CategoryPolicy {
    async fn allows(&self, ctx: &Ctx, ability: &str, record: Option<&Data>) -> bool {
        let secret = record.is_some_and(|r| r["slug"].as_str().unwrap_or_default().starts_with("secret-"));
        ctx.can(&format!("categories.{ability}")) && (!secret || ctx.can("categories.view_secret"))
    }
}

pub struct PostPolicy;

#[async_trait::async_trait]
impl Policy for PostPolicy {
    async fn allows(&self, ctx: &Ctx, ability: &str, record: Option<&Data>) -> bool {
        let owns = record.is_some_and(|r| r["author_id"].as_str() == Some(ctx.principal.id.as_str()));
        let granted = ctx.can(&format!("posts.{ability}"))
            || (matches!(ability, "update" | "view") && owns && ctx.can("posts.update_own"));
        match (ability, record) {
            ("publish" | "attach_tags", Some(r)) => {
                granted && !r["title"].as_str().unwrap_or_default().starts_with("locked")
            }
            _ => granted,
        }
    }

    /// Link writes and `move` need their permission whatever the post.
    async fn admits(&self, ctx: &Ctx, ability: &str) -> bool {
        match ability {
            "attach_tags" | "detach_tags" | "update_pivot_tags" | "move" => ctx.can(&format!("posts.{ability}")),
            _ => true,
        }
    }
}

pub struct PostResource;

#[async_trait::async_trait]
impl Resource for PostResource {
    type Entity = post::Entity;
    const SLUG: &'static str = "posts";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Post")
            .title("title")
            .key(KeyStrategy::AppGenerated(|| SqlValue::Uuid(Some(Uuid::now_v7()))))
            .exportable()
    }

    fn fields() -> Vec<Field> {
        vec![
            Field::column(post::Column::Title).required().searchable().sortable().max_length(120),
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

    fn form() -> Form {
        Form::new().columns(2).schema(components![
            Section::make("Content")
                .schema(components![TextInput::make("title").placeholder("Title"), NumberInput::make("price")]),
            SelectInput::make("category_id"),
            Toggle::make("published"),
            DateTimePicker::make("published_at"),
            // The tags, linked from the form (ADR 0035).
            rustcms_admin::BelongsToManySelect::make("tags"),
        ])
    }

    fn table() -> Table {
        Table::new()
            .columns(vec![
                TextColumn::make("title"),
                TextColumn::make("category_id").hide_below(rustcms_admin::Breakpoint::Sm),
                BooleanColumn::make("published"),
                DateColumn::make("published_at").hide_below(rustcms_admin::Breakpoint::Md),
            ])
            .filters(vec![TernaryFilter::make("published"), SelectFilter::make("category_id")])
            .default_sort("-title")
    }

    fn policy() -> Arc<dyn Policy> {
        Arc::new(PostPolicy)
    }

    fn relations() -> Vec<rustcms_admin::Relation> {
        vec![
            rustcms_admin::Relation::belongs_to_many(
                "tags",
                "tags",
                post_tag::Column::PostId,
                post_tag::Column::TagSlug,
            )
            .pivot(vec![
                Field::column(post_tag::Column::Position),
                Field::column(post_tag::Column::Note).readable(|c| c.can("posts.read_notes")).max_length(20),
            ]),
        ]
    }

    fn scope(query: Select<post::Entity>, ctx: &Ctx) -> Select<post::Entity> {
        if ctx.can("posts.manage_all") {
            query
        } else {
            query.filter(post::Column::AuthorId.eq(ctx.principal.id.parse::<i64>().unwrap_or(-1)))
        }
    }

    fn actions() -> Vec<Action<post::Entity>> {
        vec![
            Action::<post::Entity>::new("publish", |_ctx, txn, models, _data| {
                Box::pin(async move {
                    for model in models {
                        let mut active = model.into_active_model();
                        active.published = Set(true);
                        active.update(txn).await?;
                    }
                    Ok(())
                })
            })
            .label("Publish")
            .bulk()
            .requires_confirmation("Publish the selected posts?")
            .requires_permission("posts.publish"),
            // Owner-only: no admission permission, the policy decides per record.
            Action::<post::Entity>::new("touch", |_ctx, txn, posts, _data| {
                Box::pin(async move {
                    for post in posts {
                        let title = format!("{} (touched)", post.title);
                        let mut active = post.into_active_model();
                        active.title = Set(title);
                        active.update(txn).await?;
                    }
                    Ok(())
                })
            })
            .ability("update")
            .primary(),
            // Only on published posts (decided per record by the server).
            Action::<post::Entity>::new("unpublish", |_ctx, txn, posts, _data| {
                Box::pin(async move {
                    for post in posts {
                        assert!(post.published, "never handed a record it is hidden on");
                        let mut active = post.into_active_model();
                        active.published = Set(false);
                        active.update(txn).await?;
                    }
                    Ok(())
                })
            })
            .ability("update")
            .visible_when(|_ctx, post| post.published)
            .destructive(),
            Action::<post::Entity>::new("archive", |_ctx, _txn, _posts, _data| Box::pin(async move { Ok(()) }))
                .ability("update")
                .requires_permission("posts.archive"),
            // A form: the category (visible to the user), how, a weight and a note.
            Action::<post::Entity>::new("move", |_ctx, txn, posts, data| {
                Box::pin(async move {
                    let category: i32 = data["category_id"].as_str().unwrap_or_default().parse().unwrap_or_default();
                    let title = format!("{}|{}|{}", data["mode"], data["weight"], data["note"]);
                    for post in posts {
                        let mut active = post.into_active_model();
                        active.category_id = Set(Some(category));
                        active.title = Set(title.clone());
                        active.update(txn).await?;
                    }
                    Ok(())
                })
            })
            .label("Move to category…")
            .bulk()
            .form(vec![
                rustcms_admin::ActionField::belongs_to("category_id", "categories").label("Category").required(),
                rustcms_admin::ActionField::select("mode", [("move", "Move"), ("copy", "Copy")]).default("move"),
                rustcms_admin::ActionField::integer("weight"),
                rustcms_admin::ActionField::text("note").max_length(10),
            ]),
            // An instant (UTC): the handler gets it canonical and stores it.
            Action::<post::Entity>::new("schedule", |_ctx, txn, posts, data| {
                Box::pin(async move {
                    let at = data["at"].as_str().map(|at| {
                        chrono::DateTime::parse_from_rfc3339(at)
                            .expect("canonical RFC 3339")
                            .with_timezone(&chrono::Utc)
                    });
                    for post in posts {
                        let mut active = post.into_active_model();
                        active.published_at = Set(at);
                        active.update(txn).await?;
                    }
                    Ok(())
                })
            })
            .label("Schedule…")
            .submit_label("Schedule")
            .ability("publish")
            .requires_permission("posts.publish")
            .form(vec![rustcms_admin::ActionField::datetime("at").label("Publish at").required()]),
        ]
    }

    /// Notes each change with the pivot rows it sees; `stuck-links*` posts refuse.
    async fn after_relation_change(
        _ctx: &Ctx,
        txn: &DatabaseTransaction,
        record: &post::Model,
        change: &rustcms_admin::RelationChange,
    ) -> AppResult<()> {
        let mut linked: Vec<String> = post_tag::Entity::find()
            .filter(post_tag::Column::PostId.eq(record.id))
            .all(txn)
            .await?
            .into_iter()
            .map(|r| r.tag_slug)
            .collect();
        linked.sort();
        POST_LINKS.lock().unwrap().push(PostLinks {
            post: record.id.to_string(),
            title: record.title.clone(),
            change: change.clone(),
            linked,
        });
        if record.title.starts_with("stuck-links") { Err(AppError::internal("stuck")) } else { Ok(()) }
    }

    async fn mutate_before_create(ctx: &Ctx, _txn: &DatabaseTransaction, mut data: Data) -> AppResult<Data> {
        data.insert("author_id".into(), json!(ctx.principal.id));
        if data.get("title") == Some(&json!("expand")) {
            data.insert("title".into(), json!("x".repeat(500)));
        }
        Ok(data)
    }
}

/// What `PostResource::after_relation_change` saw: the post, the change and
/// the post's links in the write's transaction.
#[derive(Debug, Clone)]
pub struct PostLinks {
    pub post: String,
    pub title: String,
    pub change: rustcms_admin::RelationChange,
    pub linked: Vec<String>,
}

#[allow(dead_code)]
pub static POST_LINKS: std::sync::Mutex<Vec<PostLinks>> = std::sync::Mutex::new(Vec::new());

/// The changes `PostResource::after_relation_change` saw for `post`.
#[allow(dead_code)]
pub fn post_links(post: &str) -> Vec<PostLinks> {
    POST_LINKS.lock().unwrap().iter().filter(|l| l.post == post).cloned().collect()
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
        vec![
            Field::column(tag::Column::Slug).required().searchable(),
            Field::column(tag::Column::Label).required().searchable(),
        ]
    }

    /// With `tags.joined`, joins `categories`, which also has a `slug` column.
    fn scope(query: Select<tag::Entity>, ctx: &Ctx) -> Select<tag::Entity> {
        // `hidden-*` tags only with `tags.view_hidden`.
        let query =
            if ctx.can("tags.view_hidden") { query } else { query.filter(tag::Column::Slug.not_like("hidden-%")) };
        if !ctx.can("tags.joined") {
            return query;
        }
        let relation: sea_orm::RelationDef =
            tag::Entity::belongs_to(category::Entity).from(tag::Column::Slug).to(category::Column::Slug).into();
        sea_orm::QuerySelect::join(query, sea_orm::JoinType::LeftJoin, relation)
    }

    async fn after_save(_ctx: &Ctx, _txn: &DatabaseTransaction, record: &tag::Model) -> AppResult<()> {
        if record.label == "boom" { Err(AppError::internal("boom")) } else { Ok(()) }
    }

    /// A `locked` tag cannot be deleted (the hook fails); others are noted.
    async fn after_delete(_ctx: &Ctx, txn: &DatabaseTransaction, record: &tag::Model) -> AppResult<()> {
        if record.label == "locked" {
            return Err(AppError::internal("locked"));
        }
        // Still in the delete's transaction: the row is gone here.
        let left = tag::Entity::find_by_id(record.slug.clone()).one(txn).await?;
        assert!(left.is_none(), "after_delete runs after the delete");
        DELETED_TAGS.lock().unwrap().push(record.slug.clone());
        Ok(())
    }
}

/// Tags `TagResource::after_delete` saw (each test runs in its own process).
#[allow(dead_code)]
pub static DELETED_TAGS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

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
        create_tables(db.conn()).await;
        let registry = Registry::new()
            .register::<CategoryResource>()
            .register::<PostResource>()
            .register::<TagResource>()
            .register::<NoteResource>()
            .register::<rustcms_admin::media::MediaResource>()
            .finish();
        Self { db, registry }
    }

    pub fn ctx(&self, principal: Principal) -> Ctx {
        Ctx::new(self.db.conn().clone(), principal, self.registry.clone())
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

/// Creates the test entities' tables.
pub async fn create_tables(conn: &sea_orm::DatabaseConnection) {
    let schema = Schema::new(conn.get_database_backend());
    for statement in [
        schema.create_table_from_entity(category::Entity),
        schema.create_table_from_entity(post::Entity),
        schema.create_table_from_entity(tag::Entity),
        schema.create_table_from_entity(note::Entity),
        schema.create_table_from_entity(post_tag::Entity),
        // Also created by the framework migrations some tests run first.
        schema.create_table_from_entity(rustcms_admin::media::entity::Entity).if_not_exists().to_owned(),
        schema.create_table_from_entity(rustcms_core::models::audit_log::Entity).if_not_exists().to_owned(),
    ] {
        rustcms_testing::create_table(conn, &statement).await;
    }
}

/// Compares `actual` with `fixtures/contract/<name>`, the JSON contract shared
/// with the admin SPA. `UPDATE_CONTRACT=1` rewrites the fixture.
pub fn contract(name: &str, actual: &serde_json::Value) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/contract").join(name);
    let rendered = serde_json::to_string_pretty(actual).unwrap() + "\n";
    if std::env::var_os("UPDATE_CONTRACT").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, rendered).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("missing contract fixture {}; run with UPDATE_CONTRACT=1", path.display()));
    assert_eq!(rendered, expected, "contract {name} changed; review and run with UPDATE_CONTRACT=1");
}

/// Like [`contract`] but never rewrites the fixture (for a second backend).
#[allow(dead_code)]
pub fn contract_matches(name: &str, actual: &serde_json::Value) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/contract").join(name);
    let rendered = serde_json::to_string_pretty(actual).unwrap() + "\n";
    let expected = std::fs::read_to_string(&path).unwrap_or_default();
    assert_eq!(rendered, expected, "contract {name} differs on this backend");
}

/// Id of the role named `name` (as a string, like the API).
#[allow(dead_code)]
pub async fn role_id(app: &rustcms_testing::TestApp, name: &str) -> String {
    use rustcms_auth::entities::role;
    let found = role::Entity::find().filter(role::Column::Name.eq(name)).one(app.state().db()).await.unwrap();
    found.unwrap_or_else(|| panic!("role {name}")).id.to_string()
}
