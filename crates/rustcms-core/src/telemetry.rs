//! Logging setup.

use tracing_subscriber::EnvFilter;

use crate::config::LogConfig;

/// Installs the global subscriber. `RUST_LOG` overrides the configured filter.
/// Does nothing if a subscriber is already installed (tests).
pub fn init(config: &LogConfig) {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&config.filter));
    let builder = tracing_subscriber::fmt().with_env_filter(filter);
    let _ = if config.json { builder.json().try_init() } else { builder.try_init() };
}
