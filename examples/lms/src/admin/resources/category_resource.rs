//! Categories of the news.

use rustcms::admin::*;

use crate::models::category;

pub struct CategoryResource;

impl Resource for CategoryResource {
    type Entity = category::Entity;
    const SLUG: &'static str = "categories";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Category").plural("Categories").title("name").icon("folder").group("News").unique(
            "categories_name_key",
            &["name"],
            "is already taken",
        )
    }

    fn fields() -> Vec<Field> {
        vec![Field::column(category::Column::Name).required().searchable().sortable().max_length(80)]
    }

    fn table() -> Table {
        Table::new().columns(vec![TextColumn::make("name")]).default_sort("name")
    }
}
