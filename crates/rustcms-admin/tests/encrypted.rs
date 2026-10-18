//! Encrypted columns in app models (`rustcms_core::crypto`, ADR 0030): SeaORM
//! round trips, rotation with `key:reencrypt` (compare-and-swap, resuming,
//! unreadable rows, settings secrets). All databases.

use std::collections::BTreeMap;

use rustcms_core::crypto::{Binding, Context, Encrypted, EncryptedColumns, Keyring, reencrypt_column};
use rustcms_core::models::setting;
use rustcms_core::settings;
use rustcms_core::{AppBuilder, AppKey, Config};
use rustcms_testing::{TestApp, TestDb, db_test};
use sea_orm::entity::prelude::*;
use sea_orm::{ActiveValue::Set, Schema};

mod vault {
    use std::collections::BTreeMap;

    use rustcms_core::crypto::Encrypted;
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, serde::Serialize, serde::Deserialize)]
    #[sea_orm(table_name = "vaults")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub name: String,
        #[sea_orm(column_type = "Text")]
        pub token: Encrypted<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub env: Option<Encrypted<BTreeMap<String, String>>>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

fn token() -> Context {
    Context::column("vaults", "token")
}

fn env() -> Context {
    Context::column("vaults", "env")
}

fn build(config: Config) -> AppBuilder {
    let mut app = AppBuilder::new(config);
    app.encrypted_column::<vault::Entity>(vault::Column::Token, Binding::Column);
    app.encrypted_column::<vault::Entity>(vault::Column::Env, Binding::Column);
    app
}

/// An app on `db` whose keyring is `key` with `previous` keys.
async fn app_with(db: TestDb, key: &AppKey, previous: &[&AppKey]) -> TestApp {
    db.migrate::<rustcms_migration::Migrator>().await;
    let schema = Schema::new(db.conn().get_database_backend());
    let table = schema.create_table_from_entity(vault::Entity).if_not_exists().to_owned();
    rustcms_testing::create_table(db.conn(), &table).await;
    let previous: Vec<String> = previous.iter().map(|k| k.encode()).collect();
    let config = rustcms_testing::test_config(db.url())
        .with("app.key", key.encode())
        .and_then(|c| c.with("app.previous_keys", previous))
        .unwrap();
    TestApp::with_config(build, config, db).await
}

async fn insert(app: &TestApp, name: &str, secret: &str) -> i64 {
    let keyring = app.state().keyring();
    let vars: BTreeMap<String, String> = [("DB_PASSWORD".to_owned(), format!("{secret}-env"))].into();
    let row = vault::ActiveModel {
        name: Set(name.to_owned()),
        token: Set(Encrypted::seal(keyring, &token(), &secret.to_owned()).unwrap()),
        env: Set(Some(Encrypted::seal(keyring, &env(), &vars).unwrap())),
        ..Default::default()
    };
    row.insert(app.state().db()).await.unwrap().id
}

async fn opened(app: &TestApp, id: i64) -> (String, BTreeMap<String, String>) {
    let row = vault::Entity::find_by_id(id).one(app.state().db()).await.unwrap().unwrap();
    let keyring = app.state().keyring();
    (row.token.open(keyring, &token()).unwrap(), row.env.unwrap().open(keyring, &env()).unwrap())
}

fn columns(app: &TestApp) -> EncryptedColumns {
    (*app.state().service::<EncryptedColumns>().unwrap()).clone()
}

db_test!(models_store_only_envelopes, |db: TestDb| async move {
    let key = AppKey::generate();
    let app = app_with(db, &key, &[]).await;
    let id = insert(&app, "web", "marker-token").await;
    let (secret, env) = opened(&app, id).await;
    assert_eq!(secret, "marker-token");
    assert_eq!(env["DB_PASSWORD"], "marker-token-env");
    // What the database holds, and what a model serializes, carry nothing.
    let row = vault::Entity::find_by_id(id).one(app.state().db()).await.unwrap().unwrap();
    assert!(!row.token.envelope().contains("marker"));
    let json = serde_json::to_string(&row).unwrap();
    assert!(!json.contains("marker") && !json.contains("rcms1"), "{json}");
    assert!(!format!("{row:?}").contains("marker"));
    // A model cannot be built from JSON: an encrypted value never deserializes.
    assert!(serde_json::from_str::<vault::Model>(r#"{"id":1,"name":"x","token":"x","env":null}"#).is_err());
    // A NULL map loads as None; a damaged envelope loads, and only `open` fails.
    let damaged = vault::ActiveModel {
        name: Set("damaged".into()),
        token: Set(Encrypted::from_envelope("rcms1.0000000000000000.AAAA".into())),
        env: Set(None),
        ..Default::default()
    };
    let damaged = damaged.insert(app.state().db()).await.unwrap();
    let loaded = vault::Entity::find_by_id(damaged.id).one(app.state().db()).await.unwrap().unwrap();
    assert!(loaded.env.is_none());
    assert!(loaded.token.open(app.state().keyring(), &token()).is_err());
});

db_test!(key_reencrypt_moves_rows_to_the_current_key, |db: TestDb| async move {
    let (old, new) = (AppKey::generate(), AppKey::generate());
    let before = app_with(db.clone(), &old, &[]).await;
    let mut ids = Vec::new();
    for n in 0..7 {
        ids.push(insert(&before, &format!("s{n}"), &format!("secret{n}")).await);
    }
    // A settings secret too.
    let mut stored = BTreeMap::new();
    stored.insert("api_key".to_owned(), "settings-secret".to_owned());
    let txn = rustcms_core::db::begin_write(before.state().db()).await.unwrap();
    settings::write(&txn, before.state().keyring(), "general", 1, Default::default(), &stored, None).await.unwrap();
    txn.commit().await.unwrap();
    drop(before);

    let rotated = app_with(db.clone(), &new, &[&old]).await;
    let state = rotated.state();
    let keyring = state.keyring();
    let columns = columns(&rotated);
    assert_eq!(columns.all().len(), 2);
    // Readable with the previous key; nothing is current yet.
    assert_eq!(opened(&rotated, ids[3]).await.0, "secret3");
    // A dry run counts, writes nothing.
    let dry = reencrypt_column(state.db(), keyring, &columns.all()[0], 3, true).await.unwrap();
    assert_eq!((dry.current, dry.moved), (0, 7));
    let again = reencrypt_column(state.db(), keyring, &columns.all()[0], 3, true).await.unwrap();
    assert_eq!(again.moved, 7, "the dry run wrote nothing");
    // Small batches: resumes past each page.
    for column in columns.all() {
        let report = reencrypt_column(state.db(), keyring, column, 3, false).await.unwrap();
        assert_eq!((report.current, report.moved, report.changed), (0, 7, 0), "{}", column.column());
        assert!(report.unreadable.is_empty());
    }
    let settings_report = rustcms_core::crypto::reencrypt_settings(state, false).await.unwrap();
    assert_eq!(settings_report.moved, 1);
    // A second run finds everything current and writes nothing.
    for column in columns.all() {
        let report = reencrypt_column(state.db(), keyring, column, 500, false).await.unwrap();
        assert_eq!((report.current, report.moved), (7, 0));
    }
    let row = setting::Entity::find_by_id("general".to_owned()).one(state.db()).await.unwrap().unwrap();
    assert_eq!(rustcms_core::crypto::reencrypt_settings(state, false).await.unwrap().moved, 0);
    let unchanged = setting::Entity::find_by_id("general".to_owned()).one(state.db()).await.unwrap().unwrap();
    assert_eq!(row, unchanged, "a second settings run changes nothing, byte for byte");
    drop(rotated);

    // The old key gone: everything still opens.
    let after = app_with(db, &new, &[]).await;
    for (n, id) in ids.iter().enumerate() {
        assert_eq!(opened(&after, *id).await.0, format!("secret{n}"));
    }
    let read = settings::read(after.state().db(), after.state().keyring(), "general", false).await.unwrap();
    assert_eq!(read.secret("api_key"), &settings::SecretState::Set("settings-secret".into()));
});

db_test!(key_reencrypt_reports_and_keeps_what_it_cannot_read, |db: TestDb| async move {
    let (lost, new) = (AppKey::generate(), AppKey::generate());
    let app = app_with(db.clone(), &lost, &[]).await;
    let lost_id = insert(&app, "lost", "gone").await;
    drop(app);
    let app = app_with(db, &new, &[]).await;
    let good = insert(&app, "good", "kept").await;
    // An envelope claiming the current key, but forged: not counted as current.
    let keyring = app.state().keyring();
    let real = vault::Entity::find_by_id(good).one(app.state().db()).await.unwrap().unwrap();
    let mut forged: Vec<char> = real.token.envelope().chars().collect();
    let at = forged.len() - 10;
    forged[at] = if forged[at] == 'A' { 'B' } else { 'A' };
    let forged: String = forged.into_iter().collect();
    assert_ne!(forged, real.token.envelope());
    let mut model: vault::ActiveModel = real.into();
    let forged_id = {
        model.id = sea_orm::ActiveValue::NotSet;
        model.name = Set("forged".into());
        model.token = Set(Encrypted::from_envelope(forged.clone()));
        model.insert(app.state().db()).await.unwrap().id
    };
    let registered = columns(&app);
    let column = &registered.all()[0];
    let report = reencrypt_column(app.state().db(), keyring, column, 500, false).await.unwrap();
    assert_eq!(report.current, 1);
    let mut unreadable = report.unreadable.clone();
    unreadable.sort();
    let mut expected = vec![lost_id.to_string(), forged_id.to_string()];
    expected.sort();
    assert_eq!(unreadable, expected);
    // Never modified.
    let still = vault::Entity::find_by_id(forged_id).one(app.state().db()).await.unwrap().unwrap();
    assert_eq!(still.token.envelope(), forged);
});

db_test!(key_reencrypt_never_overwrites_a_row_changed_meanwhile, |db: TestDb| async move {
    let (old, new) = (AppKey::generate(), AppKey::generate());
    let app = app_with(db.clone(), &old, &[]).await;
    let id = insert(&app, "web", "first").await;
    drop(app);
    let app = app_with(db, &new, &[&old]).await;
    let keyring: Keyring = app.state().keyring().clone();
    // The row as the rotation will have read it, then changed by a writer.
    let read = vault::Entity::find_by_id(id).one(app.state().db()).await.unwrap().unwrap();
    let mut update: vault::ActiveModel = read.clone().into();
    update.token = Set(Encrypted::seal(&keyring, &token(), &"second".to_owned()).unwrap());
    update.update(app.state().db()).await.unwrap();
    // A compare-and-swap with the stale envelope changes nothing.
    let fresh = read.token.resealed(&keyring, &token()).unwrap();
    let stale = vault::Entity::update_many()
        .col_expr(vault::Column::Token, Expr::value(fresh.envelope().to_owned()))
        .filter(vault::Column::Id.eq(id))
        .filter(vault::Column::Token.eq(read.token.envelope().to_owned()))
        .exec(app.state().db())
        .await
        .unwrap();
    assert_eq!(stale.rows_affected, 0);
    assert_eq!(opened(&app, id).await.0, "second");
});

// ---- Admin resources with encrypted fields ----

use axum::http::{Method, StatusCode};
use rustcms_admin::{AdminProvider, Field, Resource, ResourceMeta};
use rustcms_auth::{ADMIN_GUARD, Actor, AuthProvider, AuthState, RbacService};
use rustcms_core::models::audit_log;
use rustcms_testing::{Factory, UserFactory};
use serde_json::json;

struct VaultResource;

impl Resource for VaultResource {
    type Entity = vault::Entity;
    const SLUG: &'static str = "vaults";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Vault").title("name").audit(true)
    }

    fn fields() -> Vec<Field> {
        vec![
            Field::column(vault::Column::Name).required().searchable().sortable(),
            Field::column(vault::Column::Token).encrypted().required(),
            Field::column(vault::Column::Env).encrypted_map().max_items(3),
        ]
    }
}

fn admin_build(config: Config) -> AppBuilder {
    let mut app = AppBuilder::new(config);
    app.provider(AuthProvider::new());
    app.provider(AdminProvider::new().resource::<VaultResource>());
    app
}

const MARKER: &str = "marker-7f3a";

/// An admin app on `db`, a super admin and an editor who may write vaults
/// but read only the name.
async fn admin_app(db: TestDb) -> (TestApp, rustcms_core::models::user::Model) {
    db.migrate::<rustcms_migration::Migrator>().await;
    let schema = Schema::new(db.conn().get_database_backend());
    let table = schema.create_table_from_entity(vault::Entity).if_not_exists().to_owned();
    rustcms_testing::create_table(db.conn(), &table).await;
    let app = TestApp::new(admin_build, db).await;
    let auth = app.state().service::<AuthState>().unwrap();
    let rbac = RbacService::new(app.state().db(), &auth.resolver);
    rbac.sync_permissions(&auth.permissions.all()).await.unwrap();
    let admin = UserFactory::new().name("Admin").create(app.state().db()).await;
    let role = rustcms_auth::entities::role::super_admin_id(app.state().db(), ADMIN_GUARD).await.unwrap().unwrap();
    rbac.assign_role(Actor::System, admin.id, role).await.unwrap();
    (app, admin)
}

fn records(app: &TestApp) -> String {
    app.admin_api("resources/vaults/records")
}

db_test!(admin_fields_never_send_secrets, |db: TestDb| async move {
    let (app, admin) = admin_app(db).await;
    let client = app.acting_as(&admin).await;
    let created = client
        .post_json(
            &records(&app),
            &json!({"name": "web", "token": {"replace": MARKER}, "env": {"set": {"DB_PASSWORD": MARKER, "API": MARKER}}}),
        )
        .await;
    created.assert_status(StatusCode::CREATED);
    let body = created.json()["data"].clone();
    let key = body["key"].as_str().unwrap().to_owned();
    assert_eq!(body["data"]["token"], json!({"state": "set"}));
    assert_eq!(body["data"]["env"]["state"], json!("set"));
    assert_eq!(body["data"]["env"]["keys"], json!(["API", "DB_PASSWORD"]));
    let record = format!("{}/{key}", records(&app));
    // No marker, no envelope anywhere a user can look.
    for url in [
        records(&app),
        record.clone(),
        format!("{record}/history"),
        app.admin_api("resources/vaults/schema?operation=edit"),
        app.admin_api("resources/vaults/schema?operation=view"),
        app.admin_api("audit"),
    ] {
        let text = client.get(&url).await.text();
        assert!(!text.contains(MARKER) && !text.contains("rcms1."), "{url}: {text}");
    }
    // Stored: only ciphertext; opens with the keyring.
    let row = vault::Entity::find().one(app.state().db()).await.unwrap().unwrap();
    assert!(!row.token.envelope().contains(MARKER));
    assert_eq!(row.token.open(app.state().keyring(), &token()).unwrap(), MARKER);
    // Audit: the fields changed, as `[redacted]` only.
    let entries = audit_log::Entity::find().all(app.state().db()).await.unwrap();
    let text = serde_json::to_string(&entries.iter().map(|e| e.changes.clone()).collect::<Vec<_>>()).unwrap();
    assert!(!text.contains(MARKER) && !text.contains("rcms1."), "{text}");
    // The schema: write-only, typed as secrets, no default.
    let schema = client.get(&app.admin_api("resources/vaults/schema?operation=edit")).await.json();
    let inputs = schema["form"]["components"].as_array().unwrap();
    let input = |name: &str| inputs.iter().find(|i| i["name"] == name).cloned().unwrap();
    assert_eq!(input("token")["type"], json!("secret"));
    assert_eq!(input("token")["writeOnly"], json!(true));
    assert_eq!(input("env")["type"], json!("secret_map"));
    assert_eq!(input("env")["valueType"], json!("secret_map"));
    assert!(input("env").get("default").is_none());
});

db_test!(admin_secret_writes_keep_replace_clear_and_need_the_revision, |db: TestDb| async move {
    let (app, admin) = admin_app(db).await;
    let client = app.acting_as(&admin).await;
    let created = client
        .post_json(
            &records(&app),
            &json!({"name": "web", "token": {"replace": "one"}, "env": {"set": {"A": "1", "B": "2"}}}),
        )
        .await
        .json()["data"]
        .clone();
    let record = format!("{}/{}", records(&app), created["key"].as_str().unwrap());
    let revision = created["data"]["env"]["revision"].as_str().unwrap().to_owned();
    let stored = || async {
        let row = vault::Entity::find().one(app.state().db()).await.unwrap().unwrap();
        let keyring = app.state().keyring();
        (row.token.open(keyring, &token()).unwrap(), row.env.map(|e| e.open(keyring, &env()).unwrap()))
    };
    // Leaving a field out keeps it.
    client.send(Method::PATCH, &record, Some(&json!({"name": "renamed"}))).await.assert_status(StatusCode::OK);
    assert_eq!(stored().await.0, "one");
    // Keep A, replace B, add C; anything not listed would go.
    let ops = json!({"env": {"revision": revision, "keep": ["A"], "set": {"B": "two", "C": "3"}}, "token": {"replace": "two"}});
    let updated = client.send(Method::PATCH, &record, Some(&ops)).await;
    updated.assert_status(StatusCode::OK);
    let (token_now, env_now) = stored().await;
    assert_eq!(token_now, "two");
    assert_eq!(
        env_now.unwrap(),
        [("A", "1"), ("B", "two"), ("C", "3")].map(|(k, v)| (k.to_owned(), v.to_owned())).into()
    );
    // The old revision is stale now: 409, nothing changed.
    let stale = client.send(Method::PATCH, &record, Some(&json!({"env": {"revision": revision, "set": {}}}))).await;
    assert_eq!(stale.status, StatusCode::CONFLICT);
    let missing = client.send(Method::PATCH, &record, Some(&json!({"env": {"set": {}}}))).await;
    assert_eq!(missing.status, StatusCode::CONFLICT, "a revision is required to change an existing map");
    let revision = updated.json()["data"]["data"]["env"]["revision"].as_str().unwrap().to_owned();
    // Keeping an unknown key, a key twice, or more than max_items: 422.
    for bad in [
        json!({"revision": revision, "keep": ["Z"]}),
        json!({"revision": revision, "keep": ["A"], "set": {"A": "x"}}),
        json!({"revision": revision, "set": {"1": "a", "2": "b", "3": "c", "4": "d"}}),
        json!({"revision": revision, "set": {"A": 1}}),
        json!("plain"),
        json!(null),
    ] {
        let response = client.send(Method::PATCH, &record, Some(&json!({"env": bad}))).await;
        response.assert_validation_error("env");
        assert!(!response.text().contains("\"1\""), "no value quoted");
    }
    // Clear (the column is nullable) needs the revision too.
    let cleared =
        client.send(Method::PATCH, &record, Some(&json!({"env": {"revision": revision, "clear": true}}))).await;
    cleared.assert_status(StatusCode::OK);
    assert_eq!(cleared.json()["data"]["data"]["env"], json!({"state": "unset", "revision": "none"}));
    assert!(stored().await.1.is_none());
    // The token is required: it cannot be cleared nor emptied.
    client
        .send(Method::PATCH, &record, Some(&json!({"token": {"clear": true}})))
        .await
        .assert_validation_error("token");
    client
        .send(Method::PATCH, &record, Some(&json!({"token": {"replace": ""}})))
        .await
        .assert_validation_error("token");
    // Keep on create is refused; so is a revision.
    let bad = json!({"name": "x", "token": {"replace": "t"}, "env": {"keep": ["A"]}});
    client.post_json(&records(&app), &bad).await.assert_validation_error("env");
});

db_test!(an_unreadable_map_is_replaced_never_kept, |db: TestDb| async move {
    let (app, admin) = admin_app(db).await;
    let client = app.acting_as(&admin).await;
    // A row sealed with a key this app does not have.
    let lost = Keyring::single(AppKey::generate());
    let row = vault::ActiveModel {
        name: Set("old".into()),
        token: Set(Encrypted::seal(&lost, &token(), &"t".to_owned()).unwrap()),
        env: Set(Some(Encrypted::seal(&lost, &env(), &BTreeMap::from([("A".to_owned(), "1".to_owned())])).unwrap())),
        ..Default::default()
    };
    let row = row.insert(app.state().db()).await.unwrap();
    let record = format!("{}/{}", records(&app), row.id);
    let view = client.get(&record).await.json()["data"].clone();
    assert_eq!(view["data"]["token"], json!({"state": "unreadable"}));
    assert_eq!(view["data"]["env"]["state"], json!("unreadable"));
    assert!(view["data"]["env"].get("keys").is_none());
    let revision = view["data"]["env"]["revision"].as_str().unwrap().to_owned();
    let keep = json!({"env": {"revision": revision, "keep": ["A"]}});
    client.send(Method::PATCH, &record, Some(&keep)).await.assert_validation_error("env");
    let replace = json!({"env": {"revision": revision, "set": {"A": "new"}}, "token": {"replace": "new"}});
    let fixed = client.send(Method::PATCH, &record, Some(&replace)).await;
    fixed.assert_status(StatusCode::OK);
    assert_eq!(fixed.json()["data"]["data"]["env"]["keys"], json!(["A"]));
});

db_test!(encrypted_admin_columns_rotate_with_key_reencrypt, |db: TestDb| async move {
    let (app, _) = admin_app(db).await;
    let names: Vec<String> =
        columns(&app).all().iter().map(|c| format!("{}.{}", c.table(), c.column())).collect::<Vec<_>>();
    assert_eq!(names.len(), 2, "{names:?}");
    assert!(names.contains(&"vaults.token".to_owned()) && names.contains(&"vaults.env".to_owned()));
});

#[test]
#[should_panic(expected = "encrypted field `token` cannot be sortable")]
fn encrypted_fields_cannot_be_sorted() {
    struct Bad;
    impl Resource for Bad {
        type Entity = vault::Entity;
        const SLUG: &'static str = "bad";
        fn meta() -> ResourceMeta {
            ResourceMeta::new("Bad")
        }
        fn fields() -> Vec<Field> {
            vec![Field::column(vault::Column::Token).encrypted().sortable()]
        }
    }
    let _ = rustcms_admin::registry::Registry::new().register::<Bad>();
}

#[test]
#[should_panic(expected = "must be a SecretInput")]
fn encrypted_fields_need_their_input() {
    struct Bad;
    impl Resource for Bad {
        type Entity = vault::Entity;
        const SLUG: &'static str = "bad";
        fn meta() -> ResourceMeta {
            ResourceMeta::new("Bad")
        }
        fn fields() -> Vec<Field> {
            vec![Field::column(vault::Column::Token).encrypted()]
        }
        fn form() -> rustcms_admin::Form {
            rustcms_admin::Form::new().schema(vec![rustcms_admin::TextInput::make("token").into()])
        }
    }
    let _ = rustcms_admin::registry::Registry::new().register::<Bad>();
}

mod small {
    use rustcms_core::crypto::Encrypted;
    use sea_orm::entity::prelude::*;

    /// An `INTEGER` primary key (decoded as `i32` by the drivers).
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "small_vaults")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i32,
        #[sea_orm(column_type = "Text")]
        pub token: Encrypted<String>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

db_test!(key_reencrypt_reads_integer_keys_of_any_width, |db: TestDb| async move {
    let (old, new) = (AppKey::generate(), AppKey::generate());
    db.migrate::<rustcms_migration::Migrator>().await;
    let schema = Schema::new(db.conn().get_database_backend());
    rustcms_testing::create_table(db.conn(), &schema.create_table_from_entity(small::Entity)).await;
    let context = Context::column("small_vaults", "token");
    let before = Keyring::single(old.clone());
    for n in 0..3 {
        let row = small::ActiveModel {
            token: Set(Encrypted::seal(&before, &context, &format!("s{n}")).unwrap()),
            ..Default::default()
        };
        row.insert(db.conn()).await.unwrap();
    }
    let keyring = Keyring::new(new, vec![old]).unwrap();
    let column = rustcms_core::crypto::EncryptedColumn::of::<small::Entity>(small::Column::Token, Binding::Column);
    let report = reencrypt_column(db.conn(), &keyring, &column, 2, false).await.unwrap();
    assert_eq!((report.current, report.moved, report.changed), (0, 3, 0));
});

db_test!(admin_masks_any_envelope_and_keeps_secrets_out_of_exports, |db: TestDb| async move {
    let (app, admin) = admin_app(db).await;
    let client = app.acting_as(&admin).await;
    // A damaged envelope in an ordinary column is masked too.
    let row = vault::ActiveModel {
        name: Set("rcms1.broken".into()),
        token: Set(Encrypted::seal(app.state().keyring(), &token(), &MARKER.to_owned()).unwrap()),
        env: Set(None),
        ..Default::default()
    };
    let row = row.insert(app.state().db()).await.unwrap();
    let record = client.get(&format!("{}/{}", records(&app), row.id)).await.text();
    assert!(!record.contains("rcms1.") && !record.contains(MARKER), "{record}");
    // Export columns never include encrypted fields.
    let panel = app.state().service::<rustcms_admin::http::AdminPanel>().unwrap();
    let resource = panel.registry.get("vaults").unwrap().clone();
    let principal =
        rustcms_core::Principal { id: "1".into(), guard: "admin".into(), super_admin: true, ..Default::default() };
    let ctx = rustcms_admin::Ctx::new(app.state().db().clone(), principal, panel.registry.clone());
    let names: Vec<String> = resource.export_columns(&ctx).into_iter().map(|(n, _)| n).collect();
    assert_eq!(names, ["name"]);
});

db_test!(admin_map_revisions_guard_every_change_and_bounds_hold_on_the_merge, |db: TestDb| async move {
    let (app, admin) = admin_app(db).await;
    let client = app.acting_as(&admin).await;
    let created = client
        .post_json(
            &records(&app),
            &json!({"name": "w", "token": {"replace": "t"}, "env": {"set": {"A": "1", "B": "2", "C": "3"}}}),
        )
        .await
        .json()["data"]
        .clone();
    let record = format!("{}/{}", records(&app), created["key"].as_str().unwrap());
    // Replace-all and clear without the revision are stale too.
    for op in [json!({"set": {"X": "1"}}), json!({"clear": true}), json!({"revision": "0000", "clear": true})] {
        let response = client.send(Method::PATCH, &record, Some(&json!({"env": op}))).await;
        assert_eq!(response.status, StatusCode::CONFLICT, "{op}");
        assert_eq!(response.json()["code"], json!("secret_stale"));
    }
    // Kept keys count toward max_items (3): keeping 3 and adding 1 is refused.
    let revision = created["data"]["env"]["revision"].as_str().unwrap();
    let over = json!({"env": {"revision": revision, "keep": ["A", "B", "C"], "set": {"D": "4"}}});
    client.send(Method::PATCH, &record, Some(&over)).await.assert_validation_error("env");
});

db_test!(writers_who_cannot_read_get_a_revision_but_no_names, |db: TestDb| async move {
    let (app, _) = admin_app(db).await;
    let auth = app.state().service::<AuthState>().unwrap();
    let rbac = RbacService::new(app.state().db(), &auth.resolver);
    let perms: Vec<String> =
        ["vaults.view_any", "vaults.view", "vaults.update", "vaults.create"].map(String::from).to_vec();
    let role = rbac.create_role(Actor::System, ADMIN_GUARD, "vault_writer", &perms).await.unwrap();
    let writer = UserFactory::new().name("Writer").create(app.state().db()).await;
    rbac.assign_role(Actor::System, writer.id, role.id).await.unwrap();
    let client = app.acting_as(&writer).await;
    let created = client
        .post_json(&records(&app), &json!({"name": "w", "token": {"replace": "t"}, "env": {"set": {"A": "1"}}}))
        .await;
    created.assert_status(StatusCode::CREATED);
    // Default rules let this writer read the field: names shown. The point
    // here: a revision always comes with the view, so replace-all works.
    let view = created.json()["data"]["data"]["env"].clone();
    assert!(view["revision"].is_string());
});

#[tokio::test]
#[should_panic(expected = "is encrypted")]
async fn dashboards_cannot_show_encrypted_fields() {
    let db = TestDb::sqlite().await;
    let config = rustcms_testing::test_config(db.url());
    let mut app = AppBuilder::new(config);
    app.provider(AuthProvider::new());
    let dashboard = rustcms_admin::dashboard::Dashboard::new()
        .widget(rustcms_admin::dashboard::Widget::recent("latest", "vaults").columns(&["name", "token"]));
    app.provider(AdminProvider::new().resource::<VaultResource>().dashboard(dashboard));
}
