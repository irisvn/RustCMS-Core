//! Posts: soft deletes, an owner scope, a policy, bulk actions (one with a
//! form), hooks, a tabbed form with key/value attributes, and tags (a
//! belongs-to-many relation with a pivot column).

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

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Post").title("title").icon("file-text").group("Content").soft_deletes()
    }

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
            ]),
            Tab::make("Attributes").schema(components![
                KeyValue::make("attributes").help("Extra details shown with the post, e.g. reading_time = 5 min"),
            ]),
        ])])
    }

    fn table() -> Table {
        Table::new()
            .columns(vec![
                TextColumn::make("title"),
                TextColumn::make("category_id"),
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

    fn actions() -> Vec<Action<post::Entity>> {
        vec![
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
        ]
    }

    /// Tags, attached and detached on a post's page; `position` orders them.
    fn relations() -> Vec<Relation> {
        vec![
            Relation::belongs_to_many("tags", "tags", post_tag::Column::PostId, post_tag::Column::TagId)
                .pivot(vec![Field::column(post_tag::Column::Position)])
                .label("Tags"),
        ]
    }

    async fn mutate_before_create(ctx: &Ctx, _txn: &DatabaseTransaction, mut data: Data) -> AppResult<Data> {
        data.insert("author_id".into(), json!(ctx.principal.id));
        Ok(data)
    }
}

fn chrono_now() -> sea_orm::prelude::DateTimeUtc {
    sea_orm::prelude::DateTimeUtc::from(std::time::SystemTime::now())
}
