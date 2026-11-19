//! The image viewer's switches (vocabulary 22): `preview` belongs to media
//! entries and file uploads only; `MediaEntry::gallery` (deprecated) still
//! builds. The schemas' `preview: false` is in the contract fixtures
//! (`kitchen-sink.schema.*`: the file upload and the view page made from it).

use rustcms_admin::*;

pub mod album {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "albums")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i32,
        pub title: String,
        pub photo: Option<Uuid>,
        pub links: Option<Json>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

fn album_fields() -> Vec<Field> {
    vec![
        Field::column(album::Column::Title).required(),
        Field::column(album::Column::Photo).media(),
        Field::column(album::Column::Links),
    ]
}

/// An albums resource with `form` and `infolist`.
macro_rules! album {
    ($resource:ident, $form:expr, $infolist:expr) => {
        pub struct $resource;

        impl Resource for $resource {
            type Entity = album::Entity;
            const SLUG: &'static str = "albums";

            fn meta() -> ResourceMeta {
                ResourceMeta::new("Album")
            }

            fn fields() -> Vec<Field> {
                album_fields()
            }

            fn form() -> Form {
                Form::new().schema($form)
            }

            fn infolist() -> Infolist {
                Infolist::new().schema($infolist)
            }
        }
    };
}

album!(
    Switches,
    components![TextInput::make("title"), FileUpload::make("photo").preview(false)],
    components![TextEntry::make("title"), MediaEntry::make("photo").preview(false)]
);

#[allow(deprecated)]
fn gallery() -> ui::Entry {
    MediaEntry::make("photo").gallery()
}

album!(Gallery, components![FileUpload::make("photo")], components![gallery()]);

#[test]
fn media_entries_and_file_uploads_take_preview() {
    registry::Registry::new().register::<Switches>().finish();
    registry::Registry::new().register::<Gallery>().finish();
}

album!(OnText, components![TextInput::make("title")], components![TextEntry::make("title").preview(false)]);

#[test]
#[should_panic(expected = "entry `title`: `preview` does not fit a")]
fn other_entries_refuse_preview() {
    registry::Registry::new().register::<OnText>();
}

album!(OnInput, components![TextInput::make("title").preview(false)], components![TextEntry::make("title")]);

#[test]
#[should_panic(expected = "input `title`: only file uploads take `preview`")]
fn other_inputs_refuse_preview() {
    registry::Registry::new().register::<OnInput>();
}

album!(
    InRow,
    components![Repeater::make("links").schema(components![TextInput::make("url").preview(false)])],
    components![TextEntry::make("title")]
);

#[test]
#[should_panic(expected = "input `url`: only file uploads take `preview`")]
fn inputs_in_rows_refuse_preview() {
    registry::Registry::new().register::<InRow>();
}
