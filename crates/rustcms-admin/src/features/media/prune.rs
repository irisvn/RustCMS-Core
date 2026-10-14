//! `media:prune` and the worker: queued deletions, abandoned uploads, temp files.

use super::*;

/// Uploads older than this without a media row are abandoned.
pub(super) const ABANDONED_AFTER_MINUTES: i64 = 60;

/// Queued deletions processed per run.
const DELETIONS_PER_RUN: usize = 100;

/// Removes queued objects and abandoned uploads; returns how many objects were deleted.
///
/// On PostgreSQL and MySQL each queued deletion is claimed in its own write
/// transaction (`FOR UPDATE SKIP LOCKED`) and held until its object is deleted
/// and the job acknowledged: two runners (two instances) never take the same
/// job. On SQLite (no row locks) a job is claimed by moving its `next_at`
/// ahead under the write lock, so no lock is held during storage calls.
/// After a crash a job runs again (deleting an object twice is harmless).
pub async fn prune(db: &DatabaseConnection, storage: &dyn Storage) -> AppResult<u64> {
    let mut deleted = if db.get_database_backend() == sea_orm::DbBackend::Sqlite {
        leased_deletions(db, storage).await?
    } else {
        claimed_deletions(db, storage).await?
    };
    deleted += abandoned_uploads(db, storage).await?;
    // Staged and partial files of interrupted uploads.
    deleted += storage.sweep(std::time::Duration::from_secs(ABANDONED_AFTER_MINUTES as u64 * 60)).await?;
    Ok(deleted)
}

/// Deletes the object of `job`; on failure, schedules a retry with backoff.
async fn run_deletion<C: sea_orm::ConnectionTrait>(
    conn: &C,
    storage: &dyn Storage,
    job: deletions::Model,
) -> AppResult<bool> {
    let result = match StorageKey::parse(&job.key) {
        Ok(key) => storage.delete(&key).await.map_err(|e| e.to_string()),
        // Not a key this framework writes: nothing to delete safely.
        Err(err) => Err(err.to_string()),
    };
    match result {
        Ok(()) => {
            deletions::Entity::delete_by_id(job.id).exec(conn).await?;
            Ok(true)
        }
        Err(err) => {
            tracing::warn!(key = %job.key, %err, "media object deletion failed; will retry");
            let backoff = Duration::minutes(i64::from(job.attempts.clamp(0, 10) + 1).pow(2));
            deletions::Entity::update_many()
                .col_expr(deletions::Column::Attempts, Expr::value(job.attempts.saturating_add(1)))
                .col_expr(deletions::Column::NextAt, Expr::value(Utc::now() + backoff))
                .filter(deletions::Column::Id.eq(job.id))
                .exec(conn)
                .await?;
            Ok(false)
        }
    }
}

fn due(storage: &dyn Storage) -> sea_orm::Select<deletions::Entity> {
    deletions::Entity::find()
        .filter(deletions::Column::NextAt.lte(Utc::now()))
        .filter(deletions::Column::Disk.eq(storage.disk()))
        .order_by_asc(deletions::Column::NextAt)
}

async fn claimed_deletions(db: &DatabaseConnection, storage: &dyn Storage) -> AppResult<u64> {
    let mut deleted = 0;
    for _ in 0..DELETIONS_PER_RUN {
        let txn = rustcms_core::db::begin_write(db).await?;
        let job = due(storage)
            .limit(1)
            .lock_with_behavior(sea_orm::sea_query::LockType::Update, sea_orm::sea_query::LockBehavior::SkipLocked)
            .one(&txn)
            .await?;
        let Some(job) = job else {
            txn.commit().await?;
            break;
        };
        if run_deletion(&txn, storage, job).await? {
            deleted += 1;
        }
        txn.commit().await?;
    }
    Ok(deleted)
}

/// How long a SQLite claim keeps a job from other runners.
const CLAIM_MINUTES: i64 = 10;

async fn leased_deletions(db: &DatabaseConnection, storage: &dyn Storage) -> AppResult<u64> {
    let mut deleted = 0;
    for _ in 0..DELETIONS_PER_RUN {
        let txn = rustcms_core::db::begin_write(db).await?;
        let Some(job) = due(storage).limit(1).one(&txn).await? else {
            txn.commit().await?;
            break;
        };
        deletions::Entity::update_many()
            .col_expr(deletions::Column::NextAt, Expr::value(Utc::now() + Duration::minutes(CLAIM_MINUTES)))
            .filter(deletions::Column::Id.eq(job.id))
            .exec(&txn)
            .await?;
        txn.commit().await?;
        // `job.attempts` was read under the write lock: retries count once.
        if run_deletion(db, storage, job).await? {
            deleted += 1;
        }
    }
    Ok(deleted)
}

/// Deletes the objects of uploads that never became media rows.
async fn abandoned_uploads(db: &DatabaseConnection, storage: &dyn Storage) -> AppResult<u64> {
    let mut deleted = 0;
    let abandoned = uploads::Entity::find()
        .filter(
            Condition::all()
                .add(uploads::Column::CreatedAt.lte(Utc::now() - Duration::minutes(ABANDONED_AFTER_MINUTES)))
                .add(uploads::Column::Disk.eq(storage.disk())),
        )
        .limit(100)
        .all(db)
        .await?;
    for intent in abandoned {
        // Reclaim the intent first: an upload finishing now then finds it gone
        // and fails instead of committing a row whose file is being deleted.
        let txn = rustcms_core::db::begin_write(db).await?;
        let reclaimed = uploads::Entity::delete_by_id(intent.id).exec(&txn).await?.rows_affected == 1;
        let used = entity::Entity::find().filter(entity::Column::Key.eq(&intent.key)).one(&txn).await?.is_some();
        txn.commit().await?;
        if reclaimed
            && !used
            && let Ok(key) = StorageKey::parse(&intent.key)
        {
            storage.delete(&key).await?;
            deleted += 1;
        }
    }
    Ok(deleted)
}
