//! `media:prune` and the worker: queued deletions, abandoned uploads, temp files.

use super::*;

/// Uploads older than this without a media row are abandoned.
pub(super) const ABANDONED_AFTER_MINUTES: i64 = 60;

/// Removes queued objects and abandoned uploads; returns how many objects were deleted.
pub async fn prune(db: &DatabaseConnection, storage: &dyn Storage) -> AppResult<u64> {
    let mut deleted = 0;
    let due = deletions::Entity::find()
        .filter(deletions::Column::NextAt.lte(Utc::now()))
        .filter(deletions::Column::Disk.eq(storage.disk()))
        .order_by_asc(deletions::Column::NextAt)
        .limit(100)
        .all(db)
        .await?;
    for job in due {
        let result = match StorageKey::parse(&job.key) {
            Ok(key) => storage.delete(&key).await.map_err(|e| e.to_string()),
            // Not a key this framework writes: nothing to delete safely.
            Err(err) => Err(err.to_string()),
        };
        match result {
            Ok(()) => {
                deletions::Entity::delete_by_id(job.id).exec(db).await?;
                deleted += 1;
            }
            Err(err) => {
                tracing::warn!(key = %job.key, %err, "media object deletion failed; will retry");
                let backoff = Duration::minutes(i64::from(job.attempts.clamp(0, 10) + 1).pow(2));
                deletions::Entity::update_many()
                    .col_expr(deletions::Column::Attempts, Expr::value(job.attempts.saturating_add(1)))
                    .col_expr(deletions::Column::NextAt, Expr::value(Utc::now() + backoff))
                    .filter(deletions::Column::Id.eq(job.id))
                    .exec(db)
                    .await?;
            }
        }
    }
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
    // Staged and partial files of interrupted uploads.
    deleted += storage.sweep(std::time::Duration::from_secs(ABANDONED_AFTER_MINUTES as u64 * 60)).await?;
    Ok(deleted)
}
