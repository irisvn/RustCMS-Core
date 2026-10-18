//! Exports (ADR 0021): the list as shown, as rows of readable fields. The
//! keys are chosen first (in the list's order, bounded, policy checked per
//! record), then loaded in chunks and checked again.

use super::*;

impl<R: Resource> ResourceAdapter<R> {
    /// Refuses an export the user may not make at all (403), or of a resource
    /// that is not exportable (404).
    pub(super) async fn admit_export(&self, ctx: &Ctx) -> AppResult<()> {
        if !self.meta.exportable {
            return Err(AppError::NotFound);
        }
        if !self.policy.allows(ctx, ability::EXPORT, None).await
            || !self.policy.allows(ctx, ability::VIEW_ANY, None).await
        {
            return Err(AppError::Forbidden);
        }
        Ok(())
    }

    /// The fields exported for the user: readable ones, in declaration order,
    /// with how their cells are written.
    pub(super) fn export_columns(&self, ctx: &Ctx) -> Vec<(String, crate::exports::CellKind)> {
        use crate::exports::CellKind;
        self.fields
            .iter()
            .filter(|f| f.can_read(ctx) && f.secret.is_none())
            .map(|f| {
                let kind = match self.column(&f.name).kind {
                    FieldKind::String => CellKind::Text,
                    FieldKind::Json => CellKind::Json,
                    _ => CellKind::Plain,
                };
                (f.name.clone(), kind)
            })
            .collect()
    }

    /// `query` checked as a list query and in canonical form: typed filter
    /// values (query-string text decoded by field type, `in` lists as arrays),
    /// no page.
    pub(super) async fn canonical_query(&self, ctx: &Ctx, query: &ListQuery) -> AppResult<ListQuery> {
        let mut query = query.clone();
        query.page = None;
        query.per_page = None;
        self.admit_list(ctx, &query).await?;
        self.list_select(ctx, &query)?;
        if query.from_query_string {
            for filter in &mut query.filters {
                let kind = self.column(&filter.field).kind;
                filter.value = match (&filter.op, &filter.value) {
                    (FilterOp::In, serde_json::Value::String(list)) => serde_json::Value::Array(
                        list.split(',')
                            .map(|v| kind.from_query(&serde_json::Value::String(v.trim().to_owned())))
                            .collect(),
                    ),
                    (_, value) => kind.from_query(value),
                };
            }
            query.from_query_string = false;
        }
        Ok(query)
    }

    /// Keys of the records the list `query` shows the user, in its order; at
    /// most `max` (more: 422, before anything is written).
    pub(super) async fn export_keys(&self, ctx: &Ctx, query: &ListQuery, max: usize) -> AppResult<Vec<String>> {
        self.admit_export(ctx).await?;
        self.admit_list(ctx, query).await?;
        let select = self.list_select(ctx, query)?;
        let too_many = || -> AppError {
            let mut errors = ValidationErrors::new();
            errors.add_form(format!("More than {max} records to export; narrow the search or filters."));
            errors.into()
        };
        if !self.policy.view_depends_on_record() && query.trashed == Trashed::Without {
            if !self.policy.allows(ctx, ability::VIEW, None).await {
                return Ok(Vec::new());
            }
            let batches = (max as u64).div_ceil(LIST_SCAN_BATCH).max(1);
            let (mut keys, complete) = self.read_candidate_keys(&ctx.db, &select, LIST_SCAN_BATCH, batches).await?;
            if !complete && keys.len() >= max {
                return Err(too_many());
            }
            keys.truncate(max);
            return Ok(keys);
        }
        // Record-dependent visibility: as the list, every candidate checked.
        let (keys, complete) =
            self.read_candidate_keys(&ctx.db, &select, LIST_SCAN_BATCH, MAX_LIST_SCAN / LIST_SCAN_BATCH).await?;
        if !complete {
            let mut errors = ValidationErrors::new();
            errors.add_form("Too many matching records to check individually; narrow the search or filters.");
            return Err(errors.into());
        }
        let mut visible = Vec::new();
        for chunk in keys.chunks(LOAD_CHUNK) {
            for (model, record) in self.load_ordered(ctx, &select, chunk).await? {
                if self.listable(ctx, &record).await {
                    if visible.len() >= max {
                        return Err(too_many());
                    }
                    visible.push(self.key_of(&model));
                }
            }
        }
        Ok(visible)
    }

    /// The rows of `keys` (in that order) the user may still see through
    /// `query`, with the values of `columns` only. Records gone, out of the
    /// query or no longer viewable are skipped.
    pub(super) async fn export_rows(
        &self,
        ctx: &Ctx,
        query: &ListQuery,
        keys: &[String],
        columns: &[String],
    ) -> AppResult<Vec<(String, Data)>> {
        let select = self.list_select(ctx, query)?;
        let mut out = Vec::with_capacity(keys.len());
        for chunk in keys.chunks(LOAD_CHUNK) {
            for (model, record) in self.load_ordered(ctx, &select, chunk).await? {
                if self.listable(ctx, &record).await {
                    let data =
                        columns.iter().map(|c| (c.clone(), record.get(c).cloned().unwrap_or_default())).collect();
                    out.push((self.key_of(&model), data));
                }
            }
        }
        Ok(out)
    }
}
