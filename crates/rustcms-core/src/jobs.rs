//! Periodic background work.

use std::future::Future;
use std::time::Duration;

use crate::AppState;

/// Runs `job` once for the whole deployment when Redis is configured (a lease
/// named `name`, held at most `ttl`); otherwise on this instance. Jobs must
/// stay correct when two instances run them anyway (a lease can expire during
/// a long run, and without Redis every instance runs them). Returns whether
/// this instance ran it.
pub async fn exclusive<F, Fut>(state: &AppState, name: &str, ttl: Duration, job: F) -> bool
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = ()>,
{
    #[cfg(feature = "redis")]
    if let Some(redis) = state.service::<crate::redis::Redis>() {
        return redis.exclusive(name, ttl, job).await;
    }
    let _ = (state, name, ttl);
    job().await;
    true
}
