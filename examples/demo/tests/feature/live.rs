//! The live page's routes (`AdminRoutes`): its stream and its reset need
//! `demo.live` (super admins only in the demo). The stream itself is covered
//! end to end by `admin-ui/e2e/realtime.spec.ts`.

use demo::bootstrap;
use demo::database::migrations::Migrator;
use demo::database::seeders::user_seeder::PASSWORD;
use rustcms::axum::http::{Method, StatusCode};
use rustcms_testing::admin_routes::{Case, assert_access};
use rustcms_testing::{TestApp, TestDb, db_test};

use crate::support::seed;

db_test!(only_allowed_users_reach_the_live_routes, migrate = Migrator, |db: TestDb| async move {
    let app = TestApp::new(bootstrap::build, db).await;
    seed(app.state()).await;
    let visitor = app.client();
    visitor.fetch_csrf().await;
    let editor = app.login_as("editor@example.test", PASSWORD).await;
    let admin = app.login_as("admin@example.test", PASSWORD).await;
    assert_access(
        &app,
        &demo::routes::admin::routes(),
        &[
            // Allowed streams never end, so only refusals are checked here.
            Case::new(Method::GET, "/demo/ticks").expect("visitor", &visitor, StatusCode::UNAUTHORIZED).expect(
                "editor",
                &editor,
                StatusCode::FORBIDDEN,
            ),
            Case::new(Method::POST, "/demo/ticks/reset")
                .expect("visitor", &visitor, StatusCode::UNAUTHORIZED)
                .expect("editor", &editor, StatusCode::FORBIDDEN)
                .expect("admin", &admin, StatusCode::NO_CONTENT),
        ],
    )
    .await;
});
