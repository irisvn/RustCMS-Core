//! Categories: a plain resource with a has-many relation (its posts) and a
//! custom widget (`demo/color`).

use rustcms::admin::*;
use serde_json::json;

use crate::models::category;

pub struct CategoryResource;

impl Resource for CategoryResource {
    type Entity = category::Entity;
    const SLUG: &'static str = "categories";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Category").plural("Categories").title("name").icon("folder").group("Content").unique(
            "categories_slug_key",
            &["slug"],
            "has already been taken",
        )
    }

    fn fields() -> Vec<Field> {
        vec![
            Field::column(category::Column::Name).required().searchable().sortable().max_length(120),
            Field::column(category::Column::Slug).required().searchable().max_length(120),
            Field::column(category::Column::Color).max_length(7),
        ]
    }

    fn form() -> Form {
        // An editor layout: the whole width, a main column and an aside.
        Form::new().width(FormWidth::Full).schema(components![
            Split::make()
                .main(components![TextInput::make("name"), TextInput::make("slug").prefix("/categories/"),])
                .aside(components![Section::make("Display").compact().collapsible(false).schema(components![
                    // Edited by the demo's extension widget (resources/admin/js/demo/color.js).
                    CustomInput::make("color", "demo/color").props(json!({"palette": [
                        {"value": "#dc2626", "label": "Red"},
                        {"value": "#16a34a", "label": "Green"},
                        {"value": "#2563eb", "label": "Blue"},
                        {"value": "#9333ea", "label": "Purple"},
                    ]})),
                ])])
        ])
    }

    /// The category's posts, listed (and created) on its page.
    fn relations() -> Vec<Relation> {
        vec![Relation::has_many("posts", "posts", "category_id").label("Posts")]
    }
}
