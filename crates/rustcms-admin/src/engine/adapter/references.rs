//! Checks run on what is written, after hooks: `belongs_to` targets and
//! media (also inside rich text) must exist and be visible to the user.

use super::*;

impl<R: Resource> ResourceAdapter<R> {
    /// The final check of what a write stores (after hooks): related records
    /// and media must be visible to the user, media must match its field.
    pub(super) async fn check_written(&self, ctx: &Ctx, txn: &DatabaseTransaction, data: &Data) -> AppResult<()> {
        self.check_relations(ctx, txn, data).await?;
        self.check_media(ctx, txn, data).await
    }

    /// Media references: well-formed, existing, not trashed, visible to the
    /// user, and of an accepted type and size. The referenced rows are locked
    /// (shared, in id order) so they cannot be trashed meanwhile.
    pub(super) async fn check_media(&self, ctx: &Ctx, txn: &DatabaseTransaction, data: &Data) -> AppResult<()> {
        use crate::media::entity as media;
        let mut errors = ValidationErrors::new();
        let images_only = crate::field::MediaRule {
            multiple: true,
            max_items: usize::MAX,
            accept: vec!["image/*".to_owned()],
            max_size: None,
        };
        let mut wanted: Vec<(&Field, &crate::field::MediaRule, Vec<uuid::Uuid>)> = Vec::new();
        for field in self.fields.iter().filter(|f| f.rich_text) {
            let Some(value) = data.get(&field.name).filter(|v| !v.is_null()) else { continue };
            match crate::rich_text::parse(value) {
                Ok(doc) => {
                    let ids = crate::rich_text::media_ids(&doc);
                    if !ids.is_empty() {
                        wanted.push((field, &images_only, ids));
                    }
                }
                Err(e) => errors.add(field.name.as_str(), e.0),
            }
        }
        for field in &self.fields {
            let (Some(rule), Some(value)) = (&field.media, data.get(&field.name)) else { continue };
            let texts: Vec<Option<&str>> = match value {
                serde_json::Value::Null => continue,
                serde_json::Value::String(id) if !rule.multiple => vec![Some(id.as_str())],
                serde_json::Value::Array(items) if rule.multiple => items.iter().map(|v| v.as_str()).collect(),
                _ => {
                    errors.add(field.name.as_str(), "is invalid");
                    continue;
                }
            };
            let ids: Option<Vec<uuid::Uuid>> =
                texts.iter().map(|t| t.and_then(|t| uuid::Uuid::parse_str(t).ok())).collect();
            match ids {
                Some(ids) if ids.len() > rule.max_items => {
                    errors.add(field.name.as_str(), format!("may not have more than {} items", rule.max_items));
                }
                Some(ids) if ids.iter().collect::<HashSet<_>>().len() != ids.len() => {
                    errors.add(field.name.as_str(), "may not list the same file twice");
                }
                Some(ids) => wanted.push((field, rule, ids)),
                None => errors.add(field.name.as_str(), "is invalid"),
            }
        }
        errors.into_result()?;
        if wanted.is_empty() {
            return Ok(());
        }
        let library = ctx.registry.get(crate::media::SLUG).ok_or_else(|| {
            AppError::internal(format!("`{}`: media fields need the media library (`AdminProvider::media`)", R::SLUG))
        })?;
        let mut all: Vec<uuid::Uuid> = wanted.iter().flat_map(|(_, _, ids)| ids.iter().copied()).collect();
        all.sort_unstable();
        all.dedup();
        // Visibility first: records the user cannot see are never locked.
        let keys: Vec<String> = all.iter().map(uuid::Uuid::to_string).collect();
        let visible = library.keys_in_scope(ctx, txn, &keys).await?;
        let lockable: Vec<uuid::Uuid> = all.iter().copied().filter(|id| visible.contains(&id.to_string())).collect();
        // Re-read under the lock: trashed meanwhile means unavailable.
        let rows: HashMap<uuid::Uuid, media::Model> = match txn.get_database_backend() {
            // InnoDB locks while scanning: one point lock per id, in id order (the
            // byte order of `binary(16)` and of `Uuid`).
            DbBackend::MySql => {
                let mut rows = HashMap::new();
                for id in lockable {
                    if let Some(m) = media::Entity::find_by_id(id).lock_shared().one(txn).await? {
                        rows.insert(m.id, m);
                    }
                }
                rows
            }
            backend => {
                let mut select = media::Entity::find()
                    .filter(media::Column::Id.is_in(lockable))
                    .order_by(media::Column::Id, Order::Asc);
                if backend == DbBackend::Postgres {
                    select = select.lock_shared();
                }
                select.all(txn).await?.into_iter().map(|m| (m.id, m)).collect()
            }
        };
        let mut errors = ValidationErrors::new();
        for (field, rule, ids) in wanted {
            for id in ids {
                let usable = rows.get(&id).filter(|m| m.deleted_at.is_none() && visible.contains(&id.to_string()));
                let message = match usable {
                    None => Some("refers to a file that is not available".to_owned()),
                    Some(m) if !rule.accepts(&m.mime) => Some(format!("does not accept {} files", m.mime)),
                    Some(m) if rule.max_size.is_some_and(|max| u64::try_from(m.size).unwrap_or(u64::MAX) > max) => {
                        Some("refers to a file that is too large".to_owned())
                    }
                    Some(_) => None,
                };
                if let Some(message) = message {
                    errors.add(field.name.as_str(), message);
                    break;
                }
            }
        }
        errors.into_result()
    }

    /// Client-supplied foreign keys must point to records the user can see.
    pub(super) async fn check_relations(&self, ctx: &Ctx, txn: &DatabaseTransaction, input: &Data) -> AppResult<()> {
        let mut errors = ValidationErrors::new();
        for field in &self.fields {
            let (Some(target), Some(value)) = (field.belongs_to, input.get(&field.name)) else {
                continue;
            };
            if value.is_null() {
                continue;
            }
            let key = value.as_str().map_or_else(|| value.to_string(), str::to_owned);
            let target =
                ctx.registry.get(target).ok_or_else(|| AppError::internal(format!("unknown resource `{target}`")))?;
            if !target.keys_in_scope(ctx, txn, std::slice::from_ref(&key)).await?.contains(&key) {
                errors.add(field.name.as_str(), "is invalid");
            }
        }
        errors.into_result()
    }
}
