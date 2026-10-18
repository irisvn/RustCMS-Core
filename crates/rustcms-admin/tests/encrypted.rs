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
