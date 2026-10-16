//! News articles: rich text, a cover image, a category, tags, and a
//! publication time (empty: a draft; in the future: scheduled). The slug is
//! made from the title when left empty; a taken slug is refused.

use rustcms::AppResult;
use rustcms::admin::*;
use sea_orm::DatabaseTransaction;
use serde_json::json;

use crate::models::{news, news_tag};

pub struct NewsResource;

/// `title` as a slug: lowercase ASCII words joined by dashes (Vietnamese
/// letters lose their marks, `đ` becomes `d`).
pub fn slugify(title: &str) -> String {
    const GROUPS: &[(&str, char)] = &[
        ("àáạảãâầấậẩẫăằắặẳẵ", 'a'),
        ("èéẹẻẽêềếệểễ", 'e'),
        ("ìíịỉĩ", 'i'),
        ("òóọỏõôồốộổỗơờớợởỡ", 'o'),
        ("ùúụủũưừứựửữ", 'u'),
        ("ỳýỵỷỹ", 'y'),
        ("đ", 'd'),
    ];
    let mut out = String::new();
    for c in title.to_lowercase().chars() {
        let c = GROUPS.iter().find(|(set, _)| set.contains(c)).map_or(c, |(_, plain)| *plain);
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let slug = out.trim_end_matches('-').chars().take(200).collect::<String>();
    if slug.is_empty() { "news".to_owned() } else { slug }
}

#[async_trait::async_trait]
impl Resource for NewsResource {
    type Entity = news::Entity;
    const SLUG: &'static str = "news";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("News article").plural("News").title("title").icon("newspaper").group("News").unique(
            "news_slug_key",
            &["slug"],
            "is already taken",
        )
    }

    fn fields() -> Vec<Field> {
        vec![
            Field::column(news::Column::Title).required().searchable().sortable().max_length(200),
            Field::column(news::Column::Slug).searchable().max_length(200),
            Field::column(news::Column::Body).rich_text(),
            Field::column(news::Column::Cover).media().accept(&["image/*"]),
            Field::column(news::Column::CategoryId).belongs_to("categories").filterable(),
            Field::column(news::Column::AuthorId).guarded(),
            Field::column(news::Column::PublishedAt).sortable().filterable().label("Published at"),
            Field::column(news::Column::CreatedAt).guarded().sortable(),
        ]
    }

    fn form() -> Form {
        Form::new().columns(2).schema(components![
            TextInput::make("title").column_span(2),
            TextInput::make("slug").help("Left empty: made from the title"),
            SelectInput::make("category_id"),
            DateTimePicker::make("published_at").help("Empty: a draft. In the future: scheduled."),
            FileUpload::make("cover"),
            RichEditor::make("body").column_span(2),
        ])
    }

    fn table() -> Table {
        Table::new()
            .columns(vec![TextColumn::make("title"), TextColumn::make("category_id"), DateColumn::make("published_at")])
            .filters(vec![SelectFilter::make("category_id"), DateRangeFilter::make("published_at")])
            .default_sort("-published_at")
    }

    fn relations() -> Vec<Relation> {
        vec![Relation::belongs_to_many("tags", "tags", news_tag::Column::NewsId, news_tag::Column::TagId).label("Tags")]
    }

    async fn mutate_before_create(ctx: &Ctx, _txn: &DatabaseTransaction, mut data: Data) -> AppResult<Data> {
        data.insert("author_id".into(), json!(ctx.principal.id));
        data.insert(
            "created_at".into(),
            json!(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true)),
        );
        let empty = data.get("slug").is_none_or(|s| s.as_str().is_none_or(|s| s.trim().is_empty()));
        if empty {
            let title = data.get("title").and_then(|t| t.as_str()).unwrap_or_default().to_owned();
            data.insert("slug".into(), json!(slugify(&title)));
        }
        Ok(data)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn slugs_lose_marks_and_punctuation() {
        assert_eq!(super::slugify("Khai giảng khoá học Rust — đợt 2!"), "khai-giang-khoa-hoc-rust-dot-2");
        assert_eq!(super::slugify("  Hello, World  "), "hello-world");
        assert_eq!(super::slugify("???"), "news");
    }
}
