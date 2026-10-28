//! Tags, attached to posts from a post's page (a belongs-to-many relation).

use rustcms::admin::*;

use crate::models::tag;

pub struct TagResource;

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
        vec![Field::column(tag::Column::Name).required().searchable().sortable().max_length(60)]
    }
}
