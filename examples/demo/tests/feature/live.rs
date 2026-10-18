//! The live page's routes (`AdminRoutes`): its stream, its reset and its
//! echo socket need `demo.live` (super admins only in the demo). The page
//! itself is covered end to end by `admin-ui/e2e/realtime.spec.ts`.

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
            // An admitted stream is checked by its status, then dropped.
            Case::new(Method::GET, "/demo/ticks")
                .expect("visitor", &visitor, StatusCode::UNAUTHORIZED)
                .expect("editor", &editor, StatusCode::FORBIDDEN)
                .expect("admin", &admin, StatusCode::OK),
            // A socket: in-process requests carry no upgrade, so one past its
            // access is refused by the handshake (400); see tests of the socket itself.
            Case::new(Method::GET, "/demo/echo")
                .expect("visitor", &visitor, StatusCode::UNAUTHORIZED)
                .expect("editor", &editor, StatusCode::FORBIDDEN)
                .expect("admin", &admin, StatusCode::BAD_REQUEST),
            Case::new(Method::POST, "/demo/ticks/reset")
                .expect("visitor", &visitor, StatusCode::UNAUTHORIZED)
                .expect("editor", &editor, StatusCode::FORBIDDEN)
                .expect("admin", &admin, StatusCode::NO_CONTENT),
        ],
    )
    .await;
});

db_test!(the_echo_socket_answers_and_authorizes_its_command, migrate = Migrator, |db: TestDb| async move {
    let app = TestApp::new(bootstrap::build, db).await;
    seed(app.state()).await;
    let admin = app.login_as("admin@example.test", PASSWORD).await;
    let mut socket = admin.socket("/admin/api/x/demo/echo").await.expect("admitted");
    let wait = std::time::Duration::from_secs(2);
    socket.send_text("hello").await;
    assert_eq!(socket.next_data(wait).await.map(|m| m.to_string()), Some("hello".to_owned()));
    socket.send_text("whoami").await;
    let me = socket.next_data(wait).await.map(|m| m.to_string()).unwrap_or_default();
    assert!(me.starts_with("admin:"), "{me}");
    // Without the permission there is no socket.
    let editor = app.login_as("editor@example.test", PASSWORD).await;
    assert_eq!(editor.socket("/admin/api/x/demo/echo").await.err().map(|e| e.status), Some(StatusCode::FORBIDDEN));
});
