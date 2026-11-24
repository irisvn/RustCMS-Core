//! Tags, attached to posts from a post's page (a belongs-to-many relation).

use rustcms::admin::*;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};
use serde_json::json;

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
            Field::column(tag::Column::ParentId).label("Part of"),
        ]
    }

    fn form() -> Form {
        Form::new().schema(components![
            TextInput::make("name"),
            SelectInput::make("kind").options(KINDS),
            // #region parent-options
            // Options computed on the server: a series is part of a topic, an
            // event of a series; asked again when the kind changes, checked on save.
            SelectInput::make("parent_id").depends_on(["kind"]).options_fn(|ctx, state| {
                let parent_kind = match state.text("kind") {
                    "series" => Some("topic"),
                    "event" => Some("series"),
                    _ => None,
                };
                Box::pin(async move {
                    let Some(parent_kind) = parent_kind else { return Ok(vec![]) };
                    let tags = tag::Entity::find()
                        .filter(tag::Column::Kind.eq(parent_kind))
                        .order_by_asc(tag::Column::Name)
                        .all(&ctx.conn())
                        .await?;
                    Ok(tags.into_iter().map(|t| SelectOption { value: json!(t.id), label: t.name }).collect())
                })
            }),
            // #endregion parent-options
        ])
    }

    // #region table
    fn table() -> Table {
        // `kind` shows the labels of the form's select ("Series", not `series`).
        Table::new().columns(vec![
            TextColumn::make("name"),
            BadgeColumn::make("kind").color("topic", "info").color("series", "primary").color("event", "warning"),
        ])
    }
    // #endregion table
}
