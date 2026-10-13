//! Random secrets, hashing and constant-time comparison.

use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

/// `len` random alphanumeric characters (≈5.95 bits each).
pub fn random_string(len: usize) -> String {
    (0..len).map(|_| ALPHABET[rand::random_range(0..ALPHABET.len())] as char).collect()
}

/// Lowercase hex SHA-256.
pub fn sha256_hex(input: &str) -> String {
    hex::encode(Sha256::digest(input.as_bytes()))
}

/// Lowercase hex HMAC-SHA256 of `input` under `key`.
pub fn hmac_sha256_hex(key: &[u8], input: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC takes keys of any length");
    mac.update(input.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

/// Constant-time string equality.
pub fn ct_eq(a: &str, b: &str) -> bool {
    a.as_bytes().ct_eq(b.as_bytes()).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers() {
        let s = random_string(40);
        assert_eq!(s.len(), 40);
        assert!(s.chars().all(|c| c.is_ascii_alphanumeric()));
        assert_ne!(random_string(40), s);
        assert_eq!(sha256_hex("abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert!(ct_eq("same", "same"));
        assert!(!ct_eq("same", "sane"));
        assert!(!ct_eq("same", "same!"));
        // RFC 4231, test case 1.
        assert_eq!(
            hmac_sha256_hex(&[0x0b; 20], "Hi There"),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
    }
}
