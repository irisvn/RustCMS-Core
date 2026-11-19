//! Categories: a plain resource with a has-many relation (its posts, edited
//! in its form unless the category is a link) and a custom widget
//! (`demo/color`).

use rustcms::admin::*;
use serde_json::json;

use crate::models::category;

pub struct CategoryResource;

impl Resource for CategoryResource {
    type Entity = category::Entity;
    const SLUG: &'static str = "categories";

    // #region meta
    fn meta() -> ResourceMeta {
        ResourceMeta::new("Category").plural("Categories").title("name").icon("folder").group("Content").unique(
            "categories_slug_key",
            &["slug"],
            "has already been taken",
        )
    }
    // #endregion meta

    fn fields() -> Vec<Field> {
        vec![
            Field::column(category::Column::Name).required().searchable().sortable().max_length(120),
            Field::column(category::Column::Slug).required().searchable().max_length(120),
            // #region color-field
            Field::column(category::Column::Color).max_length(7),
            // #endregion color-field
            Field::column(category::Column::Layout).required(),
            Field::column(category::Column::LinkUrl).max_length(255),
        ]
    }

    // #region form
    fn form() -> Form {
        // An editor layout: the whole width, a main column and an aside.
        Form::new().width(FormWidth::Full).schema(components![
            Split::make()
                .main(components![
                    // #region reacts
                    // A form reaction: the slug follows the name while it is empty or
                    // still the previous name's slug (until the user edits it).
                    TextInput::make("name").reacts(|_ctx, state| {
                        let before = state.previous().and_then(|v| v.as_str()).map(slugify).unwrap_or_default();
                        let slug = state.text("slug");
                        let follows = slug.is_empty() || slug == before;
                        let next = slugify(state.text("name"));
                        Box::pin(async move { Ok(if follows { vec![FormPatch::set("slug", next)] } else { vec![] }) })
                    }),
                    // #endregion reacts
                    TextInput::make("slug").prefix("/categories/"),
                    SelectInput::make("layout").options(vec![("posts", "Its posts"), ("link", "A link")]),
                    TextInput::make("link_url").label("Link").shown_when(when("layout").eq("link")),
                    // #region posts-repeater
                    // The category's posts, edited in its form: each row is a post
                    // (the posts resource's rules, policy and hooks apply), one
                    // table row each. A category made a link has none: saving
                    // that change removes the posts the user sees.
                    Repeater::make("posts")
                        .relationship()
                        .shown_when(when("layout").eq("posts"))
                        .table()
                        .schema(components![TextInput::make("title"), Toggle::make("published")])
                        .item_label("{title}")
                        .add_action_label("Add a post"),
                    // #endregion posts-repeater
                ])
                .aside(components![Section::make("Display").compact().collapsible(false).schema(components![
                    // #region color-input
                    // Edited by the demo's extension widget (resources/admin/js/demo/color.js).
                    CustomInput::make("color", "demo/color").props(json!({"palette": [
                        {"value": "#dc2626", "label": "Red"},
                        {"value": "#16a34a", "label": "Green"},
                        {"value": "#2563eb", "label": "Blue"},
                        {"value": "#9333ea", "label": "Purple"},
                    ]})),
                    // #endregion color-input
                ])])
        ])
    }
    // #endregion form

    // #region table
    /// The list: the category's fields and how many posts each has (those the
    /// user may list).
    fn table() -> Table {
        Table::new().columns(vec![
            TextColumn::make("name"),
            TextColumn::make("slug"),
            TextColumn::make("color"),
            TextColumn::make("posts_count").label("Posts").counts("posts"),
        ])
    }
    // #endregion table

    // #region infolist
    /// The view page: the slug with its prefix, the number of posts (computed),
    /// and the color drawn by the demo's `demo/swatch` entry.
    fn infolist() -> Infolist {
        Infolist::new().width(FormWidth::Full).schema(components![
            Split::make()
                .main(components![
                    TextEntry::make("name").copyable(),
                    TextEntry::make("slug").prefix("/categories/"),
                    ComputedEntry::make("posts_count").label("Posts").number(0),
                    TextEntry::make("layout").options(vec![("posts", "Its posts"), ("link", "A link")]),
                    TextEntry::make("link_url").label("Link").url().shown_when(when("layout").eq("link")),
                    // #region posts-entry
                    // Its posts under it, as the relation's section lists them.
                    RepeatableEntry::make("posts")
                        .label("Its posts")
                        .relationship()
                        .limit(10)
                        .schema(components![TextEntry::make("title"), BooleanEntry::make("published")])
                        .shown_when(when("layout").eq("posts")),
                    // #endregion posts-entry
                ])
                .aside(components![
                    Section::make("Display").compact().schema(components![CustomEntry::make("color", "demo/swatch"),])
                ])
        ])
    }

    /// Counted through the posts resource: its scope and policy apply, so a
    /// user only counts the posts they may list (a failure shows "not available").
    fn computed() -> Vec<Computed<category::Entity>> {
        vec![Computed::make("posts_count", |ctx, category: &category::Model| {
            Box::pin(async move {
                let query = ListQuery {
                    parent: Some(("category_id".to_owned(), category.id.to_string())),
                    per_page: Some(1),
                    ..Default::default()
                };
                Ok(json!(ctx.records("posts", &query).await?.meta.total))
            })
        })]
    }
    // #endregion infolist

    // #region relations
    /// The category's posts, listed (and created) on its page.
    fn relations() -> Vec<Relation> {
        vec![
            Relation::has_many("posts", "posts", "category_id").label("Posts"),
            // A filtered relation: only the drafts; "New" there creates one.
            Relation::has_many("drafts", "posts", "category_id").where_eq("published", false).label("Drafts"),
        ]
    }
    // #endregion relations
}

// #region slugify
/// `Hello, World!` → `hello-world`: ASCII letters and digits, the rest a dash.
fn slugify(text: &str) -> String {
    let mut slug = String::new();
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    slug.trim_end_matches('-').chars().take(120).collect()
}
// #endregion slugify
