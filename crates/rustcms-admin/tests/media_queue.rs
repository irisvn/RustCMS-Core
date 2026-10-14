//! The media deletion queue with two runners at once (two instances): each
//! job is processed once, and failed attempts are each counted once. All databases.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use axum::body::Body;
use rustcms_core::storage::{Storage, StorageError, StorageKey};
use rustcms_testing::{TestDb, db_test};
use sea_orm::ConnectionTrait;
use sea_orm::sea_query::{Alias, ExprTrait, Query};

/// Counts deletions per key; each takes a moment, so runs overlap.
#[derive(Default)]
struct Counting {
    deleted: Mutex<HashMap<String, u32>>,
    /// Every deletion fails (after counting the attempt).
    fail: bool,
}

#[async_trait::async_trait]
impl Storage for Counting {
    fn disk(&self) -> &'static str {
        "local"
    }

    fn staging_dir(&self) -> PathBuf {
        std::env::temp_dir()
    }

    async fn put_file(&self, _: &StorageKey, _: &Path, _: &str) -> Result<(), StorageError> {
        Ok(())
    }

    async fn open(&self, _: &StorageKey) -> Result<(Body, u64), StorageError> {
        Err(StorageError::NotFound)
    }

    async fn delete(&self, key: &StorageKey) -> Result<(), StorageError> {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        *self.deleted.lock().unwrap().entry(key.as_str().to_owned()).or_default() += 1;
        if self.fail { Err(StorageError::Backend("down".into())) } else { Ok(()) }
    }

    async fn exists(&self, _: &StorageKey) -> Result<bool, StorageError> {
        Ok(false)
    }
}

/// Queues 12 due deletions.
async fn queue(db: &TestDb) {
    for _ in 0..12 {
        let key = StorageKey::generate("png").unwrap();
        let insert = Query::insert()
            .into_table(Alias::new("media_deletions"))
            .columns([
                Alias::new("id"),
                Alias::new("disk"),
                Alias::new("key"),
                Alias::new("attempts"),
                Alias::new("next_at"),
            ])
            .values_panic([
                uuid::Uuid::now_v7().into(),
                "local".into(),
                key.as_str().into(),
                0.into(),
                (chrono::Utc::now() - chrono::Duration::seconds(1)).into(),
            ])
            .to_owned();
        db.conn().execute(&insert).await.unwrap();
    }
}

db_test!(two_runners_take_each_job_once, migrate = rustcms_migration::Migrator, |db: TestDb| async move {
    queue(&db).await;
    let other = db.reopen().await;
    let storage = Counting::default();
    let (a, b) = tokio::join!(
        rustcms_admin::media::prune(db.conn(), &storage),
        rustcms_admin::media::prune(other.conn(), &storage)
    );
    assert_eq!(a.unwrap() + b.unwrap(), 12);
    let deleted = storage.deleted.lock().unwrap().clone();
    assert_eq!(deleted.len(), 12);
    assert!(deleted.values().all(|n| *n == 1), "a job ran twice: {deleted:?}");
    let left = db
        .conn()
        .query_one(
            &Query::select()
                .expr(sea_orm::sea_query::Expr::cust("COUNT(*)"))
                .from(Alias::new("media_deletions"))
                .to_owned(),
        )
        .await
        .unwrap()
        .unwrap()
        .try_get_by_index::<i64>(0)
        .unwrap();
    assert_eq!(left, 0);
    other.cleanup().await;
});

db_test!(failed_attempts_are_counted_once, migrate = rustcms_migration::Migrator, |db: TestDb| async move {
    queue(&db).await;
    let other = db.reopen().await;
    let storage = Counting { fail: true, ..Default::default() };
    let (a, b) = tokio::join!(
        rustcms_admin::media::prune(db.conn(), &storage),
        rustcms_admin::media::prune(other.conn(), &storage)
    );
    assert_eq!(a.unwrap() + b.unwrap(), 0);
    assert!(storage.deleted.lock().unwrap().values().all(|n| *n == 1));
    // Each job failed once, was counted once and waits for its retry.
    let rows = db
        .conn()
        .query_all(
            &Query::select()
                .column(Alias::new("attempts"))
                .from(Alias::new("media_deletions"))
                .and_where(sea_orm::sea_query::Expr::col(Alias::new("next_at")).gt(chrono::Utc::now()))
                .to_owned(),
        )
        .await
        .unwrap();
    assert_eq!(rows.len(), 12);
    assert!(rows.iter().all(|r| r.try_get_by_index::<i32>(0).unwrap() == 1));
    other.cleanup().await;
});
