//! Tags of news articles (linked from an article's page).

use rustcms::admin::*;

use crate::models::tag;

pub struct TagResource;

impl Resource for TagResource {
    type Entity = tag::Entity;
    const SLUG: &'static str = "tags";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Tag").title("name").icon("tag").group("News").unique(
            "tags_name_key",
            &["name"],
            "is already taken",
        )
    }

    fn fields() -> Vec<Field> {
        vec![Field::column(tag::Column::Name).required().searchable().sortable().max_length(60)]
    }

    fn table() -> Table {
        Table::new().columns(vec![TextColumn::make("name")]).default_sort("name")
    }
}
