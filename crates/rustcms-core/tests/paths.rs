//! Public paths kept when modules were grouped by role (R1). A compile error
//! here is a broken public path.

#![allow(unused_imports)]

use rustcms_core::db::{
    LOCK_SLOTS, MYSQL_COLLATION, MYSQL_LOCK_WAIT_SECONDS, begin_snapshot, begin_write, check, connect, lock_slot,
    migrate_fresh, ping, set_lock_wait, xact_lock,
};
use rustcms_core::storage::{
    LocalStorage, S3Config, Storage, StorageConfig, StorageError, StorageKey, StorageProvider, connect as storage, of,
};

#[test]
fn paths_compile() {}
