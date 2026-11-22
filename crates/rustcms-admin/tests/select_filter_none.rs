//! `SelectFilter::none(label)` on an application resource: the `~none` token in
//! an `in` list matches records without a value (with the other values chosen,
//! any of them), and declarations that cannot hold refuse to boot. All databases.

mod support;

use std::sync::Arc;

use rustcms_admin::*;
use rustcms_core::Principal;
use rustcms_testing::{TestDb, db_test};
use sea_orm::{ActiveModelTrait, ActiveValue::Set, Schema};
use serde_json::json;
use support::PrincipalExt;

pub mod note {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "filter_notes")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i32,
        pub title: String,
        pub kind: Option<String>,
        pub status: String,
        pub owner_id: Option<i32>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

fn fields() -> Vec<Field> {
    use note::Column as C;
    vec![
        Field::column(C::Title).required(),
        Field::column(C::Kind).filterable(),
        Field::column(C::Status).filterable(),
        Field::column(C::OwnerId).filterable().belongs_to("filter_notes"),
    ]
}

macro_rules! resource {
    ($name:ident, $filters:expr) => {
        pub struct $name;

        impl Resource for $name {
            type Entity = note::Entity;
            const SLUG: &'static str = "filter_notes";

            fn meta() -> ResourceMeta {
                ResourceMeta::new("Note").title("title")
            }

            fn fields() -> Vec<Field> {
                fields()
            }

            fn table() -> Table {
                Table::new().columns(vec![TextColumn::make("title")]).filters($filters)
            }
        }
    };
}

resource!(Notes, vec![SelectFilter::make("kind").options([("bug", "Bug"), ("idea", "Idea")]).none("No kind")]);

db_test!(none_matches_records_without_a_value, |db: TestDb| async move {
    let statement = Schema::new(db.conn().get_database_backend()).create_table_from_entity(note::Entity);
    rustcms_testing::create_table(db.conn(), &statement).await;
    for (title, kind) in [("a bug", Some("bug")), ("an idea", Some("idea")), ("loose", None)] {
        note::ActiveModel {
            title: Set(title.into()),
            kind: Set(kind.map(Into::into)),
            status: Set("open".into()),
            ..Default::default()
        }
        .insert(db.conn())
        .await
        .unwrap();
    }
    let registry: Arc<Registry> = Registry::new().register::<Notes>().finish();
    let notes = registry.get("filter_notes").unwrap();
    let principal = Principal { id: "1".into(), guard: "admin".into(), ..Default::default() };
    let ctx =
        Ctx::new(db.conn().clone(), principal.with(&["filter_notes.view_any", "filter_notes.view"]), registry.clone());
    let titles = async |value: &str| {
        let query = ListQuery {
            filters: vec![Filter { field: "kind".into(), op: FilterOp::In, value: json!(value) }],
            from_query_string: true,
            ..Default::default()
        };
        let page = notes.list(&ctx, &query).await.unwrap();
        let mut titles: Vec<String> = page.data.iter().map(|e| e.data["title"].as_str().unwrap().to_owned()).collect();
        titles.sort();
        titles
    };
    assert_eq!(titles("~none").await, ["loose"]);
    assert_eq!(titles("bug,~none").await, ["a bug", "loose"]);
    assert_eq!(titles("idea").await, ["an idea"]);
});

resource!(OnARequiredColumn, vec![SelectFilter::make("status").options([("open", "Open")]).none("None")]);
resource!(OnATernary, vec![TernaryFilter::make("kind").none("None")]);
resource!(OnARelation, vec![SelectFilter::make("owner_id").none("None")]);

#[test]
#[should_panic(
    expected = "filter `status` has a `none` option but is not a select filter with options on a nullable column"
)]
fn none_needs_a_nullable_column() {
    Registry::new().register::<OnARequiredColumn>();
}

#[test]
#[should_panic(expected = "filter `kind` has a `none` option but is not a select filter")]
fn none_needs_a_select_filter() {
    Registry::new().register::<OnATernary>();
}

#[test]
#[should_panic(expected = "filter `owner_id` has a `none` option")]
fn none_is_refused_on_a_belongs_to_filter() {
    Registry::new().register::<OnARelation>();
}
