//! Configuration: defaults, `config/*.toml`, `.env` and environment variables.
//!
//! Precedence (last wins):
//! 1. built-in defaults,
//! 2. `config/<section>.toml` files, each merged under `<section>`,
//! 3. well-known variables: `APP_NAME`, `APP_ENV`, `APP_DEBUG`, `APP_URL`, `APP_KEY`, `DATABASE_URL`, `REDIS_URL`,
//! 4. `RUSTCMS_<SECTION>__<KEY>` variables (e.g. `RUSTCMS_HTTP__BODY_LIMIT=2097152`).
//!
//! `.env` is loaded first and never overrides variables already set.

use std::net::SocketAddr;
use std::path::Path;

use figment::Figment;
use figment::providers::{Env, Format, Serialized, Toml};
use ipnet::IpNet;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::key::AppKey;

/// Deployment environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Environment {
    /// Developer machine.
    Local,
    /// Automated tests.
    Testing,
    /// Production.
    Production,
}

/// `[app]` section.
#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    /// Application name.
    pub name: String,
    /// Environment.
    pub env: Environment,
    /// Exposes error details in logs at debug level.
    pub debug: bool,
    /// Public URL, also the default trusted origin for the admin panel.
    pub url: String,
    /// Application key (`APP_KEY`). Required.
    pub key: AppKey,
    /// Address `serve` listens on.
    pub listen: SocketAddr,
    /// Locale of the admin interface when neither the user nor the browser
    /// names a supported one.
    pub locale: String,
    /// Locales users may choose (the admin panel ships `en` and `vi`).
    pub locales: Vec<String>,
}

/// `[database]` section.
#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    /// Connection URL (`postgres://…` or `sqlite://…`).
    pub url: String,
    /// Pool size.
    pub max_connections: u32,
    /// Logs every SQL statement at debug level.
    pub log_statements: bool,
}

/// `[http]` section.
#[derive(Debug, Clone, Deserialize)]
pub struct HttpConfig {
    /// Maximum request body in bytes (413 above).
    pub body_limit: usize,
    /// Request timeout in seconds (408 above).
    pub timeout_secs: u64,
    /// Proxies whose `X-Forwarded-For` is trusted. Empty: the socket peer is the client.
    pub trusted_proxies: Vec<IpNet>,
}

/// `[log]` section.
#[derive(Debug, Clone, Deserialize)]
pub struct LogConfig {
    /// `tracing` filter, e.g. `info,rustcms=debug`. `RUST_LOG` overrides it.
    pub filter: String,
    /// Emits JSON lines instead of human-readable logs.
    pub json: bool,
}

/// Loaded configuration.
#[derive(Debug, Clone)]
pub struct Config {
    /// `[app]`.
    pub app: AppConfig,
    /// `[database]`.
    pub database: DatabaseConfig,
    /// `[http]`.
    pub http: HttpConfig,
    /// `[log]`.
    pub log: LogConfig,
    figment: Figment,
}

/// Configuration could not be loaded.
#[derive(Debug, thiserror::Error)]
#[error("invalid configuration: {0}")]
pub struct ConfigError(#[from] Box<figment::Error>);

fn defaults() -> serde_json::Value {
    serde_json::json!({
        "app": { "name": "rustcms", "env": "production", "debug": false, "url": "http://localhost:8000",
                 "listen": "127.0.0.1:8000", "locale": "en", "locales": ["en", "vi"] },
        "database": { "max_connections": 10, "log_statements": false },
        "http": { "body_limit": 1_048_576, "timeout_secs": 30, "trusted_proxies": [] },
        "log": { "filter": "info", "json": false },
    })
}

impl Config {
    /// Loads `.env` and configuration from the current directory.
    pub fn load() -> Result<Self, ConfigError> {
        dotenvy::dotenv().ok();
        Self::from_figment(Self::figment(Path::new("config"))?)
    }

    /// The layered sources without `.env` loading, for tests and tools.
    pub fn figment(config_dir: &Path) -> Result<Figment, ConfigError> {
        let mut figment = Figment::from(Serialized::defaults(defaults()));
        if let Ok(entries) = std::fs::read_dir(config_dir) {
            let mut files: Vec<_> = entries
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == "toml"))
                .collect();
            files.sort();
            for file in files {
                let section = file.file_stem().and_then(|s| s.to_str()).unwrap_or_default().to_owned();
                let value: figment::value::Value = Figment::from(Toml::file(&file)).extract().map_err(Box::new)?;
                figment = figment.merge(Serialized::default(&section, value));
            }
        }
        let well_known = [
            ("APP_NAME", "app.name"),
            ("APP_ENV", "app.env"),
            ("APP_DEBUG", "app.debug"),
            ("APP_URL", "app.url"),
            ("APP_KEY", "app.key"),
            ("DATABASE_URL", "database.url"),
            ("REDIS_URL", "redis.url"),
        ];
        for (var, path) in well_known {
            figment = figment.merge(Env::raw().only(&[var]).map(move |_| path.into()));
        }
        Ok(figment.merge(Env::prefixed("RUSTCMS_").split("__")))
    }

    /// Extracts the framework sections from `figment`.
    pub fn from_figment(figment: Figment) -> Result<Self, ConfigError> {
        let get = |section: &str| figment.focus(section);
        let app: AppConfig = get("app").extract().map_err(Box::new)?;
        let locales_ok = !app.locales.is_empty()
            && app.locales.iter().all(|l| crate::i18n::is_locale_tag(l))
            && app.locales.contains(&app.locale);
        if !locales_ok {
            let message = format!(
                "app.locale {:?} must be one of app.locales {:?} (tags such as `en`, `vi`)",
                app.locale, app.locales
            );
            return Err(ConfigError(Box::new(figment::Error::from(message))));
        }
        Ok(Self {
            app,
            database: get("database").extract().map_err(Box::new)?,
            http: get("http").extract().map_err(Box::new)?,
            log: get("log").extract().map_err(Box::new)?,
            figment,
        })
    }

    /// Extracts an application-defined section, e.g. `config/mail.toml` as `section("mail")`.
    pub fn section<T: DeserializeOwned>(&self, section: &str) -> Result<T, ConfigError> {
        self.figment.focus(section).extract().map_err(|e| ConfigError(Box::new(e)))
    }

    /// Replaces a value, for tests (`with("database.url", "sqlite::memory:")`).
    pub fn with(self, path: &str, value: impl Serialize) -> Result<Self, ConfigError> {
        Self::from_figment(self.figment.merge(Serialized::default(path, value)))
    }
}

#[cfg(test)]
#[allow(clippy::result_large_err)] // figment::Jail's closure signature
mod tests {
    use super::*;
    use figment::Jail;

    #[test]
    fn layers_files_and_environment() {
        Jail::expect_with(|jail| {
            jail.create_dir("config")?;
            jail.create_file("config/app.toml", "name = \"Blog\"\nenv = \"local\"")?;
            jail.create_file("config/http.toml", "body_limit = 10\ntrusted_proxies = [\"10.0.0.0/8\"]")?;
            jail.create_file("config/mail.toml", "from = \"no-reply@example.com\"")?;
            jail.set_env("APP_KEY", AppKey::generate().encode());
            jail.set_env("DATABASE_URL", "sqlite::memory:");
            jail.set_env("APP_ENV", "testing");
            jail.set_env("RUSTCMS_HTTP__TIMEOUT_SECS", "5");

            let config = Config::from_figment(Config::figment(Path::new("config")).unwrap()).unwrap();
            assert_eq!(config.app.name, "Blog");
            assert_eq!(config.app.env, Environment::Testing, "env var beats file");
            assert_eq!(config.http.body_limit, 10);
            assert_eq!(config.http.timeout_secs, 5);
            assert_eq!(config.http.trusted_proxies, vec!["10.0.0.0/8".parse::<IpNet>().unwrap()]);
            assert_eq!(config.database.url, "sqlite::memory:");

            #[derive(Deserialize)]
            struct Mail {
                from: String,
            }
            assert_eq!(config.section::<Mail>("mail").unwrap().from, "no-reply@example.com");
            Ok(())
        });
    }

    #[test]
    fn app_key_is_required_and_validated() {
        Jail::expect_with(|jail| {
            jail.set_env("DATABASE_URL", "sqlite::memory:");
            let err = Config::from_figment(Config::figment(Path::new("config")).unwrap()).unwrap_err();
            assert!(err.to_string().contains("key"), "{err}");

            jail.set_env("APP_KEY", "not-a-key");
            let err = Config::from_figment(Config::figment(Path::new("config")).unwrap()).unwrap_err();
            assert!(err.to_string().contains("base64:"), "{err}");
            Ok(())
        });
    }
}
