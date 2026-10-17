//! Configuration: defaults, `config/*.toml`, `.env` and environment variables.
//!
//! Precedence (last wins):
//! 1. built-in defaults,
//! 2. `config/<section>.toml` files, each merged under `<section>`,
//! 3. well-known variables: `APP_NAME`, `APP_ENV`, `APP_DEBUG`, `APP_URL`, `APP_KEY`, `DATABASE_URL`, `REDIS_URL`,
//! 4. `RUSTCMS_<SECTION>__<KEY>` variables (e.g. `RUSTCMS_HTTP__BODY_LIMIT=2097152`).
//!
//! `.env` is loaded first and never overrides variables already set.
//!
//! Every variable of 3. and 4. can come from a file instead (Docker or
//! Kubernetes secrets): `<VAR>_FILE` names it, e.g. `APP_KEY_FILE=/run/secrets/app_key`
//! or `RUSTCMS_MAIL__SMTP__PASSWORD_FILE=…`. The value keeps its variable's
//! place in the order above. Giving both the variable and its `_FILE` is an
//! error; so is an empty, unreadable, non-UTF-8 or larger than 64 KiB file.
//! One trailing line break is removed, nothing else. A key whose own name ends
//! in `_file` cannot be set from the environment (use a TOML file).

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

/// Largest secret file read (`<VAR>_FILE`).
const MAX_SECRET_FILE: u64 = 64 * 1024;

/// The `<VAR>_FILE` variables of the environment.
struct SecretFiles {
    /// `(variable without _FILE, path)`.
    vars: Vec<(String, std::path::PathBuf)>,
}

impl SecretFiles {
    fn from_env() -> Result<Self, ConfigError> {
        let mut vars = Vec::new();
        for (name, path) in std::env::vars_os() {
            let Some(name) = name.to_str() else { continue };
            let Some(var) = name.strip_suffix("_FILE") else { continue };
            // `RUSTCMS_LOG__FILE` is the key `log.file`, not a file of `RUSTCMS_LOG_`.
            if var.is_empty() || var.ends_with('_') {
                continue;
            }
            vars.push((var.to_owned(), std::path::PathBuf::from(path)));
        }
        Ok(Self { vars })
    }

    /// The variables, lowercased without the `RUSTCMS_` prefix, as `Env::prefixed` sees them.
    fn names(&self) -> Vec<String> {
        self.vars
            .iter()
            .filter_map(|(var, _)| var.strip_prefix("RUSTCMS_"))
            .map(|rest| format!("{rest}_FILE").to_ascii_lowercase())
            .collect()
    }

    fn well_known(&self, var: &str) -> Result<Option<figment::value::Value>, ConfigError> {
        let Some((_, path)) = self.vars.iter().find(|(v, _)| v == var) else { return Ok(None) };
        read_secret(var, path).map(Some)
    }

    /// `RUSTCMS_A__B_FILE` → (`a.b`, value).
    fn prefixed(&self) -> Result<Vec<(String, figment::value::Value)>, ConfigError> {
        let mut out = Vec::new();
        for (var, path) in &self.vars {
            let Some(rest) = var.strip_prefix("RUSTCMS_") else { continue };
            let key = rest.split("__").map(str::to_ascii_lowercase).collect::<Vec<_>>().join(".");
            out.push((key, read_secret(var, path)?));
        }
        Ok(out)
    }
}

fn config_error(message: String) -> ConfigError {
    ConfigError(Box::new(figment::Error::from(message)))
}

/// The value in `path` for `var`: never quoted in an error.
fn read_secret(var: &str, path: &Path) -> Result<figment::value::Value, ConfigError> {
    if std::env::var_os(var).is_some() {
        return Err(config_error(format!("both {var} and {var}_FILE are set: keep one")));
    }
    let shown = path.display();
    let len = std::fs::metadata(path).map_err(|e| config_error(format!("{var}_FILE {shown}: {}", e.kind())))?.len();
    if len > MAX_SECRET_FILE {
        return Err(config_error(format!("{var}_FILE {shown} is larger than {MAX_SECRET_FILE} bytes")));
    }
    let bytes = std::fs::read(path).map_err(|e| config_error(format!("{var}_FILE {shown}: {}", e.kind())))?;
    let text = String::from_utf8(bytes).map_err(|_| config_error(format!("{var}_FILE {shown} is not UTF-8")))?;
    let text = text.strip_suffix('\n').map(|t| t.strip_suffix('\r').unwrap_or(t)).unwrap_or(&text);
    if text.is_empty() {
        return Err(config_error(format!("{var}_FILE {shown} is empty")));
    }
    // Parsed as the environment is (`true`, numbers, …), except that a value
    // with surrounding whitespace stays that exact string (the parser trims).
    if text.trim() != text {
        return Ok(text.into());
    }
    Ok(text.parse::<figment::value::Value>().unwrap_or_else(|_| text.into()))
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
        let files = SecretFiles::from_env()?;
        for (var, path) in well_known {
            figment = figment.merge(Env::raw().only(&[var]).map(move |_| path.into()));
            if let Some(value) = files.well_known(var)? {
                figment = figment.merge(Serialized::default(path, value));
            }
        }
        // `_FILE` variables are not settings themselves.
        let names = files.names();
        figment = figment.merge(
            Env::prefixed("RUSTCMS_")
                .filter(move |key| !names.iter().any(|n| key.as_str().eq_ignore_ascii_case(n)))
                .split("__"),
        );
        for (path, value) in files.prefixed()? {
            figment = figment.merge(Serialized::default(&path, value));
        }
        Ok(figment)
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

    fn load() -> Result<Config, ConfigError> {
        Config::from_figment(Config::figment(Path::new("config"))?)
    }

    #[test]
    fn secrets_come_from_files_in_their_variables_place() {
        Jail::expect_with(|jail| {
            let key = AppKey::generate().encode();
            jail.create_file("app_key", &format!("{key}\n"))?;
            jail.create_file("db_url", "sqlite::memory:\r\n")?;
            jail.create_file("timeout", "7")?;
            jail.create_file("password", " spaced secret \n")?;
            jail.set_env("APP_KEY_FILE", "app_key");
            jail.set_env("DATABASE_URL_FILE", "db_url");
            jail.set_env("RUSTCMS_HTTP__TIMEOUT_SECS_FILE", "timeout");
            jail.set_env("RUSTCMS_MAIL__SMTP__PASSWORD_FILE", "password");
            let config = load().unwrap();
            assert_eq!(config.app.key.encode(), key, "one trailing line break removed");
            assert_eq!(config.database.url, "sqlite::memory:");
            assert_eq!(config.http.timeout_secs, 7, "parsed as the environment is");
            #[derive(Deserialize)]
            struct Smtp {
                password: String,
            }
            #[derive(Deserialize)]
            struct Mail {
                smtp: Smtp,
            }
            assert_eq!(
                config.section::<Mail>("mail").unwrap().smtp.password,
                " spaced secret ",
                "other whitespace kept"
            );
            // RUSTCMS_* still wins over a well-known variable's file.
            jail.set_env("RUSTCMS_DATABASE__URL", "sqlite://other.db");
            assert_eq!(load().unwrap().database.url, "sqlite://other.db");
            Ok(())
        });
    }

    /// `vars` (name, file or value) set over a valid base; the load fails with
    /// `needle`, never quoting the secret.
    fn fails_with(files: &[(&str, &[u8])], vars: &[(&str, &str)], needle: &str) {
        Jail::expect_with(|jail| {
            jail.set_env("APP_KEY", AppKey::generate().encode());
            for (name, contents) in files {
                jail.create_binary(name, contents)?;
            }
            for (name, value) in vars {
                jail.set_env(name, value);
            }
            let err = load().unwrap_err().to_string();
            assert!(err.contains(needle), "{err}");
            assert!(!err.contains("hunter2"), "content leaked: {err}");
            Ok(())
        });
    }

    #[test]
    fn secret_files_fail_loudly_without_showing_their_content() {
        let secret: (&str, &[u8]) = ("secret", b"hunter2");
        fails_with(
            &[secret],
            &[("DATABASE_URL", "sqlite::memory:"), ("DATABASE_URL_FILE", "secret")],
            "both DATABASE_URL and DATABASE_URL_FILE",
        );
        let sqlite = ("DATABASE_URL", "sqlite::memory:");
        fails_with(&[], &[sqlite, ("RUSTCMS_MAIL__PASSWORD_FILE", "missing")], "RUSTCMS_MAIL__PASSWORD_FILE missing");
        fails_with(&[("empty", b"\n")], &[sqlite, ("RUSTCMS_MAIL__PASSWORD_FILE", "empty")], "is empty");
        fails_with(
            &[("binary", &[0xff, 0xfe, 0x00])],
            &[sqlite, ("RUSTCMS_MAIL__PASSWORD_FILE", "binary")],
            "not UTF-8",
        );
        let big = vec![b'x'; 64 * 1024 + 1];
        fails_with(&[("big", &big)], &[sqlite, ("RUSTCMS_MAIL__PASSWORD_FILE", "big")], "larger than");
        fails_with(
            &[secret],
            &[sqlite, ("RUSTCMS_MAIL__PASSWORD_FILE", "secret"), ("RUSTCMS_MAIL__PASSWORD", "inline")],
            "both RUSTCMS_MAIL__PASSWORD and RUSTCMS_MAIL__PASSWORD_FILE",
        );
    }

    #[test]
    fn a_key_named_file_is_an_ordinary_setting() {
        Jail::expect_with(|jail| {
            jail.set_env("APP_KEY", AppKey::generate().encode());
            jail.set_env("DATABASE_URL", "sqlite::memory:");
            jail.set_env("RUSTCMS_LOG__FILE", "/var/log/app.log");
            let config = load().unwrap();
            #[derive(Deserialize)]
            struct Log {
                file: String,
            }
            assert_eq!(config.section::<Log>("log").unwrap().file, "/var/log/app.log");
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
