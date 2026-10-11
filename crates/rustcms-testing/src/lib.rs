//! Testing helpers for rustcms applications, in the spirit of `Illuminate\Testing`.
//!
//! - [`TestDb`]: an isolated database per test (SQLite temp file or PostgreSQL database),
//! - [`db_test!`]: runs a test body on both databases,
//! - [`TestApp`]: drives the full router (middleware included) without a socket,
//! - [`Factory`]: builds and inserts records.
#![warn(missing_docs)]

mod app;
mod db;
mod factory;

pub use app::{TestApp, TestResponse, test_config};
pub use db::{Backend, TestDb};
pub use factory::{Factory, UserFactory};

/// Declares a database test that runs on SQLite and on PostgreSQL.
///
/// PostgreSQL runs when `RUSTCMS_TEST_PG` holds an admin connection URL;
/// otherwise that variant prints a skip notice. With `migrate = M`, the database
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
                    eprintln!("skipped: RUSTCMS_TEST_PG is not set");
                    return;
                };
                let cleanup = db.clone();
                ($body)(db).await;
                cleanup.cleanup().await;
            }
        }
    };
}
