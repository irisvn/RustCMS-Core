//! lms: a small learning-management back office on rustcms. Where things live:
//! `bootstrap` assembles the app from `providers`; `models` (with their query
//! scopes), `services` (the enrollment rules) and `policies` hold the domain;
//! `jobs` run in the background; `settings` are changed by admins at run
//! time; `admin` is the panel; `database` holds migrations, seeders and
//! factories.

pub mod admin;
pub mod bootstrap;
pub mod database;
pub mod http;
pub mod jobs;
pub mod models;
pub mod policies;
pub mod providers;
pub mod routes;
pub mod services;
pub mod settings;
// rustcms:mods
