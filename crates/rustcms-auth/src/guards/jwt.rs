//! JWT access tokens: compact JWS, HS256 only.
//!
//! The header is fixed byte for byte (`{"alg":"HS256","typ":"JWT"}`), so no
//! other algorithm, `none`, key id or header trick is ever considered. The
//! signature is checked in constant time before the payload is decoded, and
//! the payload must be exactly [`Claims`] (unknown fields refused).

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hmac::{Hmac, KeyInit, Mac};
use rustcms_core::AppKey;
use serde::{Deserialize, Serialize};
use sha2::Sha256;

/// `base64url({"alg":"HS256","typ":"JWT"})`: the only header accepted.
const HEADER: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9";
/// Longest token read (bytes): a few hundred is typical.
pub const MAX_LEN: usize = 4096;
/// How far in the future `iat` may be (clock differences between instances).
const MAX_IAT_SKEW_SECS: i64 = 60;

/// The claims of an access token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Claims {
    /// User id.
    pub sub: String,
    /// Issued at (Unix seconds).
    pub iat: i64,
    /// Expires at (Unix seconds): refused from this second on.
    pub exp: i64,
    /// Token id (random).
    pub jti: String,
    /// The user's `auth_version` at issue: any credential change ends the token.
    pub ver: i64,
    /// The refresh family this token belongs to.
    pub fam: String,
    /// Abilities (`*` = all the user's API permissions).
    pub scope: Vec<String>,
}

/// Why a token was refused (all answer 401; kept apart for tests and logs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JwtError {
    /// Not three base64url parts, too long, or not our header.
    Malformed,
    /// The signature does not match.
    Signature,
    /// The payload is not exactly the expected claims.
    Claims,
    /// `exp` reached, or `iat` too far in the future.
    Expired,
}

/// The signing key: `HMAC-SHA256(APP_KEY, "rustcms/jwt/v1")`, separate from
/// every other use of the application key.
#[derive(Clone)]
pub struct JwtKey([u8; 32]);

impl JwtKey {
    /// Derives the key from the application key.
    pub fn derive(app_key: &AppKey) -> Self {
        Self(derive(app_key, "rustcms/jwt/v1"))
    }

    fn mac(&self) -> Hmac<Sha256> {
        Hmac::<Sha256>::new_from_slice(&self.0).expect("HMAC takes keys of any length")
    }
}

impl std::fmt::Debug for JwtKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("JwtKey(..)")
    }
}

/// `HMAC-SHA256(APP_KEY, purpose)`: a key for one purpose only.
pub(crate) fn derive(app_key: &AppKey, purpose: &str) -> [u8; 32] {
    let mut mac = Hmac::<Sha256>::new_from_slice(app_key.bytes()).expect("HMAC takes keys of any length");
    mac.update(purpose.as_bytes());
    mac.finalize().into_bytes().into()
}

/// Signs `claims`.
pub fn encode(key: &JwtKey, claims: &Claims) -> String {
    let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(claims).expect("claims serialize"));
    let signing_input = format!("{HEADER}.{payload}");
    let mut mac = key.mac();
    mac.update(signing_input.as_bytes());
    let signature = URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());
    format!("{signing_input}.{signature}")
}

/// Whether `token` looks like a JWT (three dot-separated parts), as opposed to
/// a personal access token.
pub fn looks_like_jwt(token: &str) -> bool {
    token.split('.').count() == 3
}

/// Verifies `token` at `now` (Unix seconds) and returns its claims.
pub fn decode(key: &JwtKey, token: &str, now: i64) -> Result<Claims, JwtError> {
    if token.len() > MAX_LEN {
        return Err(JwtError::Malformed);
    }
    let mut parts = token.split('.');
    let (Some(header), Some(payload), Some(signature), None) = (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(JwtError::Malformed);
    };
    if header != HEADER {
        return Err(JwtError::Malformed);
    }
    // Strict base64url without padding: one encoding per value.
    let signature = URL_SAFE_NO_PAD.decode(signature).map_err(|_| JwtError::Malformed)?;
    let mut mac = key.mac();
    mac.update(header.as_bytes());
    mac.update(b".");
    mac.update(payload.as_bytes());
    mac.verify_slice(&signature).map_err(|_| JwtError::Signature)?;
    let payload = URL_SAFE_NO_PAD.decode(payload).map_err(|_| JwtError::Malformed)?;
    let claims: Claims = serde_json::from_slice(&payload).map_err(|_| JwtError::Claims)?;
    if now >= claims.exp || claims.iat > now + MAX_IAT_SKEW_SECS {
        return Err(JwtError::Expired);
    }
    Ok(claims)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claims(exp: i64) -> Claims {
        Claims { sub: "7".into(), iat: 1_000, exp, jti: "j".into(), ver: 3, fam: "f".into(), scope: vec!["*".into()] }
    }

    fn sign(key: &JwtKey, header: &str, payload: &str) -> String {
        let payload = URL_SAFE_NO_PAD.encode(payload);
        let input = format!("{header}.{payload}");
        let mut mac = key.mac();
        mac.update(input.as_bytes());
        format!("{input}.{}", URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes()))
    }

    #[test]
    fn round_trips_and_expires() {
        let key = JwtKey::derive(&AppKey::generate());
        let token = encode(&key, &claims(2_000));
        assert!(looks_like_jwt(&token) && token.starts_with(HEADER));
        assert_eq!(decode(&key, &token, 1_999).unwrap(), claims(2_000));
        assert_eq!(decode(&key, &token, 2_000), Err(JwtError::Expired));
        // Issued too far in the future.
        assert_eq!(decode(&key, &token, 1_000 - MAX_IAT_SKEW_SECS - 1), Err(JwtError::Expired));
    }

    #[test]
    fn refuses_other_keys_headers_and_payloads() {
        let key = JwtKey::derive(&AppKey::generate());
        let other = JwtKey::derive(&AppKey::generate());
        let token = encode(&key, &claims(2_000));
        assert_eq!(decode(&other, &token, 1_500), Err(JwtError::Signature));
        // A tampered payload, signature or header.
        let parts: Vec<&str> = token.split('.').collect();
        let forged = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&Claims { sub: "1".into(), ..claims(2_000) }).unwrap());
        assert_eq!(decode(&key, &format!("{}.{forged}.{}", parts[0], parts[2]), 1_500), Err(JwtError::Signature));
        assert_eq!(decode(&key, &format!("{}.{}.AAAA", parts[0], parts[1]), 1_500), Err(JwtError::Signature));
        let payload = serde_json::to_string(&claims(2_000)).unwrap();
        for header in [
            URL_SAFE_NO_PAD.encode(r#"{"alg":"none","typ":"JWT"}"#),
            URL_SAFE_NO_PAD.encode(r#"{"alg":"RS256","typ":"JWT"}"#),
            URL_SAFE_NO_PAD.encode(r#"{"typ":"JWT","alg":"HS256"}"#),
            URL_SAFE_NO_PAD.encode(r#"{"alg":"HS256","typ":"JWT","kid":"x"}"#),
            format!("{HEADER}="),
        ] {
            assert_eq!(decode(&key, &sign(&key, &header, &payload), 1_500), Err(JwtError::Malformed), "{header}");
        }
        assert_eq!(decode(&key, &format!("{HEADER}..."), 1_500), Err(JwtError::Malformed));
        assert_eq!(decode(&key, &"a".repeat(MAX_LEN + 1), 1_500), Err(JwtError::Malformed));
        // Signed, but not exactly the claims.
        let extra = sign(&key, HEADER, &payload.replacen('{', r#"{"admin":true,"#, 1));
        assert_eq!(decode(&key, &extra, 1_500), Err(JwtError::Claims));
        let typed = sign(&key, HEADER, &payload.replace("\"ver\":3", "\"ver\":\"3\""));
        assert_eq!(decode(&key, &typed, 1_500), Err(JwtError::Claims));
    }

    #[test]
    fn keys_are_separated_by_purpose() {
        let app = AppKey::generate();
        assert_ne!(derive(&app, "rustcms/jwt/v1"), derive(&app, "rustcms/refresh/v1"));
        assert_ne!(&derive(&app, "rustcms/jwt/v1"), app.bytes());
    }
}
