//! Posts: soft deletes, an owner scope, a policy, bulk actions (one with a
//! form), hooks, a tabbed form with key/value attributes, and tags (a
//! belongs-to-many relation with a pivot column, also chosen in the form).

use std::sync::Arc;

use rustcms::AppResult;
use rustcms::admin::*;
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseTransaction, IntoActiveModel, QueryFilter, Select, Set};
use serde_json::json;

use crate::models::{post, post_tag};
use crate::policies::post_policy::PostPolicy;

pub struct PostResource;

#[async_trait::async_trait]
impl Resource for PostResource {
    type Entity = post::Entity;
    const SLUG: &'static str = "posts";

    // #region intro-post
    // #region meta
    fn meta() -> ResourceMeta {
        ResourceMeta::new("Post").title("title").icon("file-text").group("Content").soft_deletes().exportable()
    }
    // #endregion meta

    // #region fields
    fn fields() -> Vec<Field> {
        vec![
            Field::column(post::Column::Title).required().searchable().sortable().max_length(200),
            Field::column(post::Column::Body),
            Field::column(post::Column::CategoryId).belongs_to("categories").filterable(),
            Field::column(post::Column::AuthorId).guarded().readable(|_| true),
            Field::column(post::Column::Published).filterable().sortable(),
            Field::column(post::Column::PublishedAt).sortable().filterable(),
            Field::column(post::Column::Cover).media().accept(&["image/*"]),
            Field::column(post::Column::Content).rich_text(),
            Field::column(post::Column::Attributes).key_value().max_items(20).max_length(200),
        ]
    }
    // #endregion fields
    // #endregion intro-post

    // #region form
    fn form() -> Form {
        Form::new().schema(components![Tabs::make().schema(components![
            Tab::make("Content").schema(components![
                TextInput::make("title").placeholder("A catchy title"),
                Textarea::make("body"),
                FileUpload::make("cover"),
                RichEditor::make("content"),
            ]),
            Tab::make("Publishing").columns(2).schema(components![
                SelectInput::make("category_id"),
                Toggle::make("published"),
                DateTimePicker::make("published_at"),
                // Linked from the form too, not only on the post's page (ADR 0035).
                BelongsToManySelect::make("tags"),
            ]),
            Tab::make("Attributes").schema(components![
                KeyValue::make("attributes").help("Extra details shown with the post, e.g. reading_time = 5 min"),
            ]),
        ])])
    }
    // #endregion form

    // #region table
    fn table() -> Table {
        Table::new()
            .columns(vec![
                TextColumn::make("title"),
                TextColumn::make("category_id").hide_below(Breakpoint::Md),
                BooleanColumn::make("published"),
                DateColumn::make("published_at"),
            ])
            .filters(vec![
                TernaryFilter::make("published"),
                SelectFilter::make("category_id"),
                DateRangeFilter::make("published_at"),
            ])
            .default_sort("-published_at")
    }
    // #endregion table

    // #region policy-scope
    fn policy() -> Arc<dyn Policy> {
        Arc::new(PostPolicy)
    }

    /// Without `posts.view_any`, editors only see their own posts.
    fn scope(query: Select<post::Entity>, ctx: &Ctx) -> Select<post::Entity> {
        if ctx.can("posts.view_any") {
            query
        } else {
            query.filter(post::Column::AuthorId.eq(ctx.principal.id.parse::<i64>().unwrap_or(-1)))
        }
    }
    // #endregion policy-scope

    fn actions() -> Vec<Action<post::Entity>> {
        vec![
            // #region publish
            Action::<post::Entity>::new("publish", |_ctx, txn, posts, _data| {
                Box::pin(async move {
                    for post in posts {
                        let mut active = post.into_active_model();
                        active.published = Set(true);
                        active.published_at = Set(Some(chrono_now()));
                        active.update(txn).await?;
                    }
                    Ok(())
                })
            })
            .label("Publish")
            .bulk()
            .requires_confirmation("Publish the selected posts?"),
            // #endregion publish
            // #region move
            // Asks for a category (one the user may see) in a dialog.
            Action::<post::Entity>::new("move", |_ctx, txn, posts, data| {
                Box::pin(async move {
                    let category = data["category_id"].as_str().and_then(|k| k.parse::<i32>().ok());
                    for post in posts {
                        let mut active = post.into_active_model();
                        active.category_id = Set(category);
                        active.update(txn).await?;
                    }
                    Ok(())
                })
            })
            .label("Move to category…")
            .bulk()
            .form(vec![ActionField::belongs_to("category_id", "categories").label("Category").required()]),
            // #endregion move
            // #region unpublish
            // Offered on published posts only (decided by the server per post); the
            // row's labelled action there, the others are under ⋯.
            Action::<post::Entity>::new("unpublish", |_ctx, txn, posts, _data| {
                Box::pin(async move {
                    for post in posts {
                        let mut active = post.into_active_model();
                        active.published = Set(false);
                        active.published_at = Set(None);
                        active.update(txn).await?;
                    }
                    Ok(())
                })
            })
            .label("Unpublish")
            .ability("update")
            .visible_when(|_ctx, post| post.published)
            .primary()
            .destructive(),
            // #endregion unpublish
            // #region preview
            // Tells the panel what to do next: a message and a page to open in a new tab.
            Action::<post::Entity>::with_outcome("preview", |_ctx, _txn, posts, _data| {
                Box::pin(async move {
                    let id = posts.first().map(|post| post.id).unwrap_or_default();
                    Ok(ActionOutcome::open(format!("/admin/r/posts/records/{id}"))
                        .with_message("The preview opens in a new tab."))
                })
            })
            .label("Preview")
            .ability("view"),
            // #endregion preview
        ]
    }

    // #region relations
    /// Tags, attached and detached on a post's page; `position` orders them.
    fn relations() -> Vec<Relation> {
        vec![
            Relation::belongs_to_many("tags", "tags", post_tag::Column::PostId, post_tag::Column::TagId)
                .pivot(vec![Field::column(post_tag::Column::Position)])
                .label("Tags"),
        ]
    }
    // #endregion relations

    // #region hooks
    async fn mutate_before_create(ctx: &Ctx, _txn: &DatabaseTransaction, mut data: Data) -> AppResult<Data> {
        data.insert("author_id".into(), json!(ctx.principal.id));
        Ok(data)
    }
    // #endregion hooks
}

fn chrono_now() -> sea_orm::prelude::DateTimeUtc {
    sea_orm::prelude::DateTimeUtc::from(std::time::SystemTime::now())
}
