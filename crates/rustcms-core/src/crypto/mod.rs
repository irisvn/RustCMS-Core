//! Encrypted columns and the keyring (ADR 0030).
//!
//! [`Encrypted<T>`] keeps a secret of an app table sealed with `APP_KEY`:
//! AES-256-GCM, bound to its table and column (and row, when the app knows
//! it before the insert). It is explicit: [`Encrypted::seal`] and
//! [`Encrypted::open`] take the [`Keyring`] (`state.keyring()`) and a
//! [`Context`]; loading and saving a model only move the envelope text.
//!
//! ```rust,ignore
//! #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
//! #[sea_orm(table_name = "services")]
//! pub struct Model {
//!     #[sea_orm(primary_key)]
//!     pub id: i64,
//!     #[sea_orm(column_type = "Text", nullable)]
//!     pub env: Option<Encrypted<BTreeMap<String, String>>>,
//! }
//!
//! let context = Context::column("services", "env");
//! let sealed = Encrypted::seal(state.keyring(), &context, &env)?;
//! let env: BTreeMap<String, String> = service.env.unwrap().open(state.keyring(), &context)?;
//! ```

use std::fmt;
use std::marker::PhantomData;

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hmac::Mac;
use sea_orm::sea_query::{ArrayType, ColumnType, Nullable, ValueType, ValueTypeErr};
use sea_orm::{ColIdx, QueryResult, TryGetError, TryGetable, Value};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{AppError, AppKey, AppResult};

mod columns;
pub use columns::{
    Binding, ColumnReport, EncryptedColumn, EncryptedColumns, KeyReencrypt, reencrypt_column, reencrypt_settings,
};

/// Clap definitions of the key commands (for `rustcms docs:cli`).
pub fn definitions() -> Vec<clap::Command> {
    use crate::AppCommand;
    vec![KeyReencrypt.definition()]
}

/// Version and prefix of an envelope.
const VERSION: &str = "rcms1";
/// Purpose of the encryption key.
const KEY_PURPOSE: &str = "rustcms/encrypted/v1";
/// Purpose of the key id.
const ID_PURPOSE: &str = "rustcms/key-id/v1";
/// Purpose of the revision key.
const REVISION_PURPOSE: &str = "rustcms/encrypted-revision/v1";
/// Domain of the additional data.
const AAD_DOMAIN: &str = "rustcms/encrypted/v1";
const NONCE_LEN: usize = 12;
const TAG_LEN: usize = 16;
const ID_LEN: usize = 8;
/// Largest plaintext (the JSON of the value).
pub const MAX_PLAINTEXT: usize = 32 * 1024;
/// Largest envelope read (checked before decoding); a sealed 32 KiB fits a
/// MySQL `TEXT` column.
pub const MAX_ENVELOPE: usize = 60_000;

/// Identifies a key without revealing it (deterministic: the same key always
/// has the same id).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyId([u8; ID_LEN]);

impl KeyId {
    fn of(key: &AppKey) -> Self {
        let derived = key.derive(ID_PURPOSE);
        Self(derived[..ID_LEN].try_into().expect("8 bytes"))
    }

    fn hex(self) -> String {
        hex::encode(self.0)
    }
}

impl fmt::Debug for KeyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.hex())
    }
}

/// Why a keyring was refused (never quoting a key).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KeyringError {
    /// The same key twice, or a previous key equal to `APP_KEY`.
    #[error("app.previous_keys lists the same key twice, or APP_KEY itself")]
    Duplicate,
    /// Two different keys with the same id.
    #[error("two keys of app.previous_keys have the same key id: generate a new APP_KEY")]
    IdCollision,
}

/// `APP_KEY` (seals and opens) and `app.previous_keys` (only open, while
/// `key:reencrypt` moves everything to the current key). Sessions, JWTs and
/// links never use previous keys.
#[derive(Clone)]
pub struct Keyring {
    keys: Vec<(KeyId, AppKey)>,
}

impl Keyring {
    /// A keyring with `current` and the keys it replaced.
    pub fn new(current: AppKey, previous: Vec<AppKey>) -> Result<Self, KeyringError> {
        let mut keys: Vec<(KeyId, AppKey)> = Vec::with_capacity(previous.len() + 1);
        for key in std::iter::once(current).chain(previous) {
            let id = KeyId::of(&key);
            if keys.iter().any(|(_, k)| *k == key) {
                return Err(KeyringError::Duplicate);
            }
            if keys.iter().any(|(i, _)| *i == id) {
                return Err(KeyringError::IdCollision);
            }
            keys.push((id, key));
        }
        Ok(Self { keys })
    }

    /// A keyring of `key` alone.
    pub fn single(key: AppKey) -> Self {
        Self { keys: vec![(KeyId::of(&key), key)] }
    }

    /// The current key (`APP_KEY`).
    pub fn current(&self) -> &AppKey {
        &self.keys[0].1
    }

    /// The previous keys, newest first as configured.
    pub fn previous(&self) -> impl Iterator<Item = &AppKey> {
        self.keys[1..].iter().map(|(_, k)| k)
    }

    /// Every key, the current one first.
    pub fn all(&self) -> impl Iterator<Item = &AppKey> {
        self.keys.iter().map(|(_, k)| k)
    }

    fn current_id(&self) -> KeyId {
        self.keys[0].0
    }

    fn by_id(&self, id: KeyId) -> Option<&AppKey> {
        self.keys.iter().find(|(i, _)| *i == id).map(|(_, k)| k)
    }

    /// An opaque revision of `envelope`: changes with every write, reveals
    /// nothing of the content (keyed with the current key).
    pub fn revision(&self, envelope: &str) -> String {
        let mut mac = hmac::Hmac::<sha2::Sha256>::new_from_slice(&self.current().derive(REVISION_PURPOSE))
            .expect("HMAC takes keys of any length");
        mac.update(envelope.as_bytes());
        hex::encode(&mac.finalize().into_bytes()[..8])
    }
}

impl fmt::Debug for Keyring {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ids: Vec<KeyId> = self.keys.iter().map(|(i, _)| *i).collect();
        f.debug_struct("Keyring").field("ids", &ids).finish()
    }
}

/// Where a secret lives: its table and column, and its row when the app
/// knows the row's key before the insert (a UUID). The ciphertext opens only
/// in the same context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Context {
    table: String,
    column: String,
    row: Option<String>,
}

/// A row's identity in a [`Context`], in one canonical text form.
pub trait RowIdentity {
    /// Decimal for integers, lowercase hyphenated for UUIDs, as is for text.
    fn canonical(&self) -> String;
}

macro_rules! integer_rows {
    ($($t:ty),*) => {$(
        impl RowIdentity for $t {
            fn canonical(&self) -> String {
                self.to_string()
            }
        }
    )*};
}
integer_rows!(i16, i32, i64, u16, u32, u64);

impl RowIdentity for uuid::Uuid {
    fn canonical(&self) -> String {
        self.hyphenated().to_string()
    }
}

impl RowIdentity for &str {
    fn canonical(&self) -> String {
        (*self).to_owned()
    }
}

impl RowIdentity for String {
    fn canonical(&self) -> String {
        self.clone()
    }
}

impl Context {
    /// Column `column` of table `table` (with its schema, if any:
    /// `"tenant.services"`).
    pub fn column(table: impl Into<String>, column: impl Into<String>) -> Self {
        Self { table: table.into(), column: column.into(), row: None }
    }

    /// Bound to one row too.
    #[must_use]
    pub fn row(mut self, row: impl RowIdentity) -> Self {
        self.row = Some(row.canonical());
        self
    }

    /// The table.
    pub fn table(&self) -> &str {
        &self.table
    }

    /// The column.
    pub fn column_name(&self) -> &str {
        &self.column
    }

    /// The additional data: length-prefixed parts, so no two contexts (or
    /// headers) give the same bytes.
    fn aad(&self, header: &str) -> Vec<u8> {
        let mut out = Vec::new();
        let mut part = |bytes: &[u8]| {
            out.extend_from_slice(&<u32 as TryFrom<usize>>::try_from(bytes.len()).unwrap_or(u32::MAX).to_be_bytes());
            out.extend_from_slice(bytes);
        };
        part(AAD_DOMAIN.as_bytes());
        part(header.as_bytes());
        part(self.table.as_bytes());
        part(self.column.as_bytes());
        match &self.row {
            None => part(&[0]),
            Some(row) => {
                part(&[1]);
                part(row.as_bytes());
            }
        }
        out
    }
}

/// A secret that cannot be opened: a wrong or missing key, another context,
/// an altered or malformed envelope. All look alike, by design.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the encrypted value cannot be read")]
pub struct Unreadable;

impl From<Unreadable> for AppError {
    fn from(_: Unreadable) -> Self {
        AppError::internal("an encrypted value cannot be read (APP_KEY or app.previous_keys)")
    }
}

/// A value of type `T` sealed with the keyring: its envelope
/// `rcms1.<key id>.<nonce ‖ ciphertext ‖ tag>` is what the column stores.
///
/// `Debug` shows only the key id; `Serialize` writes `null`; `Deserialize`
/// always fails (it is never built from client JSON, so
/// `ActiveModel::from_json` does not work on a model with one).
pub struct Encrypted<T> {
    envelope: String,
    _value: PhantomData<fn() -> T>,
}

impl<T> Clone for Encrypted<T> {
    fn clone(&self) -> Self {
        Self::from_envelope(self.envelope.clone())
    }
}

impl<T> PartialEq for Encrypted<T> {
    fn eq(&self, other: &Self) -> bool {
        self.envelope == other.envelope
    }
}

impl<T> Eq for Encrypted<T> {}

/// The parts of a well-formed envelope.
struct Parts<'a> {
    header: &'a str,
    id: KeyId,
    sealed: Vec<u8>,
}

fn parse(envelope: &str) -> Option<Parts<'_>> {
    if envelope.len() > MAX_ENVELOPE {
        return None;
    }
    let (version, rest) = envelope.split_once('.')?;
    let (id, payload) = rest.split_once('.')?;
    if version != VERSION || id.len() != ID_LEN * 2 || !id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        return None;
    }
    let id = KeyId(hex::decode(id).ok()?.try_into().ok()?);
    let sealed = URL_SAFE_NO_PAD.decode(payload).ok()?;
    // Canonical encoding only (no trailing bits set).
    if URL_SAFE_NO_PAD.encode(&sealed) != payload || sealed.len() < NONCE_LEN + TAG_LEN {
        return None;
    }
    Some(Parts { header: &envelope[..VERSION.len() + 1 + ID_LEN * 2], id, sealed })
}

/// Whether `text` is shaped like an envelope (`rcms1.<key id>.<payload>`):
/// screens that must never show ciphertext mask such values.
pub fn looks_sealed(text: &str) -> bool {
    parse(text).is_some()
}

fn cipher(key: &AppKey) -> Aes256Gcm {
    Aes256Gcm::new(&key.derive(KEY_PURPOSE).into())
}

impl<T> Encrypted<T> {
    /// An envelope as stored (loading a row; `key:reencrypt`). It is not
    /// checked: [`open`](Self::open) is.
    pub fn from_envelope(envelope: String) -> Self {
        Self { envelope, _value: PhantomData }
    }

    /// The stored text (ciphertext, never the value).
    pub fn envelope(&self) -> &str {
        &self.envelope
    }

    /// The id of the key it claims (unauthenticated: a hint only).
    pub fn key_id(&self) -> Option<KeyId> {
        parse(&self.envelope).map(|p| p.id)
    }

    /// The plaintext bytes, if a key of `keyring` opens it in `context`, and
    /// whether that key is the current one.
    fn decrypt(&self, keyring: &Keyring, context: &Context) -> Result<(Vec<u8>, bool), Unreadable> {
        let parts = parse(&self.envelope).ok_or(Unreadable)?;
        let key = keyring.by_id(parts.id).ok_or(Unreadable)?;
        let (nonce, sealed) = parts.sealed.split_at(NONCE_LEN);
        let nonce: [u8; NONCE_LEN] = nonce.try_into().map_err(|_| Unreadable)?;
        let aad = context.aad(parts.header);
        let plain =
            cipher(key).decrypt(&Nonce::from(nonce), Payload { msg: sealed, aad: &aad }).map_err(|_| Unreadable)?;
        Ok((plain, parts.id == keyring.current_id()))
    }

    /// Whether it opens with the current key in `context` (authenticated).
    pub fn is_current(&self, keyring: &Keyring, context: &Context) -> bool {
        self.decrypt(keyring, context).is_ok_and(|(_, current)| current)
    }

    /// The same value sealed with the current key, in the same context
    /// (`key:reencrypt`); the value is never parsed.
    pub fn resealed(&self, keyring: &Keyring, context: &Context) -> Result<Self, Unreadable> {
        let (plain, _) = self.decrypt(keyring, context)?;
        seal_bytes(keyring, context, &plain).map_err(|_| Unreadable)
    }
}

fn seal_bytes<T>(keyring: &Keyring, context: &Context, plain: &[u8]) -> AppResult<Encrypted<T>> {
    if plain.len() > MAX_PLAINTEXT {
        return Err(AppError::internal("an encrypted value is larger than 32 KiB"));
    }
    let header = format!("{VERSION}.{}", keyring.current_id().hex());
    let nonce: [u8; NONCE_LEN] = rand::random();
    let aad = context.aad(&header);
    let sealed = cipher(keyring.current())
        .encrypt(&Nonce::from(nonce), Payload { msg: plain, aad: &aad })
        .map_err(|_| AppError::internal("encryption failed"))?;
    let mut payload = nonce.to_vec();
    payload.extend_from_slice(&sealed);
    Ok(Encrypted::from_envelope(format!("{header}.{}", URL_SAFE_NO_PAD.encode(payload))))
}

impl<T: Serialize + DeserializeOwned> Encrypted<T> {
    /// Seals `value` with the current key for `context`. Errors never quote
    /// the value.
    pub fn seal(keyring: &Keyring, context: &Context, value: &T) -> AppResult<Self> {
        let plain =
            serde_json::to_vec(value).map_err(|_| AppError::internal("an encrypted value cannot be serialized"))?;
        seal_bytes(keyring, context, &plain)
    }

    /// The value, if a key of `keyring` opens it in `context`.
    pub fn open(&self, keyring: &Keyring, context: &Context) -> Result<T, Unreadable> {
        let (plain, _) = self.decrypt(keyring, context)?;
        serde_json::from_slice(&plain).map_err(|_| Unreadable)
    }
}

impl<T> fmt::Debug for Encrypted<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.key_id() {
            Some(id) => write!(f, "Encrypted({id:?})"),
            None => f.write_str("Encrypted(malformed)"),
        }
    }
}

impl<T> Serialize for Encrypted<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_none()
    }
}

impl<'de, T> Deserialize<'de> for Encrypted<T> {
    fn deserialize<D: Deserializer<'de>>(_: D) -> Result<Self, D::Error> {
        Err(serde::de::Error::custom("an encrypted value is never deserialized"))
    }
}

impl<T> From<Encrypted<T>> for Value {
    fn from(value: Encrypted<T>) -> Self {
        Value::String(Some(value.envelope))
    }
}

impl<T> TryGetable for Encrypted<T> {
    fn try_get_by<I: ColIdx>(res: &QueryResult, index: I) -> Result<Self, TryGetError> {
        String::try_get_by(res, index).map(Self::from_envelope)
    }
}

impl<T> ValueType for Encrypted<T> {
    fn try_from(value: Value) -> Result<Self, ValueTypeErr> {
        match value {
            Value::String(Some(envelope)) => Ok(Self::from_envelope(envelope)),
            _ => Err(ValueTypeErr),
        }
    }

    fn type_name() -> String {
        "Encrypted".to_owned()
    }

    fn array_type() -> ArrayType {
        ArrayType::String
    }

    fn column_type() -> ColumnType {
        ColumnType::Text
    }
}

impl<T> Nullable for Encrypted<T> {
    fn null() -> Value {
        Value::String(None)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn ring() -> Keyring {
        Keyring::single(AppKey::generate())
    }

    fn ctx() -> Context {
        Context::column("services", "env")
    }

    #[test]
    fn round_trips_and_redacts() {
        let keyring = ring();
        let value: BTreeMap<String, String> = [("DB_PASSWORD".to_owned(), "marker-secret".to_owned())].into();
        let sealed = Encrypted::seal(&keyring, &ctx(), &value).unwrap();
        assert!(!sealed.envelope().contains("marker"));
        assert_eq!(sealed.open(&keyring, &ctx()).unwrap(), value);
        assert!(sealed.is_current(&keyring, &ctx()));
        assert!(!format!("{sealed:?}").contains("marker"));
        assert_eq!(serde_json::to_string(&sealed).unwrap(), "null");
        assert!(serde_json::from_str::<Encrypted<String>>("\"x\"").is_err());
        // A new nonce every time.
        assert_ne!(Encrypted::seal(&keyring, &ctx(), &value).unwrap(), sealed);
    }

    #[test]
    fn opens_only_in_its_context() {
        let keyring = ring();
        let sealed = Encrypted::seal(&keyring, &ctx(), &"s".to_owned()).unwrap();
        for other in [
            Context::column("services", "envs"),
            Context::column("service", "env"),
            Context::column("services", "env").row(1i64),
            Context::column("services", "env").row("-"),
        ] {
            assert_eq!(sealed.open(&keyring, &other), Err(Unreadable), "{other:?}");
        }
        // Length prefixes: moving a `/` between parts is another context.
        let a = Encrypted::seal(&keyring, &Context::column("a/b", "c"), &1).unwrap();
        assert!(a.open(&keyring, &Context::column("a", "b/c")).is_err());
        let rowed = Context::column("t", "c").row(uuid::Uuid::nil());
        let sealed = Encrypted::seal(&keyring, &rowed, &1).unwrap();
        assert_eq!(sealed.open(&keyring, &rowed), Ok(1));
        assert!(sealed.open(&keyring, &Context::column("t", "c")).is_err());
    }

    #[test]
    fn tampered_envelopes_are_unreadable() {
        let keyring = ring();
        let sealed = Encrypted::<String>::seal(&keyring, &ctx(), &"s".to_owned()).unwrap();
        let envelope = sealed.envelope().to_owned();
        let (head, payload) = envelope.rsplit_once('.').unwrap();
        let mut bytes = URL_SAFE_NO_PAD.decode(payload).unwrap();
        let flip = |bytes: &mut Vec<u8>, at: usize| {
            bytes[at] ^= 1;
            format!("{head}.{}", URL_SAFE_NO_PAD.encode(&*bytes))
        };
        let nonce = flip(&mut bytes.clone(), 0);
        let body = flip(&mut bytes.clone(), NONCE_LEN);
        let last = bytes.len() - 1;
        let tag = flip(&mut bytes, last);
        let other_id = envelope.replacen(&head[6..], "0000000000000000", 1);
        for bad in [
            nonce,
            body,
            tag,
            other_id,
            format!("{envelope}="),
            envelope.replacen("rcms1", "rcms2", 1),
            envelope.to_uppercase(),
            "rcms1.x.y".to_owned(),
            format!("{head}.{}", "A".repeat(MAX_ENVELOPE)),
        ] {
            assert_eq!(
                Encrypted::<String>::from_envelope(bad.clone()).open(&keyring, &ctx()),
                Err(Unreadable),
                "{bad:.40}"
            );
        }
    }

    #[test]
    fn previous_keys_open_and_reseal_makes_current() {
        let (old, new) = (AppKey::generate(), AppKey::generate());
        let before = Keyring::single(old.clone());
        let sealed = Encrypted::seal(&before, &ctx(), &"s".to_owned()).unwrap();
        let rotated = Keyring::new(new.clone(), vec![old]).unwrap();
        assert_eq!(sealed.open(&rotated, &ctx()).unwrap(), "s");
        assert!(!sealed.is_current(&rotated, &ctx()));
        let resealed = sealed.resealed(&rotated, &ctx()).unwrap();
        assert!(resealed.is_current(&rotated, &ctx()));
        assert_eq!(resealed.open(&Keyring::single(new), &ctx()).unwrap(), "s");
        assert!(sealed.open(&Keyring::single(AppKey::generate()), &ctx()).is_err());
    }

    #[test]
    fn keyrings_refuse_duplicates() {
        let key = AppKey::generate();
        assert_eq!(Keyring::new(key.clone(), vec![key.clone()]).err(), Some(KeyringError::Duplicate));
        let other = AppKey::generate();
        assert_eq!(Keyring::new(key, vec![other.clone(), other]).err(), Some(KeyringError::Duplicate));
    }

    #[test]
    fn the_largest_value_fits_a_mysql_text_column() {
        let keyring = ring();
        // Escaped characters make the JSON longer than the string (8 bytes
        // for these 2): the limit is on the JSON, so it still fits.
        let worst = "\"\u{1}".repeat(MAX_PLAINTEXT / 8 - 1);
        let sealed = Encrypted::seal(&keyring, &ctx(), &worst).unwrap();
        assert!(sealed.envelope().len() < 65_535);
        assert!(Encrypted::seal(&keyring, &ctx(), &"\u{1}".repeat(MAX_PLAINTEXT / 6 + 1)).is_err());
        let big = "x".repeat(MAX_PLAINTEXT - 2);
        let sealed = Encrypted::seal(&keyring, &ctx(), &big).unwrap();
        assert!(sealed.envelope().len() <= MAX_ENVELOPE && sealed.envelope().len() < 65_535);
        assert_eq!(sealed.open(&keyring, &ctx()).unwrap(), big);
        let err = Encrypted::seal(&keyring, &ctx(), &"x".repeat(MAX_PLAINTEXT)).unwrap_err().to_string();
        assert!(!err.contains("xxx"), "{err}");
    }

    #[test]
    fn revisions_change_with_the_envelope() {
        let keyring = ring();
        let a = Encrypted::seal(&keyring, &ctx(), &1).unwrap();
        let b = Encrypted::seal(&keyring, &ctx(), &1).unwrap();
        assert_ne!(keyring.revision(a.envelope()), keyring.revision(b.envelope()));
        assert_eq!(keyring.revision(a.envelope()), keyring.revision(a.envelope()));
        assert_eq!(keyring.revision(a.envelope()).len(), 16);
    }
}
