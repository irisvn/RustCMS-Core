//! Testing helpers for rustcms applications, in the spirit of `Illuminate\Testing`.
//!
//! - [`TestDb`]: an isolated database per test (SQLite temp file or PostgreSQL database),
//! - [`db_test!`]: runs a test body on both databases,
//! - [`TestApp`]: drives the full router (middleware included) without a socket,
//! - [`TestClient`]: cookies, CSRF and bearer tokens (`acting_as`, `login_as`, `with_token`),
//! - [`Factory`]: builds and inserts records,
//! - [`admin_routes::assert_access`]: an access table for `AdminRoutes`.
#![warn(missing_docs)]

pub mod admin_routes;
mod app;
mod client;
mod db;
mod factory;
mod socket;

pub use app::{TestApp, TestResponse, test_config};
pub use client::TestClient;
pub use db::{Backend, TestDb, create_table};
pub use socket::{Refused, TestSocket};

/// Reports a backend whose server variable is not set: a notice, or a failure
/// when `RUSTCMS_TEST_REQUIRE` lists it (`postgres,mysql` in CI), so a broken
/// setup never passes as skipped.
#[doc(hidden)]
pub fn skip(backend: Backend) {
    let (name, variable) = match backend {
        Backend::Postgres => ("postgres", "RUSTCMS_TEST_PG"),
        Backend::Mysql => ("mysql", "RUSTCMS_TEST_MYSQL"),
        Backend::Sqlite => ("sqlite", ""),
    };
    let required = std::env::var("RUSTCMS_TEST_REQUIRE").unwrap_or_default();
    assert!(
        !required.split(',').any(|b| b.trim() == name),
        "{variable} is required (RUSTCMS_TEST_REQUIRE) but not set"
    );
    // Chosen explicitly: never an all-skipped green run.
    assert!(
        std::env::var("RUSTCMS_TEST_BACKENDS").is_err(),
        "{variable} is required (RUSTCMS_TEST_BACKENDS selects {name}) but not set"
    );
    eprintln!("skipped: {variable} is not set");
}

/// Whether `db_test!` runs on `backend`: every backend by default, else those
/// of `RUSTCMS_TEST_BACKENDS` (`mysql`, `sqlite,postgres`, …).
///
/// # Panics
/// On an empty or unknown name, or when `RUSTCMS_TEST_REQUIRE` asks for a
/// backend that is not selected.
#[doc(hidden)]
pub fn selected(backend: Backend) -> bool {
    let backends = std::env::var("RUSTCMS_TEST_BACKENDS").ok();
    let required = std::env::var("RUSTCMS_TEST_REQUIRE").unwrap_or_default();
    selection(backends.as_deref(), &required, backend).unwrap_or_else(|e| panic!("{e}"))
}

fn backend_name(backend: Backend) -> &'static str {
    match backend {
        Backend::Sqlite => "sqlite",
        Backend::Postgres => "postgres",
        Backend::Mysql => "mysql",
    }
}

fn selection(backends: Option<&str>, required: &str, backend: Backend) -> Result<bool, String> {
    let Some(backends) = backends else { return Ok(true) };
    let names: Vec<&str> = backends.split(',').map(str::trim).filter(|n| !n.is_empty()).collect();
    if names.is_empty() {
        return Err("RUSTCMS_TEST_BACKENDS is empty: name sqlite, postgres or mysql".into());
    }
    if let Some(unknown) = names.iter().find(|n| !["sqlite", "postgres", "mysql"].contains(n)) {
        return Err(format!("RUSTCMS_TEST_BACKENDS: unknown backend `{unknown}` (sqlite, postgres, mysql)"));
    }
    for wanted in required.split(',').map(str::trim).filter(|n| ["postgres", "mysql", "sqlite"].contains(n)) {
        if !names.contains(&wanted) {
            return Err(format!("RUSTCMS_TEST_REQUIRE asks for {wanted}, which RUSTCMS_TEST_BACKENDS does not select"));
        }
    }
    Ok(names.contains(&backend_name(backend)))
}
pub use factory::UserFactory;
/// Moved to [`rustcms_core::database::Factory`] (seeders use it too); kept here for tests.
pub use rustcms_core::database::Factory;

/// An S3-compatible bucket for tests: `RUSTCMS_TEST_S3=http://host:port/bucket`
/// (credentials from `AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY`).
#[derive(Debug, Clone)]
pub struct TestS3 {
    /// Endpoint URL (`http://localhost:59000`).
    pub endpoint: String,
    /// Bucket name.
    pub bucket: String,
}

impl TestS3 {
    /// The bucket, or `None` (a skip notice) when `RUSTCMS_TEST_S3` is not set;
    /// fails when `RUSTCMS_TEST_REQUIRE` lists `s3`, or when the URL or the
    /// credentials are missing.
    pub fn from_env() -> Option<Self> {
        let Ok(url) = std::env::var("RUSTCMS_TEST_S3") else {
            let required = std::env::var("RUSTCMS_TEST_REQUIRE").unwrap_or_default();
            assert!(
                !required.split(',').any(|b| b.trim() == "s3"),
                "RUSTCMS_TEST_S3 is required (RUSTCMS_TEST_REQUIRE) but not set"
            );
            eprintln!("skipped: RUSTCMS_TEST_S3 is not set");
            return None;
        };
        let (scheme, rest) = url.split_once("://").expect("RUSTCMS_TEST_S3 is http(s)://host:port/bucket");
        let (host, bucket) = rest.split_once('/').expect("RUSTCMS_TEST_S3 names a bucket");
        assert!(!bucket.is_empty() && !bucket.contains('/'), "RUSTCMS_TEST_S3 names one bucket");
        for variable in ["AWS_ACCESS_KEY_ID", "AWS_SECRET_ACCESS_KEY"] {
            assert!(std::env::var(variable).is_ok(), "RUSTCMS_TEST_S3 needs {variable}");
        }
        Some(Self { endpoint: format!("{scheme}://{host}"), bucket: bucket.to_owned() })
    }

    /// `config` with `[storage]` on this bucket.
    pub fn configure(&self, config: rustcms_core::Config) -> rustcms_core::Config {
        config
            .with("storage.disk", "s3")
            .and_then(|c| c.with("storage.s3.bucket", &self.bucket))
            .and_then(|c| c.with("storage.s3.endpoint", &self.endpoint))
            .and_then(|c| c.with("storage.s3.allow_http", self.endpoint.starts_with("http://")))
            .expect("storage configuration")
    }
}

/// Declares a database test that runs on SQLite, PostgreSQL and MySQL.
///
/// PostgreSQL runs when `RUSTCMS_TEST_PG` holds an admin connection URL, MySQL
/// when `RUSTCMS_TEST_MYSQL` does; otherwise that variant prints a skip notice.
/// `RUSTCMS_TEST_BACKENDS=mysql` (any of `sqlite,postgres,mysql`) runs only
/// those, each then required (see [`selected`]). With `migrate = M`, the database
/// is migrated with migrator `M` first.
///
/// ```rust,ignore
/// rustcms_testing::db_test!(creates_users, migrate = Migrator, |db: TestDb| async move {
///     UserFactory::new().create(db.conn()).await;
/// });
/// ```
#[macro_export]
macro_rules! db_test {
    ($name:ident, migrate = $migrator:ty, $body:expr) => {
        $crate::db_test!(@emit $name, |backend| $crate::TestDb::migrated::<$migrator>(backend), $body);
    };
    ($name:ident, $body:expr) => {
        $crate::db_test!(@emit $name, $crate::TestDb::empty, $body);
    };
    (@emit $name:ident, $open:expr, $body:expr) => {
        mod $name {
            #[allow(unused_imports)]
            use super::*;

            #[tokio::test]
            async fn sqlite() {
                if !$crate::selected($crate::Backend::Sqlite) {
                    eprintln!("skipped: not in RUSTCMS_TEST_BACKENDS");
                    return;
                }
                let db = ($open)($crate::Backend::Sqlite).await.expect("sqlite is always available");
                let cleanup = db.clone();
                ($body)(db).await;
                cleanup.cleanup().await;
            }

            #[tokio::test]
            async fn postgres() {
                if !$crate::selected($crate::Backend::Postgres) {
                    eprintln!("skipped: not in RUSTCMS_TEST_BACKENDS");
                    return;
                }
                let Some(db) = ($open)($crate::Backend::Postgres).await else {
                    $crate::skip($crate::Backend::Postgres);
                    return;
                };
                let cleanup = db.clone();
                ($body)(db).await;
                cleanup.cleanup().await;
            }

            #[tokio::test]
            async fn mysql() {
                if !$crate::selected($crate::Backend::Mysql) {
                    eprintln!("skipped: not in RUSTCMS_TEST_BACKENDS");
                    return;
                }
                let Some(db) = ($open)($crate::Backend::Mysql).await else {
                    $crate::skip($crate::Backend::Mysql);
                    return;
                };
                let cleanup = db.clone();
                ($body)(db).await;
                cleanup.cleanup().await;
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backends_are_selected_strictly() {
        assert_eq!(selection(None, "", Backend::Sqlite), Ok(true), "all by default");
        assert_eq!(selection(Some("mysql"), "", Backend::Sqlite), Ok(false));
        assert_eq!(selection(Some("mysql"), "", Backend::Mysql), Ok(true));
        assert_eq!(selection(Some(" sqlite , postgres "), "", Backend::Postgres), Ok(true));
        assert!(selection(Some(""), "", Backend::Mysql).is_err(), "empty");
        assert!(selection(Some("maria"), "", Backend::Mysql).is_err(), "unknown");
        assert!(selection(Some("mysql"), "postgres,redis", Backend::Mysql).is_err(), "required but not selected");
        assert_eq!(selection(Some("mysql"), "mysql,redis,s3", Backend::Mysql), Ok(true), "other services ignored");
    }
}
