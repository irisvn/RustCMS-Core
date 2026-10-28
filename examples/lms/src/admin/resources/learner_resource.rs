//! Learners: people taking courses (no sign-in; not users).

use rustcms::admin::*;
use sea_orm::{QueryFilter, Select};

use crate::models::learner;
use crate::policies::instructor;

pub struct LearnerResource;

impl Resource for LearnerResource {
    type Entity = learner::Entity;
    const SLUG: &'static str = "learners";

    // #region meta
    fn meta() -> ResourceMeta {
        // Imported learners are matched by email: known ones are updated.
        ResourceMeta::new("Learner")
            .title("name")
            .icon("users")
            .group("Learning")
            .unique("learners_email_key", &["email"], "is already registered")
            .import_key("email")
    }
    // #endregion meta

    fn fields() -> Vec<Field> {
        vec![
            Field::column(learner::Column::Name).required().searchable().sortable().max_length(120),
            Field::column(learner::Column::Email).required().searchable().max_length(254),
            Field::column(learner::Column::Phone).max_length(32),
        ]
    }

    fn form() -> Form {
        Form::new().schema(components![
            TextInput::make("name"),
            TextInput::make("email").email(),
            TextInput::make("phone"),
        ])
    }

    fn relations() -> Vec<Relation> {
        vec![Relation::has_many("enrollments", "enrollments", "learner_id")]
    }

    /// Instructors see the learners of their courses.
    fn scope(query: Select<learner::Entity>, ctx: &Ctx) -> Select<learner::Entity> {
        match instructor(ctx) {
            None => query,
            Some(me) => query.filter(learner::taught_by(me, (learner::Entity, learner::Column::Id))),
        }
    }
}
