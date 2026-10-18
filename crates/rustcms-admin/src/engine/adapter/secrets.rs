//! Encrypted fields (`Field::encrypted`, `Field::encrypted_map`; ADR 0030):
//! the masked views the wire gets, the write operations, sealing on write.
//!
//! Views: `{"state": "set" | "unset" | "unreadable"}` for a string; for a map
//! also `"keys"` (when the user may read the field) and `"revision"`.
//! Writes: a string takes `{"replace": "…"}` or `{"clear": true}`; a map takes
//! `{"revision", "set": {…}, "keep": […]}` or `{"revision", "clear": true}`.
//! A field left out is unchanged.

use std::collections::BTreeMap;

use rustcms_core::crypto::{Encrypted, EncryptedColumn, Keyring, MAX_PLAINTEXT};
use serde_json::{Value as Json, json};
use sha2::{Digest, Sha256};

use super::*;
use crate::field::SecretKind;

/// The revision of a map that is not set.
pub(crate) const NO_REVISION: &str = "none";

/// An encrypted field's column and kind.
pub(super) struct SecretField {
    pub(super) kind: SecretKind,
    pub(super) column: EncryptedColumn,
}

fn stale() -> AppError {
    AppError::Conflict {
        code: "secret_stale",
        reason: "stale",
        message: "This was changed by someone else meanwhile. Reload and try again.",
    }
}

/// The envelope stored in `value`, if any.
fn envelope(value: &Value) -> Option<&str> {
    match value {
        Value::String(Some(text)) => Some(text.as_str()),
        _ => None,
    }
}

/// Checks the shape of a write operation (client input and hook output): no
/// secret value is ever quoted in a message.
pub(super) fn check_operation(
    name: &str,
    kind: SecretKind,
    op: Operation,
    nullable: bool,
    value: &Json,
    errors: &mut ValidationErrors,
) -> bool {
    let invalid = |errors: &mut ValidationErrors| {
        errors.add(name, "is invalid");
        false
    };
    let Some(object) = value.as_object() else { return invalid(errors) };
    let clear = object.get("clear");
    match kind {
        SecretKind::Text => match (object.get("replace"), clear, object.len()) {
            (Some(Json::String(text)), None, 1) if !text.is_empty() => true,
            (Some(Json::String(_)), None, 1) => {
                errors.add(name, "is required");
                false
            }
            (None, Some(Json::Bool(true)), 1) if nullable => true,
            (None, Some(Json::Bool(true)), 1) => {
                errors.add(name, "is required");
                false
            }
            _ => invalid(errors),
        },
        SecretKind::Map => {
            if object.keys().any(|k| !matches!(k.as_str(), "revision" | "set" | "keep" | "clear")) {
                return invalid(errors);
            }
            if object.get("revision").is_some_and(|r| !r.is_string()) {
                return invalid(errors);
            }
            if op == Operation::Create && object.contains_key("revision") {
                return invalid(errors);
            }
            if let Some(clear) = clear {
                if clear != &Json::Bool(true) || object.contains_key("set") || object.contains_key("keep") {
                    return invalid(errors);
                }
                if !nullable {
                    errors.add(name, "is required");
                    return false;
                }
                return true;
            }
            let set = match object.get("set") {
                None => None,
                Some(Json::Object(set)) if set.values().all(Json::is_string) => Some(set),
                Some(_) => return invalid(errors),
            };
            let keep: Vec<&str> = match object.get("keep") {
                None => Vec::new(),
                Some(Json::Array(keep)) if keep.iter().all(Json::is_string) => {
                    keep.iter().filter_map(Json::as_str).collect()
                }
                Some(_) => return invalid(errors),
            };
            if op == Operation::Create && !keep.is_empty() {
                errors.add(name, "is not allowed");
                return false;
            }
            let mut seen = std::collections::HashSet::new();
            if keep.iter().any(|k| !seen.insert(*k) || set.is_some_and(|s| s.contains_key(*k))) {
                errors.add(name, "may only be given once");
                return false;
            }
            true
        }
    }
}

impl<R: Resource> ResourceAdapter<R> {
    pub(super) fn secret(&self, name: &str) -> Option<&SecretField> {
        self.secrets.get(name)
    }

    /// The masked view of an encrypted field for the wire (`names`: whether
    /// the user may see a map's key names).
    pub(super) fn secret_view(&self, ctx: &Ctx, name: &str, model: &Model<R>, names: bool) -> Json {
        let Some(secret) = self.secret(name) else { return Json::Null };
        let stored = model.get(self.column(name).column);
        let Some(sealed) = envelope(&stored) else {
            return match secret.kind {
                SecretKind::Text => json!({ "state": "unset" }),
                SecretKind::Map => json!({ "state": "unset", "revision": NO_REVISION }),
            };
        };
        let keyring = ctx.keyring().ok();
        let context = secret.column.context("");
        match secret.kind {
            SecretKind::Text => {
                let opened = keyring
                    .is_some_and(|k| Encrypted::<String>::from_envelope(sealed.to_owned()).open(k, &context).is_ok());
                json!({ "state": if opened { "set" } else { "unreadable" } })
            }
            SecretKind::Map => {
                let revision = keyring.map_or_else(|| NO_REVISION.to_owned(), |k| k.revision(sealed));
                let opened = keyring.and_then(|k| {
                    Encrypted::<BTreeMap<String, String>>::from_envelope(sealed.to_owned()).open(k, &context).ok()
                });
                match opened {
                    Some(map) if names => {
                        json!({ "state": "set", "keys": map.keys().collect::<Vec<_>>(), "revision": revision })
                    }
                    Some(_) => json!({ "state": "set", "revision": revision }),
                    None => json!({ "state": "unreadable", "revision": revision }),
                }
            }
        }
    }

    /// What the audit snapshot holds for an encrypted field: a digest of the
    /// envelope (compared, then always redacted; never the value).
    pub(super) fn secret_digest(&self, name: &str, model: &Model<R>) -> Json {
        let stored = model.get(self.column(name).column);
        envelope(&stored).map_or(Json::Null, |sealed| json!(hex::encode(Sha256::digest(sealed.as_bytes()))))
    }

    /// The value to store for an encrypted field from its write operation,
    /// resolved against `stored` (the locked record of an update).
    pub(super) fn seal_operation(
        &self,
        ctx: &Ctx,
        name: &str,
        operation: &Json,
        op: Operation,
        stored: Option<&Model<R>>,
        errors: &mut ValidationErrors,
    ) -> AppResult<Option<Value>> {
        let secret = self.secret(name).ok_or_else(|| AppError::internal("not an encrypted field"))?;
        let keyring = ctx.keyring()?;
        let context = secret.column.context("");
        let object = operation.as_object().ok_or_else(|| AppError::internal("checked operation"))?;
        let cleared = Value::String(None);
        let too_long = |errors: &mut ValidationErrors| {
            errors.add(name, "is too long");
            Ok(None)
        };
        match secret.kind {
            SecretKind::Text => {
                if object.contains_key("clear") {
                    return Ok(Some(cleared));
                }
                let text = object["replace"].as_str().unwrap_or_default().to_owned();
                if let Some(max) = self.field(name).and_then(|f| f.max_length)
                    && text.chars().count() > max
                {
                    errors.add(name, format!("may not be longer than {max} characters"));
                    return Ok(None);
                }
                if serde_json::to_vec(&text).map_or(true, |j| j.len() > MAX_PLAINTEXT) {
                    return too_long(errors);
                }
                Ok(Some(Encrypted::seal(keyring, &context, &text)?.into()))
            }
            SecretKind::Map => {
                let stored = stored.map(|m| m.get(self.column(name).column));
                let stored = stored.as_ref().and_then(envelope);
                // An existing record's map changes only at the revision seen
                // (a create carries none: checked with the operation).
                if op == Operation::Update {
                    let current = stored.map_or_else(|| NO_REVISION.to_owned(), |s| keyring.revision(s));
                    if object.get("revision").and_then(Json::as_str) != Some(current.as_str()) {
                        return Err(stale());
                    }
                }
                if object.contains_key("clear") {
                    return Ok(Some(cleared));
                }
                let mut map = BTreeMap::new();
                let keep: Vec<&str> = object
                    .get("keep")
                    .and_then(Json::as_array)
                    .map_or_else(Vec::new, |k| k.iter().filter_map(Json::as_str).collect());
                if !keep.is_empty() {
                    let opened = stored.and_then(|s| {
                        Encrypted::<BTreeMap<String, String>>::from_envelope(s.to_owned()).open(keyring, &context).ok()
                    });
                    let Some(old) = opened else {
                        errors.add(name, "cannot be decrypted; replace or clear it");
                        return Ok(None);
                    };
                    for key in keep {
                        let Some(value) = old.get(key) else {
                            errors.add(name, "was not found");
                            return Ok(None);
                        };
                        map.insert(key.to_owned(), value.clone());
                    }
                }
                if let Some(Json::Object(set)) = object.get("set") {
                    for (key, value) in set {
                        map.insert(key.clone(), value.as_str().unwrap_or_default().to_owned());
                    }
                }
                let bounds = self.secret_bounds(name);
                let as_json = Json::Object(map.iter().map(|(k, v)| (k.clone(), Json::String(v.clone()))).collect());
                if let Some(problem) = crate::engine::validate::key_value_problem(&as_json, bounds) {
                    errors.add(name, problem);
                    return Ok(None);
                }
                if serde_json::to_vec(&map).map_or(true, |j| j.len() > MAX_PLAINTEXT) {
                    return too_long(errors);
                }
                Ok(Some(Encrypted::seal(keyring, &context, &map)?.into()))
            }
        }
    }

    /// Most pairs and longest value of an encrypted map.
    pub(super) fn secret_bounds(&self, name: &str) -> (usize, usize) {
        let field = self.field(name);
        (
            field.and_then(|f| f.max_items).unwrap_or(KEY_VALUE_ITEMS),
            field.and_then(|f| f.max_length).unwrap_or(KEY_VALUE_LENGTH),
        )
    }

    /// `input` as idempotency claims hash it: secret operations keyed with
    /// the keyring first, so a stored request hash never allows guessing a
    /// secret offline.
    pub(super) fn claim_input(&self, ctx: &Ctx, input: &Data) -> Data {
        let mut out = input.clone();
        for name in self.secrets.keys() {
            if let Some(value) = out.get_mut(name) {
                let digest = ctx.keyring().map_or_else(
                    |_| hex::encode(Sha256::digest(value.to_string().as_bytes())),
                    |k: &Keyring| k.revision(&value.to_string()),
                );
                *value = Json::String(digest);
            }
        }
        out
    }
}
