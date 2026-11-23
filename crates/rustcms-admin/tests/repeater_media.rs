//! Media, rich text and record references in repeater rows (ADR 0044, part
//! B): file uploads and rich editors of JSON rows checked like fields, the
//! media of relationship rows checked with the parent's (once, last),
//! `belongs_to` fields in relationship rows checked as a form's.
//! SQLite, PostgreSQL, MySQL.

use axum::http::StatusCode;
use rustcms_admin::*;
use rustcms_auth::{ADMIN_GUARD, Actor, AuthProvider, AuthState, RbacService};
use rustcms_core::{AppBuilder, AppResult, Config};
use rustcms_testing::{Factory, TestApp, TestClient, TestDb, TestResponse, UserFactory, db_test};
use sea_orm::{ConnectionTrait, DatabaseTransaction, EntityTrait, QueryOrder, Schema};
use serde_json::{Value, json};

mod support;

pub mod note {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "notes")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i32,
        pub title: String,
        pub links: Option<Json>,
        pub deleted_at: Option<DateTimeUtc>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod album {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "albums")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i32,
        pub title: String,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod person {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "people")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i32,
        pub name: String,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod photo {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "photos")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i32,
        pub album_id: i32,
        pub image: Option<Uuid>,
        pub caption: Option<Json>,
        pub credit_id: Option<i32>,
        pub position: i32,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub struct NoteResource;

impl Resource for NoteResource {
    type Entity = note::Entity;
    const SLUG: &'static str = "notes";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Note").title("title").soft_deletes()
    }

    fn fields() -> Vec<Field> {
        vec![
            Field::column(note::Column::Title).required(),
            Field::column(note::Column::Links),
            Field::column(note::Column::DeletedAt).guarded().readable(|_| true),
        ]
    }

    fn form() -> Form {
        Form::new().schema(components![
            TextInput::make("title"),
            Repeater::make("links").schema(components![
                TextInput::make("label").required(),
                // Still images: the viewer stays closed (rows take `preview` too).
                FileUpload::make("icon").accept(&["image/*"]).max_size(100).preview(false),
                FileUpload::make("files").multiple(2),
                RichEditor::make("body"),
            ]),
        ])
    }
}

pub struct AlbumResource;

impl Resource for AlbumResource {
    type Entity = album::Entity;
    const SLUG: &'static str = "albums";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Album").title("title")
    }

    fn fields() -> Vec<Field> {
        vec![Field::column(album::Column::Title).required()]
    }

    fn form() -> Form {
        Form::new().schema(components![
            TextInput::make("title"),
            Repeater::make("photos").relationship().order_column("position").schema(components![
                FileUpload::make("image"),
                RichEditor::make("caption"),
                SelectInput::make("credit_id"),
            ]),
        ])
    }

    fn relations() -> Vec<Relation> {
        vec![Relation::has_many("photos", "photos", "album_id")]
    }
}

pub struct PersonResource;

impl Resource for PersonResource {
    type Entity = person::Entity;
    const SLUG: &'static str = "people";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Person").plural("People").title("name")
    }

    fn fields() -> Vec<Field> {
        vec![Field::column(person::Column::Name).required()]
    }

    // People named "hidden" are outside everyone's scope.
    fn scope(query: sea_orm::Select<person::Entity>, _ctx: &Ctx) -> sea_orm::Select<person::Entity> {
        sea_orm::QueryFilter::filter(query, sea_orm::ColumnTrait::ne(&person::Column::Name, "hidden"))
    }
}

pub struct PhotoResource;

#[async_trait::async_trait]
impl Resource for PhotoResource {
    type Entity = photo::Entity;
    const SLUG: &'static str = "photos";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Photo").lock_parent("album_id", "albums")
    }

    fn fields() -> Vec<Field> {
        use photo::Column as C;
        vec![
            Field::column(C::AlbumId).required().belongs_to("albums"),
            Field::column(C::Image).media().accept(&["image/*"]),
            Field::column(C::Caption).rich_text(),
            Field::column(C::CreditId).belongs_to("people"),
            Field::column(C::Position).guarded().readable(|_| true),
        ]
    }

    // A hook may add media to a row: checked with the album's (it was a 500).
    async fn mutate_before_create(_ctx: &Ctx, _txn: &DatabaseTransaction, mut data: Data) -> AppResult<Data> {
        if let Some(image) = data.get("caption").and_then(|c| c["content"][0]["attrs"]["media"].as_str()) {
            data.insert("image".into(), json!(image));
        }
        Ok(data)
    }

    // An outside effect, through a job of the write's transaction: it runs
    // before the write's media check, and goes if that check fails.
    async fn after_save(ctx: &Ctx, txn: &DatabaseTransaction, model: &photo::Model) -> AppResult<()> {
        ctx.dispatch(txn, &PhotoSaved { id: model.id }).await?;
        Ok(())
    }
}

/// Queued by a photo's `after_save`.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct PhotoSaved {
    pub id: i32,
}

#[async_trait::async_trait]
impl rustcms_core::queue::Job for PhotoSaved {
    const KIND: &'static str = "test.photo_saved";

    async fn handle(self, _: rustcms_core::queue::JobCtx) -> anyhow::Result<()> {
        Ok(())
    }
}

/// A settings page whose rows take an image and a PDF.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct GallerySettings {
    pub items: Vec<GalleryItem>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GalleryItem {
    #[serde(default)]
    pub image: Option<uuid::Uuid>,
    #[serde(default)]
    pub document: Option<uuid::Uuid>,
}

impl rustcms_core::settings::Settings for GallerySettings {
    const GROUP: &'static str = "gallery";
}

#[async_trait::async_trait]
impl SettingsPage for GallerySettings {
    fn meta() -> SettingsMeta {
        SettingsMeta::new("Gallery")
    }

    fn fields() -> Vec<SettingField> {
        vec![SettingField::repeater(Repeater::make("items").schema(components![
            FileUpload::make("image").accept(&["image/*"]),
            FileUpload::make("document").accept(&["application/pdf"]),
        ]))]
    }
}

fn build(config: Config) -> AppBuilder {
    let root = std::env::temp_dir().join(format!("rustcms-repeater-media-{}", uuid::Uuid::now_v7()));
    let config = config.with("storage.root", root.to_string_lossy()).expect("storage config");
    let mut app = AppBuilder::new(config);
    app.provider(rustcms_core::storage::StorageProvider);
    app.provider(AuthProvider::new());
    app.provider(
        AdminProvider::new()
            .media()
            .resource::<NoteResource>()
            .resource::<AlbumResource>()
            .resource::<PersonResource>()
            .resource::<PhotoResource>()
            .settings::<GallerySettings>(),
    );
    app
}

async fn setup(db: TestDb) -> (TestApp, rustcms_core::models::user::Model) {
    db.migrate::<rustcms_migration::Migrator>().await;
    let schema = Schema::new(db.conn().get_database_backend());
    for table in [
        schema.create_table_from_entity(note::Entity),
        schema.create_table_from_entity(album::Entity),
        schema.create_table_from_entity(person::Entity),
        schema.create_table_from_entity(photo::Entity),
    ] {
        rustcms_testing::create_table(db.conn(), &table).await;
    }
    let app = TestApp::new(build, db).await;
    let auth = app.state().service::<AuthState>().unwrap();
    let db = app.state().db();
    let rbac = RbacService::new(db, &auth.resolver);
    rbac.sync_permissions(&auth.permissions.all()).await.unwrap();
    let admin = UserFactory::new().name("Admin").create(db).await;
    let super_admin = rustcms_auth::entities::role::super_admin_id(db, ADMIN_GUARD).await.unwrap().unwrap();
    rbac.assign_role(Actor::System, admin.id, super_admin).await.unwrap();
    (app, admin)
}

/// A media item of the library: `mime`, `size` bytes, in the trash or not.
async fn stored_media(app: &TestApp, owner: i64, mime: &str, size: i64, trashed: bool) -> String {
    use sea_orm::{ActiveModelTrait, ActiveValue::Set};
    let id = uuid::Uuid::now_v7();
    rustcms_admin::media::entity::ActiveModel {
        id: Set(id),
        disk: Set("local".into()),
        key: Set(format!("media/{id}")),
        name: Set("f".into()),
        mime: Set(mime.into()),
        size: Set(size),
        sha256: Set("0".repeat(64)),
        width: Set(None),
        height: Set(None),
        alt: Set(None),
        collection: Set(None),
        owner_id: Set(owner),
        created_at: Set(chrono::Utc::now()),
        deleted_at: Set(trashed.then(chrono::Utc::now)),
    }
    .insert(app.state().db())
    .await
    .unwrap();
    id.to_string()
}

fn doc(media: &str) -> Value {
    json!({"type": "doc", "version": 1, "content": [{"type": "image", "attrs": {"media": media, "alt": "x"}}]})
}

fn issues(response: TestResponse) -> Value {
    response.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
    response.json()["issues"].clone()
}

async fn create(client: &TestClient<'_>, app: &TestApp, slug: &str, body: Value) -> TestResponse {
    client.post_json(&app.admin_api(&format!("resources/{slug}/records")), &body).await
}

db_test!(json_rows_reference_available_accepted_files, |db: TestDb| async move {
    let (app, admin) = setup(db).await;
    let client = app.acting_as(&admin).await;
    let png = stored_media(&app, admin.id, "image/png", 10, false).await;
    let big = stored_media(&app, admin.id, "image/png", 1000, false).await;
    let pdf = stored_media(&app, admin.id, "application/pdf", 10, false).await;
    let trashed = stored_media(&app, admin.id, "image/png", 10, true).await;
    let note = |row: Value| json!({"title": "N", "links": [row]});
    // Files and documents as fields keep them.
    let row = json!({"label": "a", "icon": png, "files": [pdf, png], "body": doc(&png)});
    let created = create(&client, &app, "notes", note(row.clone())).await;
    created.assert_status(StatusCode::CREATED);
    assert_eq!(created.json()["data"]["data"]["links"], json!([row]));
    // Each refusal at the row's input.
    let refused = |row: Value| {
        let client = &client;
        let app = &app;
        async move { issues(create(client, app, "notes", note(row)).await) }
    };
    // The issue's code, or its message for one without a code of its own (`custom`).
    let code = |issues: &Value, path: &str| {
        let issue = &issues[path][0];
        if issue["code"] == json!("custom") { issue["params"]["message"].clone() } else { issue["code"].clone() }
    };
    let unavailable = json!("refers to a file that is not available");
    let found = refused(json!({"label": "a", "icon": trashed})).await;
    assert_eq!(code(&found, "links.0.icon"), unavailable, "{found}");
    let found = refused(json!({"label": "a", "icon": pdf})).await;
    assert_eq!(code(&found, "links.0.icon"), json!("file_types"), "{found}");
    let found = refused(json!({"label": "a", "icon": big})).await;
    assert!(found["links.0.icon"].is_array(), "{found}");
    let found = refused(json!({"label": "a", "icon": "nope"})).await;
    assert_eq!(code(&found, "links.0.icon"), json!("invalid"), "{found}");
    let found = refused(json!({"label": "a", "files": [png, png]})).await;
    assert_eq!(code(&found, "links.0.files"), json!("duplicate_file"), "{found}");
    let found = refused(json!({"label": "a", "files": [png, pdf, big]})).await;
    assert_eq!(code(&found, "links.0.files"), json!("max_items"), "{found}");
    let found = refused(json!({"label": "a", "body": doc(&trashed)})).await;
    assert_eq!(code(&found, "links.0.body"), unavailable, "{found}");
    let found = refused(json!({"label": "a", "body": {"type": "doc", "content": "x"}})).await;
    assert!(found["links.0.body"].is_array(), "{found}");
});

db_test!(relationship_rows_take_media_rich_text_and_references, |db: TestDb| async move {
    let (app, admin) = setup(db).await;
    let client = app.acting_as(&admin).await;
    let png = stored_media(&app, admin.id, "image/png", 10, false).await;
    let trashed = stored_media(&app, admin.id, "image/png", 10, true).await;
    app.state().db().execute_unprepared("INSERT INTO people (name) VALUES ('Ann'), ('hidden')").await.unwrap();
    let album = |rows: Value| json!({"title": "A", "photos": {"create": rows}});
    let row = |reference: &str, data: Value| json!({"ref": reference, "data": data});
    let created = create(
        &client,
        &app,
        "albums",
        album(json!([row("n1", json!({"image": png, "credit_id": 1})), row("n2", json!({"caption": doc(&png)}))])),
    )
    .await;
    created.assert_status(StatusCode::CREATED);
    // The second row's hook took the caption's image: checked with the album's, not refused.
    let photos = photo::Entity::find().order_by_asc(photo::Column::Position).all(app.state().db()).await.unwrap();
    assert_eq!(photos.len(), 2);
    assert_eq!(photos[1].image.map(|i| i.to_string()), Some(png.clone()));
    assert_eq!(photos[0].credit_id, Some(1));
    // Refusals at the row's input, nothing saved.
    let found = issues(create(&client, &app, "albums", album(json!([row("n1", json!({"image": trashed}))]))).await);
    assert_eq!(
        found["photos.n1.image"][0]["params"]["message"],
        json!("refers to a file that is not available"),
        "{found}"
    );
    let found =
        issues(create(&client, &app, "albums", album(json!([row("n1", json!({"caption": doc(&trashed)}))]))).await);
    assert!(found["photos.n1.caption"].is_array() || found["photos.n1.image"].is_array(), "{found}");
    let found = issues(create(&client, &app, "albums", album(json!([row("n1", json!({"credit_id": 2}))]))).await);
    assert!(found["photos.n1.credit_id"].is_array(), "{found}");
    assert_eq!(photo::Entity::find().all(app.state().db()).await.unwrap().len(), 2);
});

db_test!(row_schemas_carry_the_inputs_rules_and_a_compact_editor, |db: TestDb| async move {
    let (app, admin) = setup(db).await;
    let client = app.acting_as(&admin).await;
    let notes = client.get(&app.admin_api("resources/notes/schema?operation=create")).await.json();
    if app.state().db().get_database_backend() == sea_orm::DatabaseBackend::Sqlite {
        // Shared with the SPA tests: JSON rows with files and a document.
        support::contract("repeater-media.schema.create.json", &notes);
    }
    let rows = notes["form"]["components"][1]["repeater"]["components"].clone();
    assert_eq!(
        rows[1]["media"],
        json!({"multiple": false, "maxItems": 1, "accept": ["image/*"], "maxSize": 100, "preview": false})
    );
    assert_eq!(rows[2]["media"]["maxItems"], json!(2));
    assert_eq!(rows[3]["toolbar"], json!("compact"));
    let albums = client.get(&app.admin_api("resources/albums/schema?operation=create")).await.json();
    let rows = albums["form"]["components"][1]["repeater"]["components"].clone();
    assert_eq!(rows[1]["toolbar"], json!("compact"));
    assert_eq!(rows[2]["relationship"], json!({"resource": "people"}));
});

macro_rules! refused_at_boot {
    ($test:ident, $resource:ident, $message:literal, $form:expr) => {
        pub struct $resource;

        impl Resource for $resource {
            type Entity = note::Entity;
            const SLUG: &'static str = "notes";

            fn meta() -> ResourceMeta {
                ResourceMeta::new("Note")
            }

            fn fields() -> Vec<Field> {
                vec![Field::column(note::Column::Title).required(), Field::column(note::Column::Links)]
            }

            fn form() -> Form {
                Form::new().schema($form)
            }
        }

        #[test]
        #[should_panic(expected = $message)]
        fn $test() {
            Registry::new().register::<$resource>().finish();
        }
    };
}

refused_at_boot!(
    row_rules_on_a_fields_input,
    FieldRules,
    "are for inputs of repeater rows",
    components![TextInput::make("title").accept(&["image/*"])]
);
refused_at_boot!(
    file_rules_on_text,
    TextRules,
    "`accept`, `max_size` and `multiple` are for file uploads",
    components![Repeater::make("links").schema(components![TextInput::make("label").accept(&["image/*"])])]
);
refused_at_boot!(
    an_undeclared_node,
    UnknownNode,
    "accepts the node `app.quiz`, which is not declared",
    components![Repeater::make("links").schema(components![RichEditor::make("body").nodes(["app.quiz"])])]
);
refused_at_boot!(
    an_upload_in_a_table_row,
    TableUpload,
    "table rows hold no nested repeater, file upload or rich editor",
    components![Repeater::make("links").table().schema(components![FileUpload::make("icon")])]
);

db_test!(a_restore_leaves_the_files_of_json_rows_alone, |db: TestDb| async move {
    let (app, admin) = setup(db).await;
    let client = app.acting_as(&admin).await;
    let png = stored_media(&app, admin.id, "image/png", 10, false).await;
    let created = create(&client, &app, "notes", json!({"title": "N", "links": [{"label": "a", "icon": png}]})).await;
    created.assert_status(StatusCode::CREATED);
    let key = created.json()["data"]["key"].as_str().unwrap().to_owned();
    client.fetch_csrf().await;
    let url = app.admin_api(&format!("resources/notes/records/{key}"));
    client.send(axum::http::Method::DELETE, &url, None).await.assert_status(StatusCode::NO_CONTENT);
    // The row's file went to the trash meanwhile: the record comes back, the
    // file shows as missing (one column may hold rows of several forms).
    {
        use sea_orm::{ActiveModelTrait, ActiveValue::Set, IntoActiveModel};
        let id = uuid::Uuid::parse_str(&png).unwrap();
        let file = rustcms_admin::media::entity::Entity::find_by_id(id).one(app.state().db()).await.unwrap().unwrap();
        let mut file = file.into_active_model();
        file.deleted_at = Set(Some(chrono::Utc::now()));
        file.update(app.state().db()).await.unwrap();
    }
    let restored = client.post_json(&format!("{url}/restore"), &json!({})).await;
    restored.assert_status(StatusCode::OK);
    assert_eq!(restored.json()["data"]["data"]["links"][0]["icon"], json!(png));
});

db_test!(a_settings_row_file_moved_to_another_input_meets_its_rules, |db: TestDb| async move {
    let (app, admin) = setup(db).await;
    let client = app.acting_as(&admin).await;
    let png = stored_media(&app, admin.id, "image/png", 10, false).await;
    let url = app.admin_api("settings/gallery");
    let version = client.get(&url).await.json()["version"].as_str().unwrap().to_owned();
    client.fetch_csrf().await;
    let first = json!({"version": version, "data": {"items": [{"image": png}]}});
    client.send(axum::http::Method::PUT, &url, Some(&first)).await.assert_status(StatusCode::OK);
    // The same file, kept where it was and also put in the PDF input: checked there.
    let version = client.get(&url).await.json()["version"].as_str().unwrap().to_owned();
    let moved = json!({"version": version, "data": {"items": [{"image": png}, {"document": png}]}});
    let refused = client.send(axum::http::Method::PUT, &url, Some(&moved)).await;
    assert!(issues(refused)["items.1.document"].is_array());
});

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct LogoSettings {
    pub logo: Option<uuid::Uuid>,
}

impl rustcms_core::settings::Settings for LogoSettings {
    const GROUP: &'static str = "logo";
}

#[async_trait::async_trait]
impl SettingsPage for LogoSettings {
    fn meta() -> SettingsMeta {
        SettingsMeta::new("Logo")
    }

    fn fields() -> Vec<SettingField> {
        vec![SettingField::media("logo")]
    }

    // Rules of rows on a setting's own input: the SettingField decides.
    fn form() -> Form {
        Form::new().schema(components![FileUpload::make("logo").accept(&["image/png"])])
    }
}

#[test]
#[should_panic(expected = "are for inputs of repeater rows (declare them on the SettingField)")]
fn row_rules_on_a_settings_input() {
    let _ = AdminProvider::new().settings::<LogoSettings>();
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct NotesSettings {
    pub notes: Vec<serde_json::Value>,
}

impl rustcms_core::settings::Settings for NotesSettings {
    const GROUP: &'static str = "notes";
}

#[async_trait::async_trait]
impl SettingsPage for NotesSettings {
    fn meta() -> SettingsMeta {
        SettingsMeta::new("Notes")
    }

    fn fields() -> Vec<SettingField> {
        vec![SettingField::repeater(Repeater::make("notes").schema(components![RichEditor::make("body")]))]
    }
}

fn notes_settings(config: Config) -> AppBuilder {
    let mut app = AppBuilder::new(config);
    app.provider(AuthProvider::new());
    app.provider(AdminProvider::new().settings::<NotesSettings>());
    app
}

db_test!(rich_editors_in_settings_rows_need_the_media_library, |db: TestDb| async move {
    let built = std::panic::AssertUnwindSafe(TestApp::new(notes_settings, db));
    let panicked = futures_util::FutureExt::catch_unwind(built).await;
    let message = panicked.err().and_then(|p| p.downcast_ref::<String>().cloned()).unwrap_or_default();
    assert!(message.contains("needs the media library"), "{message}");
});

/// `n` media items of the library (images, 10 bytes), inserted at once.
async fn many_media(app: &TestApp, owner: i64, n: usize) -> Vec<String> {
    use sea_orm::ActiveValue::Set;
    let ids: Vec<uuid::Uuid> = (0..n).map(|_| uuid::Uuid::now_v7()).collect();
    for chunk in ids.chunks(200) {
        let rows = chunk.iter().map(|id| rustcms_admin::media::entity::ActiveModel {
            id: Set(*id),
            disk: Set("local".into()),
            key: Set(format!("media/{id}")),
            name: Set("f".into()),
            mime: Set("image/png".into()),
            size: Set(10),
            sha256: Set("0".repeat(64)),
            width: Set(None),
            height: Set(None),
            alt: Set(None),
            collection: Set(None),
            owner_id: Set(owner),
            created_at: Set(chrono::Utc::now()),
            deleted_at: Set(None),
        });
        rustcms_admin::media::entity::Entity::insert_many(rows).exec(app.state().db()).await.unwrap();
    }
    ids.iter().map(uuid::Uuid::to_string).collect()
}

db_test!(a_write_checks_its_media_in_batches_and_at_most_2000, |db: TestDb| async move {
    let (app, admin) = setup(db).await;
    let client = app.acting_as(&admin).await;
    // 600 distinct files, two per row: more than one batch of 500.
    let files = many_media(&app, admin.id, 600).await;
    let rows = |files: &[String]| -> Value {
        files.chunks(2).enumerate().map(|(i, pair)| json!({"label": format!("r{i}"), "files": pair})).collect()
    };
    let created = create(&client, &app, "notes", json!({"title": "N", "links": rows(&files)})).await;
    created.assert_status(StatusCode::CREATED);
    // A file of the second batch unavailable: refused at its row's path.
    let mut missing = files.clone();
    missing[599] = uuid::Uuid::now_v7().to_string();
    let found = issues(create(&client, &app, "notes", json!({"title": "N", "links": rows(&missing)})).await);
    assert_eq!(found["links.299.files"][0]["params"]["message"], json!("refers to a file that is not available"));
    assert_eq!(found.as_object().unwrap().len(), 1, "{found}");
    // 2,001 distinct ids: refused before any lookup.
    let ids: Vec<String> = (0..2001).map(|_| uuid::Uuid::now_v7().to_string()).collect();
    let found = issues(create(&client, &app, "notes", json!({"title": "N", "links": rows(&ids)})).await);
    assert_eq!(found["links"][0]["code"], json!("too_many_media"), "{found}");
    // Settings saves share the check.
    let url = app.admin_api("settings/gallery");
    let version = client.get(&url).await.json()["version"].as_str().unwrap().to_owned();
    let items: Vec<Value> = ids.chunks(2).map(|p| json!({"image": p[0], "document": p.get(1)})).collect();
    client.fetch_csrf().await;
    let refused =
        client.send(axum::http::Method::PUT, &url, Some(&json!({"version": version, "data": {"items": items}}))).await;
    assert_eq!(issues(refused)["items"][0]["code"], json!("too_many_media"));
});

db_test!(a_failed_media_check_takes_back_the_rows_jobs_and_audit, |db: TestDb| async move {
    use sea_orm::PaginatorTrait;
    let (app, admin) = setup(db).await;
    let client = app.acting_as(&admin).await;
    let png = stored_media(&app, admin.id, "image/png", 10, false).await;
    let trashed = stored_media(&app, admin.id, "image/png", 10, true).await;
    let conn = app.state().db();
    let audit = rustcms_core::models::audit_log::Entity::find().count(conn).await.unwrap();
    // Both rows' `after_save` queue a job; the media check runs after them and fails.
    let album = json!({"title": "A", "photos": {"create": [
        {"ref": "n1", "data": {"image": png}},
        {"ref": "n2", "data": {"image": trashed}},
    ]}});
    let found = issues(create(&client, &app, "albums", album).await);
    assert!(found["photos.n2.image"].is_array(), "{found}");
    assert_eq!(rustcms_core::models::job::Entity::find().count(conn).await.unwrap(), 0);
    assert!(photo::Entity::find().all(conn).await.unwrap().is_empty());
    assert!(album::Entity::find().all(conn).await.unwrap().is_empty());
    assert_eq!(rustcms_core::models::audit_log::Entity::find().count(conn).await.unwrap(), audit);
    // Without the failure, the jobs are committed with the rows.
    let album = json!({"title": "A", "photos": {"create": [{"ref": "n1", "data": {"image": png}}]}});
    create(&client, &app, "albums", album).await.assert_status(StatusCode::CREATED);
    assert_eq!(rustcms_core::models::job::Entity::find().count(conn).await.unwrap(), 1);
});
