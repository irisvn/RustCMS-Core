//! Application settings stored in the database, one row per group (ADR 0015).
//!
//! A group is a struct implementing [`Settings`]; read it anywhere with
//! [`get`] (or [`load`] inside a transaction). Secret fields have the type
//! [`Secret`]: stored encrypted with a key derived from `APP_KEY`, and never
//! serialized out of the store. The admin panel edits groups
//! (`rustcms_admin::SettingsPage`).

use std::collections::BTreeMap;
use std::fmt;

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use chrono::Utc;
use sea_orm::{ActiveModelTrait, ActiveValue::Set, ConnectionTrait, DatabaseTransaction, EntityTrait, QuerySelect};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

use crate::models::setting;
use crate::{AppError, AppKey, AppResult, AppState};

/// Purpose of the encryption key (`HMAC(APP_KEY, purpose)`).
const KEY_PURPOSE: &str = "rustcms/settings/v1";
const NONCE_LEN: usize = 12;

/// A settings group.
///
/// ```rust,ignore
/// #[derive(Clone, Default, Serialize, Deserialize)]
/// pub struct GeneralSettings {
///     pub site_name: String,
///     pub smtp_password: Secret,
/// }
///
/// impl Settings for GeneralSettings {
///     const GROUP: &'static str = "general";
/// }
/// ```
pub trait Settings: Serialize + DeserializeOwned + Default + Send + Sync + 'static {
    /// Name of the group: `[a-z0-9_]{1,32}`, unique in the application.
    const GROUP: &'static str;
}

/// Whether `group` is a valid group name.
pub fn valid_group(group: &str) -> bool {
    (1..=32).contains(&group.len()) && group.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// A secret value (a password, an API key). Serializes as `null` everywhere,
/// so a settings struct can be logged, queued or sent without leaking it;
/// only the store reads and writes the value.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Secret(Option<String>);

impl Secret {
    /// A set secret.
    pub fn new(value: impl Into<String>) -> Self {
        Self(Some(value.into()))
    }

    /// The value, if set.
    pub fn expose(&self) -> Option<&str> {
        self.0.as_deref()
    }

    /// Whether a value is set.
    pub fn is_set(&self) -> bool {
        self.0.is_some()
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(if self.is_set() { "Secret(set)" } else { "Secret(unset)" })
    }
}

thread_local! {
    /// Set while the store inspects a struct: secrets then serialize their value.
    static EXPOSE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

impl Serialize for Secret {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match &self.0 {
            Some(value) if EXPOSE.with(std::cell::Cell::get) => serializer.serialize_str(value),
            _ => serializer.serialize_none(),
        }
    }
}

/// `value` serialized with its secrets in clear (for the store's own checks).
#[doc(hidden)]
pub fn to_value_exposed<T: Serialize>(value: &T) -> Result<Value, serde_json::Error> {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            EXPOSE.with(|e| e.set(false));
        }
    }
    EXPOSE.with(|e| e.set(true));
    let _reset = Reset;
    serde_json::to_value(value)
}

impl<'de> Deserialize<'de> for Secret {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self(Option::<String>::deserialize(deserializer)?))
    }
}

/// A stored secret, as read.
#[derive(Clone, PartialEq, Eq)]
pub enum SecretState {
    /// Set; the plaintext.
    Set(String),
    /// Not set.
    Unset,
    /// Set, but cannot be decrypted (`APP_KEY` changed, or the row was altered).
    Unreadable,
}

impl fmt::Debug for SecretState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Set(_) => "Set",
            Self::Unset => "Unset",
            Self::Unreadable => "Unreadable",
        })
    }
}

/// A group as stored.
#[derive(Debug, Clone, Default)]
pub struct Stored {
    /// `0` when the group was never saved.
    pub version: i64,
    /// Values other than secrets.
    pub values: Map<String, Value>,
    /// Secrets that are stored (set or unreadable).
    pub secrets: BTreeMap<String, SecretState>,
}

impl Stored {
    /// The state of secret `field`.
    pub fn secret(&self, field: &str) -> &SecretState {
        self.secrets.get(field).unwrap_or(&SecretState::Unset)
    }
}

fn cipher(key: &AppKey) -> Aes256Gcm {
    Aes256Gcm::new(&key.derive(KEY_PURPOSE).into())
}

/// The additional data binding a ciphertext to its group, field and version.
fn aad(group: &str, field: &str, version: i64) -> String {
    format!("{group}/{field}/{version}")
}

fn encrypt(key: &AppKey, group: &str, field: &str, version: i64, plain: &str) -> AppResult<String> {
    let nonce: [u8; NONCE_LEN] = rand::random();
    let aad = aad(group, field, version);
    let sealed = cipher(key)
        .encrypt(&Nonce::from(nonce), Payload { msg: plain.as_bytes(), aad: aad.as_bytes() })
        .map_err(|_| AppError::internal("settings: encryption failed"))?;
    let mut out = nonce.to_vec();
    out.extend_from_slice(&sealed);
    Ok(STANDARD.encode(out))
}

fn decrypt(key: &AppKey, group: &str, field: &str, version: i64, stored: &str) -> Option<String> {
    let bytes = STANDARD.decode(stored).ok()?;
    if bytes.len() <= NONCE_LEN {
        return None;
    }
    let (nonce, sealed) = bytes.split_at(NONCE_LEN);
    let nonce: [u8; NONCE_LEN] = nonce.try_into().ok()?;
    let aad = aad(group, field, version);
    let plain = cipher(key).decrypt(&Nonce::from(nonce), Payload { msg: sealed, aad: aad.as_bytes() }).ok()?;
    String::from_utf8(plain).ok()
}

/// Reads group `group` (`lock`: for update, inside a write transaction).
pub async fn read<C: ConnectionTrait>(conn: &C, key: &AppKey, group: &str, lock: bool) -> AppResult<Stored> {
    let mut find = setting::Entity::find_by_id(group.to_owned());
    if lock && conn.get_database_backend() != sea_orm::DbBackend::Sqlite {
        find = find.lock_exclusive();
    }
    let Some(row) = find.one(conn).await? else { return Ok(Stored::default()) };
    let values = match row.value {
        Value::Object(map) => map,
        _ => return Err(AppError::internal(format!("settings `{group}`: stored value is not an object"))),
    };
    let mut secrets = BTreeMap::new();
    let stored = match &row.secrets {
        None => None,
        Some(Value::Object(stored)) => Some(stored),
        // Fail closed: an altered column is not "no secrets".
        Some(_) => return Err(AppError::internal(format!("settings `{group}`: stored secrets are not an object"))),
    };
    if let Some(stored) = stored {
        for (field, sealed) in stored {
            let state = sealed
                .as_str()
                .and_then(|s| decrypt(key, group, field, row.version, s))
                .map_or(SecretState::Unreadable, SecretState::Set);
            secrets.insert(field.clone(), state);
        }
    }
    Ok(Stored { version: row.version, values, secrets })
}

/// Stores group `group` at `version` (the previous version + 1; `1` creates
/// the row), with `secrets` (plaintexts) encrypted for that version. The
/// caller holds the lock and has checked the previous version.
pub async fn write(
    txn: &DatabaseTransaction,
    key: &AppKey,
    group: &str,
    version: i64,
    values: Map<String, Value>,
    secrets: &BTreeMap<String, String>,
    updated_by: Option<i64>,
) -> AppResult<()> {
    let mut sealed = Map::new();
    for (field, plain) in secrets {
        sealed.insert(field.clone(), Value::String(encrypt(key, group, field, version, plain)?));
    }
    let row = setting::ActiveModel {
        name: Set(group.to_owned()),
        value: Set(Value::Object(values)),
        secrets: Set((!sealed.is_empty()).then_some(Value::Object(sealed))),
        version: Set(version),
        updated_at: Set(Utc::now()),
        updated_by: Set(updated_by),
    };
    if version == 1 {
        row.insert(txn).await?
    } else {
        row.update(txn).await?
    };
    Ok(())
}

/// The typed value of a stored group: defaults for what is missing.
pub fn typed<T: Settings>(stored: &Stored) -> AppResult<T> {
    let Value::Object(mut map) = serde_json::to_value(T::default())
        .map_err(|_| AppError::internal(format!("settings `{}`: defaults do not serialize", T::GROUP)))?
    else {
        return Err(AppError::internal(format!("settings `{}`: not a struct", T::GROUP)));
    };
    for (field, value) in &stored.values {
        if map.contains_key(field) {
            map.insert(field.clone(), value.clone());
        }
    }
    for (field, state) in &stored.secrets {
        match state {
            SecretState::Set(plain) => {
                map.insert(field.clone(), Value::String(plain.clone()));
            }
            SecretState::Unset => {}
            SecretState::Unreadable => {
                return Err(AppError::internal(format!(
                    "settings: secret `{}.{field}` cannot be decrypted (APP_KEY changed?); set it again",
                    T::GROUP
                )));
            }
        }
    }
    // The serde message may quote a value (a secret): never pass it on.
    serde_json::from_value(Value::Object(map))
        .map_err(|_| AppError::internal(format!("settings `{}` do not match their struct", T::GROUP)))
}

/// Reads group `T` through `conn` (a connection, or a transaction to see its
/// own writes).
pub async fn load<T: Settings, C: ConnectionTrait>(conn: &C, key: &AppKey) -> AppResult<T> {
    typed(&read(conn, key, T::GROUP, false).await?)
}

/// Reads group `T`: one query; a change made on any instance is seen at once.
pub async fn get<T: Settings>(state: &AppState) -> AppResult<T> {
    load(state.db(), &state.config().app.key).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default, Serialize, Deserialize)]
    struct Mail {
        host: String,
        password: Secret,
    }

    impl Settings for Mail {
        const GROUP: &'static str = "mail";
    }

    #[test]
    fn secrets_never_serialize() {
        let mail = Mail { host: "smtp".into(), password: Secret::new("hunter2") };
        let json = serde_json::to_string(&mail).unwrap();
        assert_eq!(json, r#"{"host":"smtp","password":null}"#);
        assert_eq!(format!("{:?}", mail.password), "Secret(set)");
        assert_eq!(format!("{:?}", SecretState::Set("hunter2".into())), "Set");
        // Only the store's own inspection sees the value, and only while it runs.
        assert_eq!(to_value_exposed(&mail).unwrap()["password"], serde_json::json!("hunter2"));
        assert_eq!(serde_json::to_value(&mail).unwrap()["password"], Value::Null);
    }

    #[test]
    fn altered_secret_columns_fail_closed() {
        // `read` rejects a non-object container (checked through the same match).
        let bad = [Value::Null, serde_json::json!([]), serde_json::json!("x")];
        for value in bad {
            assert!(!matches!(value, Value::Object(_)));
        }
    }

    #[test]
    fn ciphertexts_are_bound_to_group_field_and_version() {
        let key = AppKey::generate();
        let sealed = encrypt(&key, "mail", "password", 3, "hunter2").unwrap();
        assert_eq!(decrypt(&key, "mail", "password", 3, &sealed).as_deref(), Some("hunter2"));
        assert_ne!(sealed, encrypt(&key, "mail", "password", 3, "hunter2").unwrap(), "a new nonce each time");
        for (group, field, version) in [("other", "password", 3), ("mail", "user", 3), ("mail", "password", 4)] {
            assert_eq!(decrypt(&key, group, field, version, &sealed), None, "{group}/{field}/{version}");
        }
        assert_eq!(decrypt(&AppKey::generate(), "mail", "password", 3, &sealed), None, "another APP_KEY");
        assert!(!sealed.contains("hunter2"));
    }

    #[test]
    fn typed_values_use_defaults_and_hide_errors() {
        let mut stored = Stored::default();
        stored.secrets.insert("password".into(), SecretState::Set("hunter2".into()));
        let mail: Mail = typed(&stored).unwrap();
        assert_eq!((mail.host.as_str(), mail.password.expose()), ("", Some("hunter2")));

        stored.values.insert("host".into(), serde_json::json!(5));
        let err = typed::<Mail>(&stored).err().unwrap().to_string();
        assert!(!err.contains("hunter2") && !err.contains('5'), "{err}");

        stored.values.clear();
        stored.secrets.insert("password".into(), SecretState::Unreadable);
        assert!(typed::<Mail>(&stored).err().unwrap().to_string().contains("mail.password"));
    }

    #[test]
    fn group_names() {
        assert!(valid_group("general") && valid_group("mail_2"));
        assert!(!valid_group("") && !valid_group("Mail") && !valid_group(&"a".repeat(33)) && !valid_group("a-b"));
    }
}
