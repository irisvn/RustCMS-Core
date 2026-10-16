//! Checks the hand-written authorization oracle (`fixtures/authz/auth.toml`)
//! against the running application, and that it covers every auth route.

mod support;

use std::collections::BTreeSet;

use axum::http::{Method, StatusCode};
use rustcms_migration::Migrator;
use rustcms_testing::{TestApp, TestClient, TestDb, db_test};
use serde::Deserialize;
use support::Seed;

#[derive(Deserialize)]
struct Oracle {
    case: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    route: String,
    actor: String,
    status: u16,
    body: Option<String>,
}

fn oracle() -> Oracle {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/authz/auth.toml");
    toml::from_str(&std::fs::read_to_string(path).expect("oracle file")).expect("oracle syntax")
}

/// Each case gets fresh clients: tokens revoked or sessions ended by one case do not leak into the next.
async fn actor<'a>(app: &'a TestApp, seed: &Seed, name: &str) -> TestClient<'a> {
    match name {
        "guest" => {
            let client = app.client();
            client.fetch_csrf().await;
            client
        }
        "member" => app.acting_as(&seed.member).await,
        "foreign" => app.acting_as(&seed.member).await.origin(Some("https://evil.example")),
        "reader" => app.with_token(&seed.reader, &["posts.view"]).await,
        other => panic!("unknown actor `{other}` in oracle"),
    }
}

#[test]
fn oracle_covers_every_auth_route() {
    let covered: BTreeSet<String> = oracle().case.into_iter().map(|c| c.route).collect();
    for (method, path) in rustcms_auth::routes::routes("/admin") {
        let route = format!("{method} {path}");
        assert!(covered.contains(&route), "route `{route}` has no case in fixtures/authz/auth.toml");
    }
}

db_test!(oracle_matches_the_application, migrate = Migrator, |db: TestDb| async move {
    // Password resets on (their routes answer 404 otherwise).
    let (app, seed) = support::setup_with(support::build_with_mail, db, &[]).await;
    let mut failures = Vec::new();
    for case in oracle().case {
        let (method, path) = case.route.split_once(' ').expect("`METHOD /path`");
        let client = actor(&app, &seed, &case.actor).await;
        let body = case.body.as_deref().map(|b| serde_json::from_str(b).expect("case body is JSON"));
        let response = client.send(method.parse::<Method>().unwrap(), path, body.as_ref()).await;
        if response.status != StatusCode::from_u16(case.status).unwrap() {
            failures
                .push(format!("{} as {}: expected {}, got {}", case.route, case.actor, case.status, response.status));
        }
    }
    assert!(failures.is_empty(), "oracle mismatches:\n{}", failures.join("\n"));
});
