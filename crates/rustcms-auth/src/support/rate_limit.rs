//! Fixed-window rate limiter: in memory (one instance), or in Redis (shared
//! by every instance of the application, feature `redis`).
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
    #[cfg(feature = "redis")]
    redis: std::sync::OnceLock<rustcms_core::redis::Redis>,
}

/// `(key, max)` pairs with each key once (the lowest `max` wins).
fn unique(limits: &[(&str, u32)]) -> Vec<(String, u32)> {
    let mut out: Vec<(String, u32)> = Vec::with_capacity(limits.len());
    for (key, max) in limits {
        let key = sha256_hex(key);
        match out.iter_mut().find(|(k, _)| *k == key) {
            Some(existing) => existing.1 = existing.1.min(*max),
            None => out.push((key, *max)),
        }
    }
    out
}

/// Checks every key, then counts all of them (all-or-nothing, in one script).
/// A window starts when its key is created and is never extended. Returns 0,
/// or the milliseconds until the first full window ends.
#[cfg(feature = "redis")]
const RESERVE: &str = r#"
local window = tonumber(ARGV[1])
for i, key in ipairs(KEYS) do
  local count = tonumber(redis.call('GET', key) or '0')
  if count >= tonumber(ARGV[i + 1]) then
    local ttl = redis.call('PTTL', key)
    if ttl < 1 then ttl = 1 end
    return ttl
  end
end
for _, key in ipairs(KEYS) do
  if not redis.call('SET', key, 1, 'PX', window, 'NX') then
    redis.call('INCR', key)
  end
end
return 0
"#;

/// Gives one attempt back: never creates a key or goes below zero; the window
/// (TTL) is kept.
#[cfg(feature = "redis")]
const RELEASE: &str = r#"
local count = tonumber(redis.call('GET', KEYS[1]) or '0')
if count > 0 then redis.call('DECR', KEYS[1]) end
return 0
"#;

impl Default for RateLimiter {
    fn default() -> Self {
        Self::with_capacity(DEFAULT_CAPACITY)
    }
}

impl RateLimiter {
    /// Limiter tracking at most `capacity` keys.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: Mutex::default(),
            capacity: capacity.max(1),
            #[cfg(feature = "redis")]
            redis: std::sync::OnceLock::new(),
        }
    }

    /// Keeps the counters in Redis from now on (shared by every instance).
    #[cfg(feature = "redis")]
    pub fn use_redis(&self, redis: rustcms_core::redis::Redis) {
        let _ = self.redis.set(redis);
    }

    #[cfg(feature = "redis")]
    fn redis_error(err: redis::RedisError) -> AppError {
        // A limiter that cannot count lets nothing through.
        tracing::error!(%err, "rate limiter: Redis unavailable");
        AppError::internal("rate limiter unavailable")
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Atomically counts one attempt for every `(key, max)`, or none: fails with
    /// 429 when any key already used `max` attempts in its window (a `max` of 0
    /// refuses always). With Redis, an unreachable server fails closed (500).
    pub async fn reserve(&self, limits: &[(&str, u32)]) -> AppResult<()> {
        #[cfg(feature = "redis")]
        if let Some(redis) = self.redis.get() {
            let keys = unique(limits);
            if keys.iter().any(|(_, max)| *max == 0) {
                return Err(AppError::TooManyRequests { retry_after: WINDOW.as_secs() });
            }
            let script = redis::Script::new(RESERVE);
            let mut call = script.prepare_invoke();
            call.arg(WINDOW.as_millis() as u64);
            for (key, max) in &keys {
                call.key(redis.key("rl", key)).arg(*max);
            }
            let wait: u64 = call.invoke_async(&mut redis.connection()).await.map_err(Self::redis_error)?;
            return match wait {
                0 => Ok(()),
                ms => Err(AppError::TooManyRequests { retry_after: ms.div_ceil(1000).max(1) }),
            };
        }
        self.reserve_at(Instant::now(), limits)
    }

    fn reserve_at(&self, now: Instant, limits: &[(&str, u32)]) -> AppResult<()> {
        let keys = unique(limits);
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
    pub async fn release(&self, key: &str) -> AppResult<()> {
        #[cfg(feature = "redis")]
        if let Some(redis) = self.redis.get() {
            let script = redis::Script::new(RELEASE);
            let _: i64 = script
                .key(redis.key("rl", &sha256_hex(key)))
                .invoke_async(&mut redis.connection())
                .await
                .map_err(Self::redis_error)?;
            return Ok(());
        }
        self.release_now(key);
        Ok(())
    }

    fn release_now(&self, key: &str) {
        if let Some((_, count)) = self.lock().windows.get_mut(&sha256_hex(key)) {
            *count = count.saturating_sub(1);
        }
    }

    /// Forgets `key` (after a successful sign-in).
    pub async fn clear(&self, key: &str) -> AppResult<()> {
        #[cfg(feature = "redis")]
        if let Some(redis) = self.redis.get() {
            let _: i64 = redis::cmd("DEL")
                .arg(redis.key("rl", &sha256_hex(key)))
                .query_async(&mut redis.connection())
                .await
                .map_err(Self::redis_error)?;
            return Ok(());
        }
        self.clear_now(key);
        Ok(())
    }

    fn clear_now(&self, key: &str) {
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
            limiter.reserve_at(Instant::now(), &[("account", 3), ("ip", 10)]).unwrap();
        }
        let err = limiter.reserve_at(Instant::now(), &[("account", 3), ("ip", 10)]).unwrap_err();
        assert!(matches!(err, AppError::TooManyRequests { retry_after } if (1..=60).contains(&retry_after)));
        // The refused reservation did not count against the IP either.
        for _ in 0..7 {
            limiter.reserve_at(Instant::now(), &[("ip", 10)]).unwrap();
        }
        assert!(limiter.reserve_at(Instant::now(), &[("ip", 10)]).is_err());

        limiter.release_now("ip");
        limiter.reserve_at(Instant::now(), &[("ip", 10)]).unwrap();
        limiter.clear_now("account");
        limiter.reserve_at(Instant::now(), &[("account", 3)]).unwrap();
    }

    #[test]
    fn keys_have_a_fixed_size() {
        let limiter = RateLimiter::default();
        limiter.reserve_at(Instant::now(), &[(&"x".repeat(1_000_000), 5)]).unwrap();
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
