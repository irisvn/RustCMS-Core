//! Tags, attached to posts from a post's page (a belongs-to-many relation).

use rustcms::admin::*;

use crate::models::tag;

pub struct TagResource;

// #region kinds
/// What a tag groups: the form's choices, which the tags list shows by label.
pub const KINDS: [(&str, &str); 3] = [("topic", "Topic"), ("series", "Series"), ("event", "Event")];
// #endregion kinds

impl Resource for TagResource {
    type Entity = tag::Entity;
    const SLUG: &'static str = "tags";

    // #region meta
    fn meta() -> ResourceMeta {
        // Tags can be imported from CSV; a known name is updated, not added twice.
        ResourceMeta::new("Tag")
            .title("name")
            .icon("tag")
            .group("Content")
            .unique("tags_name_key", &["name"], "has already been taken")
            .import_key("name")
    }
    // #endregion meta

    fn fields() -> Vec<Field> {
        vec![
            Field::column(tag::Column::Name).required().searchable().sortable().max_length(60),
            Field::column(tag::Column::Kind),
        ]
    }

    fn form() -> Form {
        Form::new().schema(components![TextInput::make("name"), SelectInput::make("kind").options(KINDS)])
    }

    // #region table
    fn table() -> Table {
        // `kind` shows the labels of the form's select ("Series", not `series`).
        Table::new().columns(vec![
            TextColumn::make("name"),
            BadgeColumn::make("kind").color("series", "primary").color("event", "warning"),
        ])
    }
    // #endregion table
}
