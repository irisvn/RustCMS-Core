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
