//! Posts: soft deletes, an owner scope, a policy, a bulk action and hooks.

use std::sync::Arc;

use rustcms::AppResult;
use rustcms::admin::*;
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseTransaction, IntoActiveModel, QueryFilter, Select, Set};
use serde_json::json;

use crate::models::post;
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
        ]
    }

    fn form() -> Form {
        Form::new().columns(2).schema(components![
            Section::make("Content").schema(components![
                TextInput::make("title").placeholder("A catchy title"),
                Textarea::make("body"),
                FileUpload::make("cover"),
                RichEditor::make("content"),
            ]),
            Section::make("Publishing").schema(components![
                SelectInput::make("category_id"),
                Toggle::make("published"),
                DateTimePicker::make("published_at"),
            ]),
        ])
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
