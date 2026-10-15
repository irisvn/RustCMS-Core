//! The Queue page (System → Queue): who sees it, statuses as the workers
//! see them, and retry / forget / cancel under the workers' own conditions.
//! SQLite, PostgreSQL and MySQL.

use std::time::Duration;

use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use rustcms_auth::{ADMIN_GUARD, Actor, AuthState, RbacService};
use rustcms_core::models::{audit_log, job, job_receipt, user};
use rustcms_core::queue::{Job, JobCtx};
use rustcms_testing::{Factory, TestApp, TestDb, TestResponse, UserFactory, db_test};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, Set};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Its payload holds a value the page must never show.
#[derive(Serialize, Deserialize)]
struct Ping {
    secret: String,
}

#[async_trait::async_trait]
impl Job for Ping {
    const KIND: &'static str = "test.ping";

    async fn handle(self, _: JobCtx) -> anyhow::Result<()> {
        Ok(())
    }
}

fn build(config: rustcms_core::Config) -> rustcms_core::AppBuilder {
    let mut app = rustcms_core::AppBuilder::new(config);
    app.provider(rustcms_auth::AuthProvider::new());
    app.provider(rustcms_admin::AdminProvider::new());
    app.job::<Ping>();
    app
}

struct Users {
    /// `queue.view` only.
    viewer: user::Model,
    /// `queue.manage` only.
    manager: user::Model,
    /// Both, and `users.view_any`.
    operator: user::Model,
}

async fn setup(db: TestDb) -> (TestApp, Users) {
    db.migrate::<rustcms_migration::Migrator>().await;
    let app = TestApp::new(build, db).await;
    let auth = app.state().service::<AuthState>().unwrap();
    let db = app.state().db();
    let rbac = RbacService::new(db, &auth.resolver);
    rbac.sync_permissions(&auth.permissions.all()).await.unwrap();
    let mut users = Vec::new();
    for (name, permissions) in [
        ("viewer", &["queue.view"][..]),
        ("manager", &["queue.manage"]),
        ("operator", &["queue.view", "queue.manage", "users.view_any"]),
    ] {
        let permissions: Vec<String> = permissions.iter().map(|p| (*p).to_owned()).collect();
        let role = rbac.create_role(Actor::System, ADMIN_GUARD, name, &permissions).await.unwrap();
        let user = UserFactory::new().name(name).create(db).await;
        rbac.assign_role(Actor::System, user.id, role.id).await.unwrap();
        users.push(user);
    }
    let operator = users.pop().unwrap();
    let manager = users.pop().unwrap();
    let viewer = users.pop().unwrap();
    (app, Users { viewer, manager, operator })
}

/// A job row in a given state (`at` = now in the database's terms).
struct Row {
    attempts: i32,
    max_attempts: i32,
    available_at: DateTime<Utc>,
    reserved_until: Option<DateTime<Utc>>,
    failed_at: Option<DateTime<Utc>>,
    initiated_by: Option<i64>,
    created_at: DateTime<Utc>,
}

impl Row {
    fn new() -> Self {
        let now = now();
        Self {
            attempts: 0,
            max_attempts: 3,
            available_at: now,
            reserved_until: None,
            failed_at: None,
            initiated_by: None,
            created_at: now,
        }
    }

    async fn insert(self, app: &TestApp) -> uuid::Uuid {
        let id = uuid::Uuid::now_v7();
        job::ActiveModel {
            id: Set(id),
            queue: Set("default".into()),
            kind: Set(Ping::KIND.into()),
            version: Set(1),
            payload: Set(json!({"secret": "never sent"})),
            attempts: Set(self.attempts),
            max_attempts: Set(self.max_attempts),
            available_at: Set(self.available_at),
            reserved_by: Set(self.reserved_until.map(|_| "worker-token".to_owned())),
            reserved_until: Set(self.reserved_until),
            last_error: Set(self.failed_at.map(|_| "failed (see the logs)".to_owned())),
            initiated_by: Set(self.initiated_by),
            created_at: Set(self.created_at),
            failed_at: Set(self.failed_at),
        }
        .insert(app.state().db())
        .await
        .unwrap();
        id
    }
}

fn now() -> DateTime<Utc> {
    let now = Utc::now();
    DateTime::from_timestamp_micros(now.timestamp_micros()).unwrap()
}

fn secs(n: i64) -> chrono::Duration {
    chrono::Duration::seconds(n)
}

fn act(app: &TestApp, id: impl std::fmt::Display, action: &str) -> String {
    app.admin_api(&format!("queue/jobs/{id}/{action}"))
}

fn reason(response: &TestResponse) -> Value {
    response.assert_status(StatusCode::CONFLICT);
    response.json()["reason"].clone()
}

db_test!(the_page_needs_view_and_actions_need_both, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    let failed = Row { failed_at: Some(now()), ..Row::new() }.insert(&app).await;
    let missing = uuid::Uuid::now_v7();

    let viewer = app.acting_as(&users.viewer).await;
    viewer.get(&app.admin_api("queue")).await.assert_status(StatusCode::OK);
    viewer.get(&app.admin_api("queue/jobs")).await.assert_status(StatusCode::OK);
    assert_eq!(viewer.get(&app.admin_api("queue")).await.json()["canManage"], json!(false));
    // Manage only: neither the page nor the actions, whatever the id.
    let manager = app.acting_as(&users.manager).await;
    manager.get(&app.admin_api("queue")).await.assert_status(StatusCode::FORBIDDEN);
    manager.get(&app.admin_api("queue/jobs?status=oops")).await.assert_status(StatusCode::FORBIDDEN);
    for (who, client) in [("viewer", &viewer), ("manager", &manager)] {
        for id in [failed.to_string(), missing.to_string(), "not-a-uuid".to_owned()] {
            client.fetch_csrf().await;
            let response = client.post_json(&act(&app, &id, "retry"), &json!({})).await;
            assert_eq!(response.status, StatusCode::FORBIDDEN, "{who} {id}");
        }
    }
    // Both: missing and malformed ids are alike.
    let operator = app.acting_as(&users.operator).await;
    operator.fetch_csrf().await;
    for id in [missing.to_string(), "not-a-uuid".to_owned()] {
        operator.post_json(&act(&app, &id, "retry"), &json!({})).await.assert_status(StatusCode::NOT_FOUND);
    }
    operator.get(&app.admin_api("queue/jobs?payload=1")).await.assert_validation_error("payload");
    // Losing the permission takes effect at once.
    let auth = app.state().service::<AuthState>().unwrap();
    let rbac = RbacService::new(app.state().db(), &auth.resolver);
    rbac.revoke_permission(Actor::System, users.operator.id, ADMIN_GUARD, "queue.manage").await.ok();
    let role = rustcms_auth::entities::role::Entity::find()
        .filter(rustcms_auth::entities::role::Column::Name.eq("operator"))
        .one(app.state().db())
        .await
        .unwrap()
        .unwrap();
    rbac.revoke_role(Actor::System, users.operator.id, role.id).await.unwrap();
    let operator = app.acting_as(&users.operator).await;
    operator.fetch_csrf().await;
    operator.post_json(&act(&app, failed, "retry"), &json!({})).await.assert_status(StatusCode::FORBIDDEN);
});

db_test!(statuses_are_the_workers_own, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    let now = now();
    let pending = Row::new().insert(&app).await;
    let scheduled = Row { available_at: now + secs(3_600), ..Row::new() }.insert(&app).await;
    let running = Row { attempts: 1, reserved_until: Some(now + secs(60)), ..Row::new() }.insert(&app).await;
    // The lease ended 2 s ago, within the 5 s clock skew: still the worker's.
    let skewed = Row { attempts: 1, reserved_until: Some(now - secs(2)), ..Row::new() }.insert(&app).await;
    // Ended long ago with attempts left: the next worker takes it.
    let reclaimable = Row { attempts: 1, reserved_until: Some(now - secs(60)), ..Row::new() }.insert(&app).await;
    // Ended long ago on its last attempt: failed at the next worker pass.
    let stalled = Row { attempts: 3, reserved_until: Some(now - secs(60)), ..Row::new() }.insert(&app).await;
    let failed =
        Row { attempts: 3, failed_at: Some(now), initiated_by: Some(users.viewer.id), ..Row::new() }.insert(&app).await;

    let operator = app.acting_as(&users.operator).await;
    let overview = operator.get(&app.admin_api("queue")).await.json();
    assert_eq!(
        overview["queues"],
        json!([{"name": "default", "pending": 2, "scheduled": 1, "running": 2, "stalled": 1, "failed": 1}])
    );
    assert_eq!(overview["canManage"], json!(true));
    let statuses = operator.get(&app.admin_api("queue/jobs")).await.json();
    let status_of = |id: uuid::Uuid| {
        statuses["data"].as_array().unwrap().iter().find(|j| j["id"] == json!(id.to_string())).unwrap()["status"]
            .clone()
    };
    for (id, status) in [
        (pending, "pending"),
        (scheduled, "scheduled"),
        (running, "running"),
        (skewed, "running"),
        (reclaimable, "pending"),
        (stalled, "stalled"),
        (failed, "failed"),
    ] {
        assert_eq!(status_of(id), json!(status), "{status}");
    }
    // Filters agree with the statuses.
    let filtered = operator.get(&app.admin_api("queue/jobs?status=running")).await.json();
    assert_eq!(filtered["data"].as_array().unwrap().len(), 2);
    // Never the payload or the worker's token; the dispatcher's name for those who list users.
    let text = statuses.to_string();
    assert!(!text.contains("never sent") && !text.contains("worker-token") && !text.contains("payload"), "{text}");
    let failed_view = statuses["data"].as_array().unwrap().iter().find(|j| j["status"] == "failed").unwrap();
    assert_eq!(failed_view["initiatedBy"], json!({"id": users.viewer.id.to_string(), "name": "viewer"}));
    assert_eq!(failed_view["lastError"], json!("failed (see the logs)"));
    let viewer = app.acting_as(&users.viewer).await;
    let seen = viewer.get(&app.admin_api("queue/jobs?status=failed")).await.json();
    assert_eq!(seen["data"][0]["initiatedBy"], json!({"id": users.viewer.id.to_string()}));
});

db_test!(actions_follow_the_workers_conditions, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    let now = now();
    let operator = app.acting_as(&users.operator).await;
    operator.fetch_csrf().await;
    let db = app.state().db();

    // Retry: a failed job runs again, keeping its id and its receipts.
    let failed = Row { attempts: 3, failed_at: Some(now), ..Row::new() }.insert(&app).await;
    job_receipt::ActiveModel { key: Set(format!("{failed}:effect")), created_at: Set(now) }.insert(db).await.unwrap();
    operator.post_json(&act(&app, failed, "retry"), &json!({})).await.assert_status(StatusCode::NO_CONTENT);
    let row = job::Entity::find_by_id(failed).one(db).await.unwrap().unwrap();
    assert_eq!((row.attempts, row.failed_at, row.reserved_until), (0, None, None));
    assert_eq!(reason(&operator.post_json(&act(&app, failed, "retry"), &json!({})).await), json!("not_failed"));
    assert_eq!(rustcms_core::queue::run_until_idle(app.state(), None, 10).await.unwrap(), 1);
    assert!(job::Entity::find_by_id(failed).one(db).await.unwrap().is_none(), "ran and done");
    assert_eq!(job_receipt::Entity::find().count(db).await.unwrap(), 1, "receipts stay");

    // Forget: failed jobs only.
    let gone = Row { failed_at: Some(now), ..Row::new() }.insert(&app).await;
    let waiting = Row::new().insert(&app).await;
    assert_eq!(reason(&operator.post_json(&act(&app, waiting, "forget"), &json!({})).await), json!("not_failed"));
    operator.post_json(&act(&app, gone, "forget"), &json!({})).await.assert_status(StatusCode::NO_CONTENT);
    assert!(job::Entity::find_by_id(gone).one(db).await.unwrap().is_none());

    // Cancel: jobs waiting to run, never one a worker may still own.
    let running = Row { attempts: 1, reserved_until: Some(now + secs(60)), ..Row::new() }.insert(&app).await;
    let skewed = Row { attempts: 1, reserved_until: Some(now - secs(2)), ..Row::new() }.insert(&app).await;
    let stalled = Row { attempts: 3, reserved_until: Some(now - secs(60)), ..Row::new() }.insert(&app).await;
    let scheduled = Row { available_at: now + secs(3_600), ..Row::new() }.insert(&app).await;
    let failed_again = Row { failed_at: Some(now), ..Row::new() }.insert(&app).await;
    for id in [running, skewed] {
        assert_eq!(reason(&operator.post_json(&act(&app, id, "cancel"), &json!({})).await), json!("running"));
    }
    assert_eq!(reason(&operator.post_json(&act(&app, failed_again, "cancel"), &json!({})).await), json!("not_waiting"));
    for id in [waiting, stalled, scheduled] {
        operator.post_json(&act(&app, id, "cancel"), &json!({})).await.assert_status(StatusCode::NO_CONTENT);
        assert!(job::Entity::find_by_id(id).one(db).await.unwrap().is_none());
    }
    assert!(job::Entity::find_by_id(running).one(db).await.unwrap().is_some(), "the worker keeps it");

    // Claimed again by a new worker after its lease ended: not cancellable meanwhile.
    let reclaimed = Row { attempts: 1, reserved_until: Some(now - secs(60)), ..Row::new() }.insert(&app).await;
    job::Entity::update_many()
        .col_expr(job::Column::ReservedUntil, sea_orm::sea_query::Expr::value(Some(now + secs(60))))
        .col_expr(job::Column::ReservedBy, sea_orm::sea_query::Expr::value(Some("new-token".to_owned())))
        .filter(job::Column::Id.eq(reclaimed))
        .exec(db)
        .await
        .unwrap();
    assert_eq!(reason(&operator.post_json(&act(&app, reclaimed, "cancel"), &json!({})).await), json!("running"));

    // Every action is in the audit log, with the job's kind and queue.
    let events: Vec<String> = audit_log::Entity::find()
        .filter(audit_log::Column::SubjectType.eq("job"))
        .all(db)
        .await
        .unwrap()
        .into_iter()
        .map(|e| {
            assert_eq!(e.actor_user_id, Some(users.operator.id));
            assert_eq!(e.meta.as_ref().unwrap()["kind"], json!(Ping::KIND));
            e.event
        })
        .collect();
    let count = |event: &str| events.iter().filter(|e| *e == event).count();
    assert_eq!((count("queue.retried"), count("queue.forgotten"), count("queue.cancelled")), (1, 1, 3));
});

db_test!(jobs_page_through_ties, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    // Seven jobs dispatched at the same instant, and two a microsecond apart.
    let at = now();
    let mut ids = Vec::new();
    for _ in 0..7 {
        ids.push(Row { created_at: at, ..Row::new() }.insert(&app).await);
    }
    for delta in [1, 2] {
        ids.push(Row { created_at: at + chrono::Duration::microseconds(delta), ..Row::new() }.insert(&app).await);
    }
    let operator = app.acting_as(&users.operator).await;
    let mut seen = Vec::new();
    let mut url = app.admin_api("queue/jobs?per_page=2");
    loop {
        let page = operator.get(&url).await.json();
        seen.extend(page["data"].as_array().unwrap().iter().map(|j| j["id"].as_str().unwrap().to_owned()));
        match page["meta"]["nextCursor"].as_str() {
            Some(cursor) => url = app.admin_api(&format!("queue/jobs?per_page=2&cursor={cursor}")),
            None => break,
        }
        // A job deleted between pages does not disturb the rest.
        if seen.len() == 2 {
            job::Entity::delete_by_id(ids[0]).exec(app.state().db()).await.unwrap();
        }
    }
    let mut unique = seen.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), seen.len(), "no job twice");
    assert!(seen.len() == 9 || seen.len() == 8, "{}", seen.len());
    assert_eq!(seen.first().unwrap(), &ids[8].to_string(), "newest first");
    let _ = Duration::ZERO;
});
