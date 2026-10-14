//! The generic engine behind every resource: registry, request context,
//! wire format, idempotency, and the adapter that runs authorized CRUD.

pub(crate) mod adapter;
pub mod context;
pub mod idempotency;
pub mod registry;
pub(crate) mod validate;
pub mod wire;
