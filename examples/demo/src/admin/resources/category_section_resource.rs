//! A category's landing page sections and their links: edited in the
//! category's form only (rows of rows), not in the navigation. Each link
//! locks its section, which locks its category (a chain of parent locks): a
//! write of either waits for the category's.

use rustcms::admin::*;

use crate::models::{category_section, section_link};

pub const STYLES: [(&str, &str); 2] = [("compact", "Compact"), ("detailed", "Detailed")];

pub struct CategorySectionResource;

impl Resource for CategorySectionResource {
    type Entity = category_section::Entity;
    const SLUG: &'static str = "category_sections";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Section").title("title").navigation(false).lock_parent("category_id", "categories")
    }

    fn fields() -> Vec<Field> {
        use category_section::Column as C;
        vec![
            Field::column(C::CategoryId).belongs_to("categories").required(),
            Field::column(C::Title).required().max_length(120),
            Field::column(C::Style).required(),
            Field::column(C::Position).guarded().readable(|_| true).sortable(),
        ]
    }

    fn form() -> Form {
        Form::new().schema(components![TextInput::make("title"), SelectInput::make("style").options(STYLES.to_vec())])
    }

    fn relations() -> Vec<Relation> {
        vec![Relation::has_many("links", "section_links", "section_id").label("Links")]
    }
}

pub struct SectionLinkResource;

impl Resource for SectionLinkResource {
    type Entity = section_link::Entity;
    const SLUG: &'static str = "section_links";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Link").title("label").navigation(false).lock_parent("section_id", "category_sections")
    }

    fn fields() -> Vec<Field> {
        use section_link::Column as C;
        vec![
            Field::column(C::SectionId).belongs_to("category_sections").required(),
            Field::column(C::Label).required().max_length(80),
            Field::column(C::Url).required().max_length(255),
            Field::column(C::Note).max_length(255),
            Field::column(C::Position).guarded().readable(|_| true).sortable(),
        ]
    }
}
