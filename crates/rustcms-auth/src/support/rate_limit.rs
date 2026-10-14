//! In-memory fixed-window rate limiter (single instance; see the support boundary).
//!
//! - Attempts are reserved *before* the expensive check runs: a burst of
//!   concurrent requests cannot all pass the limit while the first ones are
//!   still hashing, and a request cancelled mid-check stays counted.
//! - Memory is bounded: keys are stored as fixed-size hashes, expired windows
//!   are reclaimed regularly, and above `capacity` the oldest windows are dropped.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use rustcms_core::{AppError, AppResult};

use crate::secret::sha256_hex;

const WINDOW: Duration = Duration::from_secs(60);
/// Default maximum number of tracked keys (~100 bytes each).
pub const DEFAULT_CAPACITY: usize = 100_000;
/// Expired windows are reclaimed every this many reservations.
const PRUNE_EVERY: u32 = 256;

#[derive(Default)]
struct Inner {
    windows: HashMap<String, (Instant, u32)>,
    since_prune: u32,
}

/// Counts attempts per key within one-minute windows.
pub struct RateLimiter {
    inner: Mutex<Inner>,
    capacity: usize,
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::with_capacity(DEFAULT_CAPACITY)
    }
}

impl RateLimiter {
    /// Limiter tracking at most `capacity` keys.
    pub fn with_capacity(capacity: usize) -> Self {
        Self { inner: Mutex::default(), capacity: capacity.max(1) }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Atomically counts one attempt for every `(key, max)`, or none: fails with
    /// 429 when any key already used `max` attempts in its window.
    pub fn reserve(&self, limits: &[(&str, u32)]) -> AppResult<()> {
        self.reserve_at(Instant::now(), limits)
    }

    fn reserve_at(&self, now: Instant, limits: &[(&str, u32)]) -> AppResult<()> {
        let keys: Vec<(String, u32)> = limits.iter().map(|(k, max)| (sha256_hex(k), *max)).collect();
        let mut inner = self.lock();
        inner.since_prune += 1;
        if inner.since_prune >= PRUNE_EVERY || inner.windows.len() + keys.len() > self.capacity {
            self.prune(&mut inner, now);
        }
        for (key, max) in &keys {
            if let Some((start, count)) = inner.windows.get(key)
                && now.duration_since(*start) < WINDOW
                && count >= max
            {
                let retry_after = (WINDOW - now.duration_since(*start)).as_secs().max(1);
                return Err(AppError::TooManyRequests { retry_after });
            }
        }
        for (key, _) in keys {
            let entry = inner.windows.entry(key).or_insert((now, 0));
            if now.duration_since(entry.0) >= WINDOW {
                *entry = (now, 0);
            }
            entry.1 += 1;
        }
        Ok(())
    }

    /// Drops expired windows, then the oldest ones while above capacity.
    fn prune(&self, inner: &mut Inner, now: Instant) {
        inner.since_prune = 0;
        inner.windows.retain(|_, (start, _)| now.duration_since(*start) < WINDOW);
        let excess = inner.windows.len().saturating_sub(self.capacity.saturating_sub(self.capacity / 10).max(1));
        if inner.windows.len() >= self.capacity && excess > 0 {
            let mut by_age: Vec<(Instant, String)> =
                inner.windows.iter().map(|(k, (start, _))| (*start, k.clone())).collect();
            by_age.sort();
            for (_, key) in by_age.into_iter().take(excess) {
                inner.windows.remove(&key);
            }
        }
    }

    /// Gives back one reserved attempt of `key` (e.g. it succeeded).
    pub fn release(&self, key: &str) {
        if let Some((_, count)) = self.lock().windows.get_mut(&sha256_hex(key)) {
            *count = count.saturating_sub(1);
        }
    }

    /// Forgets `key` (after a successful sign-in).
    pub fn clear(&self, key: &str) {
        self.lock().windows.remove(&sha256_hex(key));
    }

    /// Number of tracked keys.
    pub fn tracked_keys(&self) -> usize {
        self.lock().windows.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserves_atomically_across_keys() {
        let limiter = RateLimiter::default();
        for _ in 0..3 {
            limiter.reserve(&[("account", 3), ("ip", 10)]).unwrap();
        }
        let err = limiter.reserve(&[("account", 3), ("ip", 10)]).unwrap_err();
        assert!(matches!(err, AppError::TooManyRequests { retry_after } if (1..=60).contains(&retry_after)));
        // The refused reservation did not count against the IP either.
        for _ in 0..7 {
            limiter.reserve(&[("ip", 10)]).unwrap();
        }
        assert!(limiter.reserve(&[("ip", 10)]).is_err());

        limiter.release("ip");
        limiter.reserve(&[("ip", 10)]).unwrap();
        limiter.clear("account");
        limiter.reserve(&[("account", 3)]).unwrap();
    }

    #[test]
    fn keys_have_a_fixed_size() {
        let limiter = RateLimiter::default();
        limiter.reserve(&[(&"x".repeat(1_000_000), 5)]).unwrap();
        assert!(limiter.lock().windows.keys().all(|k| k.len() == 64));
    }

    #[test]
    fn expired_windows_are_reclaimed() {
        let limiter = RateLimiter::default();
        let start = Instant::now();
        for i in 0..100 {
            limiter.reserve_at(start, &[(&format!("k{i}"), 5)]).unwrap();
        }
        assert_eq!(limiter.tracked_keys(), 100);
        let later = start + WINDOW + Duration::from_secs(1);
        for i in 0..PRUNE_EVERY {
            limiter.reserve_at(later, &[(&format!("fresh{}", i % 3), u32::MAX)]).unwrap();
        }
        assert_eq!(limiter.tracked_keys(), 3, "only live windows remain");
    }

    #[test]
    fn capacity_bounds_memory_by_dropping_the_oldest() {
        let limiter = RateLimiter::with_capacity(50);
        let start = Instant::now();
        for i in 0..500u64 {
            limiter.reserve_at(start + Duration::from_millis(i), &[(&format!("k{i}"), 5)]).unwrap();
            assert!(limiter.tracked_keys() <= 50);
        }
        // The newest key is still tracked and limited.
        let last = start + Duration::from_millis(499);
        for _ in 0..4 {
            limiter.reserve_at(last, &[("k499", 5)]).unwrap();
        }
        assert!(limiter.reserve_at(last, &[("k499", 5)]).is_err());
    }
}
