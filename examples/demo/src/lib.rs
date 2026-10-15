//! demo: a rustcms application. Where things live:
//! `bootstrap` assembles the app from `providers`; `routes` declares the API,
//! served by `http`; `models` and `policies` hold the domain; `admin` the
//! panel; `settings` what admins change at run time; `database` migrations
//! and seeders.

pub mod admin;
pub mod bootstrap;
pub mod database;
pub mod http;
pub mod models;
pub mod policies;
pub mod providers;
pub mod routes;
pub mod settings;
// rustcms:mods
