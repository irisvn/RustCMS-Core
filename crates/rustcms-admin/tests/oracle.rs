//! Checks the hand-written oracle `fixtures/authz/admin.toml` against the
//! running admin API, and that it covers every admin route.

mod support;

use std::collections::BTreeSet;

use axum::http::{Method, StatusCode};
use rustcms_testing::{TestApp, TestClient, TestDb, db_test};
use serde::Deserialize;
use serde_json::json;
use support::app::{Users, setup};

#[derive(Deserialize)]
struct Oracle {
    case: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    route: String,
    path: String,
    actor: String,
    status: u16,
    body: Option<String>,
}

fn oracle() -> Oracle {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/authz/admin.toml");
    toml::from_str(&std::fs::read_to_string(path).expect("oracle file")).expect("oracle syntax")
}

async fn actor<'a>(app: &'a TestApp, users: &Users, name: &str) -> TestClient<'a> {
    match name {
        "guest" => {
            let client = app.client();
            client.fetch_csrf().await;
            client
        }
        "member" => app.acting_as(&users.member).await,
        "editor" => app.acting_as(&users.editor).await,
        "admin" => app.acting_as(&users.admin).await,
        "foreign" => app.acting_as(&users.editor).await.origin(Some("https://evil.example")),
        "token" => app.with_token(&users.admin, &["*"]).await,
        other => panic!("unknown actor `{other}` in oracle"),
    }
}

#[test]
fn oracle_covers_every_admin_route() {
    let covered: BTreeSet<String> = oracle().case.into_iter().map(|c| c.route).collect();
    for (method, path) in rustcms_admin::http::routes("/admin") {
        let route = format!("{method} {path}");
        assert!(covered.contains(&route), "route `{route}` has no case in fixtures/authz/admin.toml");
    }
}

db_test!(oracle_matches_the_admin_api, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    let post = async |user, title: &str| {
        let response = app
            .acting_as(user)
            .await
            .post_json(&app.admin_api("resources/posts/records"), &json!({"title": title}))
            .await;
        response.json()["data"]["key"].as_str().unwrap().to_owned()
    };
    let own = post(&users.editor, "own").await;
    let spare = post(&users.editor, "spare").await;
    let theirs = post(&users.admin, "theirs").await;
    let fill = |text: &str| text.replace("{own}", &own).replace("{spare}", &spare).replace("{theirs}", &theirs);

    let mut failures = Vec::new();
    for case in oracle().case {
        let client = actor(&app, &users, &case.actor).await;
        let (method, _) = case.route.split_once(' ').expect("`METHOD /path`");
        let body = case.body.as_deref().map(|b| serde_json::from_str(&fill(b)).expect("case body is JSON"));
        let response = client.send(method.parse::<Method>().unwrap(), &fill(&case.path), body.as_ref()).await;
        if response.status != StatusCode::from_u16(case.status).unwrap() {
            failures.push(format!(
                "{} {} as {}: expected {}, got {} {}",
                method,
                case.path,
                case.actor,
                case.status,
                response.status,
                String::from_utf8_lossy(&response.body)
            ));
        }
    }
    assert!(failures.is_empty(), "oracle mismatches:\n{}", failures.join("\n"));
});
