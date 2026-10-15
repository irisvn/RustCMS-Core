//! Testing helpers for rustcms applications, in the spirit of `Illuminate\Testing`.
//!
//! - [`TestDb`]: an isolated database per test (SQLite temp file or PostgreSQL database),
//! - [`db_test!`]: runs a test body on both databases,
//! - [`TestApp`]: drives the full router (middleware included) without a socket,
//! - [`TestClient`]: cookies, CSRF and bearer tokens (`acting_as`, `login_as`, `with_token`),
//! - [`Factory`]: builds and inserts records.
#![warn(missing_docs)]

mod app;
mod client;
mod db;
mod factory;

pub use app::{TestApp, TestResponse, test_config};
pub use client::TestClient;
pub use db::{Backend, TestDb, create_table};

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
    eprintln!("skipped: {variable} is not set");
}
pub use factory::{Factory, UserFactory};

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
/// when `RUSTCMS_TEST_MYSQL` does; otherwise that variant prints a skip notice. With `migrate = M`, the database
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
                let db = ($open)($crate::Backend::Sqlite).await.expect("sqlite is always available");
                let cleanup = db.clone();
                ($body)(db).await;
                cleanup.cleanup().await;
            }

            #[tokio::test]
            async fn postgres() {
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
