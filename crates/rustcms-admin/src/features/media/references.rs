//! Media references written by resources and settings: well-formed, existing,
//! not trashed, visible to the writer, of an accepted type and size.

use std::collections::{HashMap, HashSet};

use sea_orm::{DbBackend, Order};

use super::*;
use crate::field::MediaRule;

/// The ids `value` holds for a media field with `rule` (one id, or a list for
/// `multiple`); `None` after adding the problem to `errors`, or for `null`.
pub(crate) fn reference_ids(
    field: &str,
    rule: &MediaRule,
    value: &serde_json::Value,
    errors: &mut ValidationErrors,
) -> Option<Vec<Uuid>> {
    let texts: Vec<Option<&str>> = match value {
        serde_json::Value::Null => return None,
        serde_json::Value::String(id) if !rule.multiple => vec![Some(id.as_str())],
        serde_json::Value::Array(items) if rule.multiple => items.iter().map(|v| v.as_str()).collect(),
        _ => {
            errors.add(field, "is invalid");
            return None;
        }
    };
    let ids: Option<Vec<Uuid>> = texts.iter().map(|t| t.and_then(|t| Uuid::parse_str(t).ok())).collect();
    match ids {
        Some(ids) if ids.len() > rule.max_items => {
            errors.add(field, format!("may not have more than {} items", rule.max_items));
            None
        }
        Some(ids) if ids.iter().collect::<HashSet<_>>().len() != ids.len() => {
            errors.add(field, "may not list the same file twice");
            None
        }
        Some(ids) if ids.is_empty() => None,
        Some(ids) => Some(ids),
        None => {
            errors.add(field, "is invalid");
            None
        }
    }
}

/// Checks each field's referenced media: existing, not trashed, visible to
/// the user (scope and policy of the media resource, as in `ctx`), and of an
/// accepted type and size. The rows are locked (shared, in id order) in
/// `txn` so they cannot be trashed meanwhile; records the user cannot see are
/// never locked.
pub(crate) async fn check_references(
    ctx: &Ctx,
    txn: &DatabaseTransaction,
    wanted: Vec<(&str, &MediaRule, Vec<Uuid>)>,
) -> AppResult<()> {
    if wanted.is_empty() {
        return Ok(());
    }
    let library = ctx.registry.get(SLUG).ok_or_else(|| AppError::internal("the media library is not enabled"))?;
    let mut all: Vec<Uuid> = wanted.iter().flat_map(|(_, _, ids)| ids.iter().copied()).collect();
    all.sort_unstable();
    all.dedup();
    // Visibility first: records the user cannot see are never locked.
    let keys: Vec<String> = all.iter().map(Uuid::to_string).collect();
    let visible = library.keys_in_scope(ctx, txn, &keys).await?;
    let lockable: Vec<Uuid> = all.iter().copied().filter(|id| visible.contains(&id.to_string())).collect();
    // Re-read under the lock: trashed meanwhile means unavailable.
    let rows: HashMap<Uuid, entity::Model> = match txn.get_database_backend() {
        // InnoDB locks while scanning: one point lock per id, in id order (the
        // byte order of `binary(16)` and of `Uuid`).
        DbBackend::MySql => {
            let mut rows = HashMap::new();
            for id in lockable {
                if let Some(m) = entity::Entity::find_by_id(id).lock_shared().one(txn).await? {
                    rows.insert(m.id, m);
                }
            }
            rows
        }
        backend => {
            let mut select = entity::Entity::find()
                .filter(entity::Column::Id.is_in(lockable))
                .order_by(entity::Column::Id, Order::Asc);
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
                errors.add(field, message);
                break;
            }
        }
    }
    errors.into_result()
}
