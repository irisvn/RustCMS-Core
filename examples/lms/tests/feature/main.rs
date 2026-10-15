//! Feature tests of the LMS on SQLite, PostgreSQL and MySQL (`db_test!`):
//! seats, instructors, the welcome job, seeding and languages.

#[path = "../support/mod.rs"]
mod support;

mod customizing;
mod enrollments;
mod instructors;
mod languages;
mod seeding;
// rustcms:tests
