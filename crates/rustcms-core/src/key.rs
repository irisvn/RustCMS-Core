//! The application key (`APP_KEY`), used to sign cookies and derive secrets.

use std::fmt;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Deserializer};

const PREFIX: &str = "base64:";
const LEN: usize = 32;

/// 32 random bytes, written as `base64:<44 chars>` like Laravel's `APP_KEY`.
#[derive(Clone, PartialEq, Eq)]
pub struct AppKey([u8; LEN]);

/// Why an `APP_KEY` value was rejected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KeyError {
    /// Missing `base64:` prefix.
    #[error("APP_KEY must start with `base64:`; run `key:generate`")]
    Format,
    /// Not valid base64 or not 32 bytes.
    #[error("APP_KEY must encode exactly {LEN} bytes; run `key:generate`")]
    Length,
}

impl AppKey {
    /// Generates a new random key.
    pub fn generate() -> Self {
        Self(rand::random())
    }

    /// Parses `base64:...`.
    pub fn parse(value: &str) -> Result<Self, KeyError> {
        let encoded = value.strip_prefix(PREFIX).ok_or(KeyError::Format)?;
        let bytes = STANDARD.decode(encoded).map_err(|_| KeyError::Length)?;
        Ok(Self(bytes.try_into().map_err(|_| KeyError::Length)?))
    }

    /// Raw key bytes.
    pub fn bytes(&self) -> &[u8; LEN] {
        &self.0
    }

    /// The `base64:...` form, for writing to `.env`.
    pub fn encode(&self) -> String {
        format!("{PREFIX}{}", STANDARD.encode(self.0))
    }
}

impl fmt::Debug for AppKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AppKey(<redacted>)")
    }
}

impl<'de> Deserialize<'de> for AppKey {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let key = AppKey::generate();
        assert_eq!(AppKey::parse(&key.encode()).unwrap(), key);
        assert_ne!(AppKey::generate(), key);
    }

    #[test]
    fn rejects_bad_keys() {
        assert_eq!(AppKey::parse("secret"), Err(KeyError::Format));
        assert_eq!(AppKey::parse("base64:c2hvcnQ="), Err(KeyError::Length));
        assert_eq!(AppKey::parse("base64:***"), Err(KeyError::Length));
    }

    #[test]
    fn debug_is_redacted() {
        assert_eq!(format!("{:?}", AppKey::generate()), "AppKey(<redacted>)");
    }
}
