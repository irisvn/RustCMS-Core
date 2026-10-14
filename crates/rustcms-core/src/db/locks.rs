//! Named transaction locks (advisory on PostgreSQL, a `rustcms_locks` row on
//! MySQL, the write lock on SQLite).

use super::*;

/// Number of lock slots per hashed namespace (see [`xact_lock`]).
pub const LOCK_SLOTS: u64 = 1024;

/// Serializes transactions on `name` until this one ends.
///
/// - PostgreSQL: a transaction-scoped advisory lock on a hash of `name`.
/// - MySQL: the row `name` of the preseeded `rustcms_locks` table, locked with
///   `FOR UPDATE` (names are `rbac`, `lineage:<slot>`, `idem:<slot>` with
///   slots below [`LOCK_SLOTS`]; see [`lock_slot`]).
/// - SQLite: nothing; [`begin_write`] already holds the database write lock.
///
/// Lock order: a transaction takes at most one such lock, as its first
/// statement, before any row lock. A missing row is an error, never a silent
/// skip.
pub async fn xact_lock(txn: &DatabaseTransaction, name: &str) -> Result<(), DbErr> {
    match txn.get_database_backend() {
        DbBackend::Postgres => {
            let key = i64::from_str_radix(&format!("{:016x}", fnv1a(name.as_bytes()))[..15], 16).unwrap_or_default();
            txn.execute_unprepared(&format!("SELECT pg_advisory_xact_lock({key})")).await?;
        }
        DbBackend::MySql => {
            let locked = txn
                .query_one_raw(Statement::from_sql_and_values(
                    DbBackend::MySql,
                    "SELECT name FROM rustcms_locks WHERE name = ? FOR UPDATE",
                    [name.into()],
                ))
                .await?;
            if locked.is_none() {
                return Err(DbErr::Custom(format!("lock `{name}` is missing from rustcms_locks (run the migrations)")));
            }
        }
        _ => {}
    }
    Ok(())
}

/// The lock name for `value` in a hashed namespace (`lineage`, `idem`): one of
/// [`LOCK_SLOTS`] rows, so the lock table stays bounded. Different values may
/// share a slot; they then wait for each other briefly.
pub fn lock_slot(namespace: &str, value: &str) -> String {
    format!("{namespace}:{}", fnv1a(value.as_bytes()) % LOCK_SLOTS)
}

/// FNV-1a: stable across runs and platforms.
pub(super) fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3))
}
