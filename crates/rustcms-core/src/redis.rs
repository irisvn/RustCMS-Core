//! Redis for several instances of one application (feature `redis`):
//! shared rate limits and single-runner periodic jobs.
//!
//! Configured with `[redis] url` (or `REDIS_URL`) and `prefix`. Every key of
//! one deployment starts with its prefix: the instances of an application must
//! share it, and two applications on one Redis must not. Standalone Redis only
//! (no Cluster). Commands time out after a second and connecting after two.

use std::future::Future;
use std::time::Duration;

use serde::Deserialize;

/// `[redis]` settings.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct RedisConfig {
    /// `redis://host:port/db`; empty or absent: no Redis (one instance).
    pub url: Option<String>,
    /// Key prefix of this deployment. Default: `rustcms:<app.name>`.
    pub prefix: Option<String>,
}

/// Connected Redis, bound in the container when configured.
#[derive(Clone)]
pub struct Redis {
    connection: redis::aio::ConnectionManager,
    prefix: String,
}

impl std::fmt::Debug for Redis {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Redis").field("prefix", &self.prefix).finish_non_exhaustive()
    }
}

/// Lease tokens are compared and deleted atomically: only the owner releases.
const RELEASE: &str = r#"
if redis.call('GET', KEYS[1]) == ARGV[1] then
  return redis.call('DEL', KEYS[1])
end
return 0
"#;

impl Redis {
    /// Connects (bounded) and checks the server answers.
    pub async fn connect(url: &str, prefix: String) -> anyhow::Result<Self> {
        let client = redis::Client::open(url)?;
        let config = redis::aio::ConnectionManagerConfig::new()
            .set_connection_timeout(Some(Duration::from_secs(2)))
            .set_response_timeout(Some(Duration::from_secs(1)))
            .set_number_of_retries(1);
        let mut connection = tokio::time::timeout(
            Duration::from_secs(5),
            redis::aio::ConnectionManager::new_with_config(client, config),
        )
        .await
        .map_err(|_| anyhow::anyhow!("connecting to Redis timed out"))??;
        let _: String = redis::cmd("PING").query_async(&mut connection).await?;
        Ok(Self { connection, prefix })
    }

    /// `<prefix>:<namespace>:<name>`.
    pub fn key(&self, namespace: &str, name: &str) -> String {
        format!("{}:{namespace}:{name}", self.prefix)
    }

    /// A connection handle (cheap to clone; reconnects by itself).
    pub fn connection(&self) -> redis::aio::ConnectionManager {
        self.connection.clone()
    }

    /// Runs `job` unless another instance holds the lease `name` (for `ttl`):
    /// periodic work done by one instance at a time. The lease only avoids
    /// duplicate work: jobs must stay correct when two run (a lease can expire
    /// during a long run). Without an answer from Redis the job runs anyway.
    /// Returns whether it ran.
    pub async fn exclusive<F, Fut>(&self, name: &str, ttl: Duration, job: F) -> bool
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = ()>,
    {
        let key = self.key("lease", name);
        let token = uuid::Uuid::new_v4().to_string();
        let mut connection = self.connection();
        let acquired: Result<Option<String>, _> = redis::cmd("SET")
            .arg(&key)
            .arg(&token)
            .arg("NX")
            .arg("PX")
            .arg(ttl.as_millis() as u64)
            .query_async(&mut connection)
            .await;
        match acquired {
            Ok(Some(_)) => {}
            Ok(None) => return false,
            Err(err) => tracing::warn!(%err, lease = name, "Redis lease unavailable; running the job anyway"),
        }
        job().await;
        let released: Result<i64, _> =
            redis::Script::new(RELEASE).key(&key).arg(&token).invoke_async(&mut connection).await;
        if let Err(err) = released {
            tracing::warn!(%err, lease = name, "Redis lease not released; it expires by itself");
        }
        true
    }
}

/// Connects when `[redis] url` is set; `None` otherwise.
pub(crate) async fn from_config(config: &crate::Config) -> anyhow::Result<Option<Redis>> {
    let settings: RedisConfig = config.section("redis")?;
    let Some(url) = settings.url.filter(|u| !u.trim().is_empty()) else { return Ok(None) };
    let prefix = settings.prefix.unwrap_or_else(|| format!("rustcms:{}", config.app.name));
    anyhow::ensure!(
        !prefix.is_empty() && !prefix.contains(char::is_whitespace),
        "redis.prefix may not be empty or contain spaces"
    );
    let redis =
        Redis::connect(&url, prefix).await.map_err(|e| anyhow::anyhow!("cannot use Redis at [redis] url: {e}"))?;
    Ok(Some(redis))
}
