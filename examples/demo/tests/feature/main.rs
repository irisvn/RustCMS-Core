//! Feature tests: the demo through HTTP, on SQLite, PostgreSQL and MySQL
//! (`db_test!`). One test binary; `rustcms make:test <Name>` adds modules.

#[path = "../support/mod.rs"]
mod support;

mod api;
mod content;
mod health;
// rustcms:tests
