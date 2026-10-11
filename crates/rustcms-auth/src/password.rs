//! Argon2id password hashing on the blocking pool.

use std::sync::Arc;

use argon2::Argon2;
use argon2::password_hash::{PasswordHasher, PasswordVerifier};
use rustcms_core::{AppError, AppResult};
use tokio::sync::Semaphore;

/// Hashes and verifies passwords without blocking the async runtime.
///
/// At most one hash per CPU runs at a time, so a burst of sign-ins cannot
/// starve the blocking pool. The permit travels with the blocking work, so a
/// cancelled request (timeout, disconnect) frees capacity only once its hash
/// has actually finished. Verifying against no user still costs one hash, so
/// response time does not reveal which emails exist.
#[derive(Clone)]
pub struct Passwords {
    permits: Arc<Semaphore>,
    dummy: Arc<String>,
}

impl Passwords {
    /// Creates the hasher (computes one dummy hash).
    pub fn new() -> Self {
        Self::with_permits(std::thread::available_parallelism().map_or(2, std::num::NonZero::get))
    }

    /// Hasher running at most `permits` hashes at a time.
    pub fn with_permits(permits: usize) -> Self {
        let dummy = hash_sync("rustcms-dummy-password").expect("argon2 with default parameters");
        Self { permits: Arc::new(Semaphore::new(permits.max(1))), dummy: Arc::new(dummy) }
    }

    /// Hashes not currently running.
    pub fn available_permits(&self) -> usize {
        self.permits.available_permits()
    }

    /// Argon2id PHC string for `password`.
    pub async fn hash(&self, password: String) -> AppResult<String> {
        let permit = self.permits.clone().acquire_owned().await.map_err(AppError::internal)?;
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            hash_sync(&password)
        })
        .await
        .map_err(AppError::internal)?
    }

    /// Whether `password` matches `hash`. `None` (unknown user) burns the same time and returns `false`.
    pub async fn verify(&self, password: String, hash: Option<String>) -> bool {
        let Ok(permit) = self.permits.clone().acquire_owned().await else { return false };
        let known = hash.is_some();
        let hash = hash.unwrap_or_else(|| self.dummy.as_ref().clone());
        let matches = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            Argon2::default().verify_password(password.as_bytes(), hash.as_str()).is_ok()
        })
        .await
        .unwrap_or(false);
        known && matches
    }
}

impl Default for Passwords {
    fn default() -> Self {
        Self::new()
    }
}

/// Hashes synchronously (seeders, test factories). Prefer [`Passwords::hash`] in async code.
pub fn hash_blocking(password: &str) -> AppResult<String> {
    hash_sync(password)
}

fn hash_sync(password: &str) -> AppResult<String> {
    Argon2::default().hash_password(password.as_bytes()).map(|h| h.to_string()).map_err(AppError::internal)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancelled_hashing_keeps_its_permit_until_done() {
        use std::time::{Duration, Instant};

        async fn wait_for_permit(passwords: &Passwords) {
            let deadline = Instant::now() + Duration::from_secs(10);
            while passwords.available_permits() == 0 {
                assert!(Instant::now() < deadline, "permit released once the work finishes");
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }

        let passwords = Passwords::with_permits(1);
        let cancelled = tokio::time::timeout(Duration::from_millis(1), passwords.hash("x".into())).await.is_err();
        assert!(cancelled, "the hash outlives a 1 ms timeout");
        assert_eq!(passwords.available_permits(), 0, "hash still running: capacity not released early");
        wait_for_permit(&passwords).await;

        let cancelled =
            tokio::time::timeout(Duration::from_millis(1), passwords.verify("x".into(), None)).await.is_err();
        assert!(cancelled, "the verification outlives a 1 ms timeout");
        assert_eq!(passwords.available_permits(), 0, "verification still running: capacity not released early");
        wait_for_permit(&passwords).await;
    }

    #[tokio::test]
    async fn hashes_and_verifies() {
        let passwords = Passwords::new();
        let hash = passwords.hash("correct horse".into()).await.unwrap();
        assert!(hash.starts_with("$argon2id$"));
        assert!(passwords.verify("correct horse".into(), Some(hash.clone())).await);
        assert!(!passwords.verify("wrong".into(), Some(hash)).await);
        assert!(!passwords.verify("rustcms-dummy-password".into(), None).await, "unknown user never matches");
        assert!(!passwords.verify("x".into(), Some("not a hash".into())).await);
    }
}
