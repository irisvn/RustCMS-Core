//! Writing: transactions, row locks, soft delete markers and the response
//! after commit.

use super::*;

impl<R: Resource> ResourceAdapter<R> {
    /// Envelope of a record whose write has committed. Building it must not
    /// turn the committed write into an error (a retry would repeat it): on
    /// failure, the key alone is returned, with no data.
    pub(super) async fn committed_envelope(&self, ctx: &Ctx, model: Model<R>) -> Envelope {
        self.committed_key_envelope(ctx, self.key_of(&model)).await
    }

    /// [`Self::committed_envelope`] by key (replays of committed creates).
    pub(super) async fn committed_key_envelope(&self, ctx: &Ctx, key: String) -> Envelope {
        // Bounded: a slow lookup must not let the HTTP timeout (or a client
        // giving up) hide the committed key and invite a duplicating retry.
        // Within the request's own deadline too (minus a margin to write the response).
        let budget = rustcms_core::http::request_deadline().map_or(POST_COMMIT_HYDRATION, |deadline| {
            deadline
                .saturating_duration_since(tokio::time::Instant::now())
                .saturating_sub(RESPONSE_MARGIN)
                .min(POST_COMMIT_HYDRATION)
        });
        let key_only = |key: String| Envelope {
            key,
            title: None,
            abilities: BTreeMap::new(),
            data: Data::new(),
            labels: BTreeMap::new(),
            trashed: false,
        };
        // Re-read in scope: a write may move the record out of the user's scope
        // (e.g. a new owner), and then the response shows nothing of it.
        let hydrate = async {
            match self.find_model(ctx, &ctx.db, &key).await {
                Ok(model) => Ok(self.envelopes(ctx, vec![model]).await?.remove(0)),
                Err(AppError::NotFound) => Ok(key_only(key.clone())),
                Err(err) => Err(err),
            }
        };
        let hydrated = tokio::time::timeout(budget, hydrate).await;
        match hydrated.unwrap_or_else(|_| Err(AppError::internal("post-commit hydration timed out"))) {
            Ok(envelope) => envelope,
            Err(err) => {
                tracing::warn!(resource = R::SLUG, %key, %err, "write committed; response without record data");
                key_only(key)
            }
        }
    }

    /// Opens a write transaction and a context whose [`Ctx::conn`] is that
    /// transaction, for policies and hooks.
    pub(super) async fn begin(&self, ctx: &Ctx) -> AppResult<(Arc<sea_orm::DatabaseTransaction>, Ctx)> {
        // A busy database (SQLite) or a lock wait timing out: nothing written.
        let txn = rustcms_core::db::begin_write(&ctx.db).await.map_err(crate::idempotency::conflict)?;
        if ctx.idempotency_key.is_some() {
            crate::idempotency::bound_claim_wait(&txn).await?;
        }
        let txn = Arc::new(txn);
        let wctx = ctx.within(txn.clone());
        Ok((txn, wctx))
    }

    /// Ends a write opened with [`Self::begin`] without committing it.
    pub(super) async fn abort(txn: Arc<sea_orm::DatabaseTransaction>, wctx: Ctx) -> AppResult<()> {
        drop(wctx);
        let txn = Arc::try_unwrap(txn)
            .map_err(|_| AppError::internal(format!("`{}`: the write transaction is still in use", R::SLUG)))?;
        txn.rollback().await?;
        Ok(())
    }

    /// Commits a write opened with [`Self::begin`].
    pub(super) async fn commit(txn: Arc<sea_orm::DatabaseTransaction>, wctx: Ctx) -> AppResult<()> {
        drop(wctx);
        let txn = Arc::try_unwrap(txn)
            .map_err(|_| AppError::internal(format!("`{}`: the write transaction is still in use", R::SLUG)))?;
        txn.commit().await?;
        Ok(())
    }

    /// The record `key` for a write with `ability`: found in scope and allowed
    /// by the policy *before* locking (hidden or forbidden records are never
    /// locked, and answer at once), then locked and checked again, so ownership
    /// or tenancy changed meanwhile is seen.
    pub(super) async fn find_authorized_locked(
        &self,
        ctx: &Ctx,
        txn: &sea_orm::DatabaseTransaction,
        key: &str,
        ability: &str,
        trashed: Trashed,
    ) -> AppResult<Model<R>> {
        let model = self.find_model_in(ctx, txn, key, trashed).await?;
        if !self.policy.allows(ctx, ability, Some(&Self::record(&model))).await {
            return Err(AppError::Forbidden);
        }
        if self.lock_rows(txn, std::slice::from_ref(&self.key_of(&model)), trashed).await?.is_empty() {
            return Err(AppError::NotFound);
        }
        let model = self.find_model_in(ctx, txn, key, trashed).await?;
        if !self.policy.allows(ctx, ability, Some(&Self::record(&model))).await {
            return Err(AppError::Forbidden);
        }
        Ok(model)
    }

    /// Locks the rows of `keys` until commit, by primary key only (scope joins or
    /// `DISTINCT` do not interfere). Callers pass only keys already found in
    /// scope and authorized, then re-check after locking. SQLite write
    /// transactions already hold the database lock.
    ///
    /// Returns the keys actually locked. A row deleted while waiting is skipped
    /// by PostgreSQL, so callers must use only these keys: a replacement row
    /// inserted under the same key afterwards is not locked.
    pub(super) async fn lock_rows(
        &self,
        txn: &sea_orm::DatabaseTransaction,
        keys: &[String],
        trashed: Trashed,
    ) -> AppResult<HashSet<String>> {
        let backend = txn.get_database_backend();
        if backend == DbBackend::Sqlite {
            return Ok(keys.iter().cloned().collect());
        }
        let values = self.decode_keys(keys)?;
        if values.is_empty() {
            return Ok(HashSet::new());
        }
        // Key order: concurrent writes to overlapping records lock them in the
        // same order. A row trashed (or restored) meanwhile is not locked.
        let rows = |select: sea_orm::Select<Entity<R>>| match (&self.soft, trashed) {
            (Some((column, _, _)), Trashed::Without) => select.filter(column.is_null()),
            (Some((column, _, _)), Trashed::Only) => select.filter(column.is_not_null()),
            _ => select,
        };
        let ordered = Entity::<R>::find().filter(self.pk.is_in(values)).order_by(self.pk, Order::Asc);
        if backend != DbBackend::MySql {
            let locked = rows(ordered).lock_exclusive().all(txn).await?;
            return Ok(locked.iter().map(|m| self.key_of(m)).collect());
        }
        // InnoDB locks rows as it scans, not in the sorted order: one point lock
        // per key, in the order a plain read returns them.
        let mut locked = HashSet::new();
        for model in rows(ordered).all(txn).await? {
            let key = model.get(self.pk);
            if let Some(model) = rows(Entity::<R>::find().filter(self.pk.eq(key))).lock_exclusive().one(txn).await? {
                locked.insert(self.key_of(&model));
            }
        }
        Ok(locked)
    }

    /// Sets the soft delete marker to now, or clears it.
    pub(super) fn set_marker(
        &self,
        active: &mut <Entity<R> as EntityTrait>::ActiveModel,
        trash: bool,
    ) -> AppResult<()> {
        let Some((column, name, kind)) = &self.soft else { return Ok(()) };
        let candidates = if trash {
            let now = chrono::Utc::now();
            let text = match kind {
                FieldKind::LocalDateTime => now.naive_utc().format("%Y-%m-%dT%H:%M:%S%.6f").to_string(),
                _ => now.to_rfc3339_opts(chrono::SecondsFormat::Micros, true),
            };
            kind.decode(&serde_json::Value::String(text)).map_err(|e| AppError::internal(e.0))?
        } else {
            kind.nulls()
        };
        if candidates.into_iter().any(|v| active.try_set(*column, v).is_ok()) {
            Ok(())
        } else {
            Err(AppError::internal(format!("`{}`: cannot write the soft delete column `{name}`", R::SLUG)))
        }
    }
}
