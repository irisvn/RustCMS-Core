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
        if ctx.import_row.is_some() {
            // A row of an import (a job, long after the request): the user
            // as they are now, held so until commit (no role change, no
            // revocation meanwhile); then the import is allowed again.
            let (txn, current) = rustcms_auth::rbac::begin_as(&ctx.db, &ctx.principal).await?;
            let txn = Arc::new(txn);
            let mut wctx = ctx.within(txn.clone());
            wctx.principal = current;
            if let Err(err) = self.admit_import(&wctx).await {
                Self::abort(txn, wctx).await?;
                return Err(err);
            }
            return Ok((txn, wctx));
        }
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
        Ok(self.find_authorized_locked_moving(ctx, txn, key, ability, trashed, None).await?.0)
    }

    /// [`Self::find_authorized_locked`], locking the record's parent first
    /// ([`ResourceMeta::lock_parent`]), and the parent `input` moves it to.
    /// Returns the record and the parent keys locked.
    pub(super) async fn find_authorized_locked_moving(
        &self,
        ctx: &Ctx,
        txn: &sea_orm::DatabaseTransaction,
        key: &str,
        ability: &str,
        trashed: Trashed,
        input: Option<&Data>,
    ) -> AppResult<(Model<R>, BTreeSet<String>)> {
        let model = self.find_model_in(ctx, txn, key, trashed).await?;
        if !self.policy.allows(ctx, ability, Some(&self.record(&model))).await {
            return Err(AppError::Forbidden);
        }
        let read_parent = self.parent_of_model(&model);
        let moved_to = input.and_then(|input| self.parent_of_data(input)).flatten();
        let parents = self.lock_parents(ctx, txn, read_parent.iter().chain(moved_to.iter()).cloned()).await?;
        if self.lock_rows(txn, std::slice::from_ref(&self.key_of(&model)), trashed).await?.is_empty() {
            return Err(AppError::NotFound);
        }
        let model = self.find_model_in(ctx, txn, key, trashed).await?;
        // Moved to another parent before the lock: that parent is not held.
        if self.parent_of_model(&model) != read_parent {
            return Err(parent_moved());
        }
        if !self.policy.allows(ctx, ability, Some(&self.record(&model))).await {
            return Err(AppError::Forbidden);
        }
        Ok((model, parents))
    }

    /// The parent key of `model` ([`ResourceMeta::lock_parent`]), if any.
    pub(super) fn parent_of_model(&self, model: &Model<R>) -> Option<String> {
        let (field, _) = self.meta.parent_lock.as_ref()?;
        key_text(self.record(model).get(field)?)
    }

    /// The parent key `data` sets: `None` when it does not set the field,
    /// `Some(None)` when it clears it.
    pub(super) fn parent_of_data(&self, data: &Data) -> Option<Option<String>> {
        let (field, _) = self.meta.parent_lock.as_ref()?;
        data.get(field).map(key_text)
    }

    /// Locks the parent rows `keys` (canonical, deduplicated, in key order),
    /// before any record of this resource. Returns the keys locked.
    pub(super) async fn lock_parents(
        &self,
        ctx: &Ctx,
        txn: &sea_orm::DatabaseTransaction,
        keys: impl IntoIterator<Item = String>,
    ) -> AppResult<BTreeSet<String>> {
        let Some((_, target)) = &self.meta.parent_lock else {
            return Ok(BTreeSet::new());
        };
        let parent = ctx
            .registry
            .get(target)
            .ok_or_else(|| AppError::internal(format!("`{}`: no resource `{target}`", R::SLUG)))?;
        // Keys that are not valid keys of the parent lock nothing (the
        // reference check reports them).
        let keys: BTreeSet<String> = keys.into_iter().filter_map(|k| parent.canonical_key(&k)).collect();
        if !keys.is_empty() {
            parent.lock_keys(txn, &keys.iter().cloned().collect::<Vec<_>>()).await?;
        }
        Ok(keys)
    }

    /// After the hooks: the parent written must be one locked before.
    pub(super) fn check_parent_written(&self, ctx: &Ctx, data: &Data, locked: &BTreeSet<String>) -> AppResult<()> {
        match self.parent_of_data(data) {
            Some(Some(parent)) => self.check_parent_locked(ctx, &parent, locked),
            _ => Ok(()),
        }
    }

    /// As stored, after the write (column defaults included): the record's
    /// parent is one locked before, or none.
    pub(super) fn check_parent_stored(&self, ctx: &Ctx, model: &Model<R>, locked: &BTreeSet<String>) -> AppResult<()> {
        match self.parent_of_model(model) {
            Some(parent) => self.check_parent_locked(ctx, &parent, locked),
            None => Ok(()),
        }
    }

    /// `parent` (any spelling: `01`, an upper-case UUID) is among `locked`
    /// (canonical keys).
    fn check_parent_locked(&self, ctx: &Ctx, parent: &str, locked: &BTreeSet<String>) -> AppResult<()> {
        let Some((field, target)) = &self.meta.parent_lock else {
            return Ok(());
        };
        match ctx.registry.get(target).and_then(|p| p.canonical_key(parent)) {
            Some(key) if locked.contains(&key) => return Ok(()),
            // Not a valid key: the reference check reports it.
            None => return Ok(()),
            Some(_) => {}
        }
        Err(AppError::internal(format!(
            "`{}`: `{field}` was set to a parent that was not locked (by a hook, an action or a column default)",
            R::SLUG
        )))
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

/// A key as written on the wire (`"12"`, `12` or a UUID string); null → none.
pub(super) fn key_text(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(text) => Some(text.clone()),
        serde_json::Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

/// The record moved to another parent between reading and locking it.
pub(super) fn parent_moved() -> AppError {
    AppError::Conflict {
        code: "write_conflict",
        reason: "busy",
        message: "Another change to the same data is in progress. Nothing was saved; try again.",
    }
}
