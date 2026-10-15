//! The `staff` and `instructor` roles (through `RbacService`).

use rustcms::auth::entities::role;
use rustcms::auth::{ADMIN_GUARD, Actor, AuthState, RbacService};
use rustcms::database::Seeder;
use rustcms::{AppState, async_trait};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

use crate::policies::permissions::{ALL_COURSES, ASSIGN_INSTRUCTOR, UPDATE_PROGRESS};

/// Instructors: their courses read-only, their lessons, their learners' progress.
pub const INSTRUCTOR: &[&str] = &[
    "courses.view_any",
    "courses.view",
    "lessons.view_any",
    "lessons.view",
    "lessons.create",
    "lessons.update",
    "lessons.delete",
    "learners.view_any",
    "learners.view",
    "enrollments.view_any",
    "enrollments.view",
    UPDATE_PROGRESS,
];

/// Staff: every course, its instructor, learners and enrollments.
pub const STAFF: &[&str] = &[
    ALL_COURSES,
    ASSIGN_INSTRUCTOR,
    "courses.view_any",
    "courses.view",
    "courses.create",
    "courses.update",
    "courses.delete",
    "lessons.view_any",
    "lessons.view",
    "lessons.create",
    "lessons.update",
    "lessons.delete",
    "learners.view_any",
    "learners.view",
    "learners.create",
    "learners.update",
    "learners.delete",
    "enrollments.view_any",
    "enrollments.view",
    "enrollments.create",
    "enrollments.update",
    "enrollments.delete",
    UPDATE_PROGRESS,
    "settings.lms.view",
    "settings.lms.update",
];

pub struct RoleSeeder;

#[async_trait]
impl Seeder for RoleSeeder {
    fn name(&self) -> &'static str {
        "role"
    }

    async fn run(&self, state: &AppState) -> anyhow::Result<()> {
        let auth = state.service::<AuthState>().expect("AuthProvider");
        let rbac = RbacService::new(state.db(), &auth.resolver);
        rbac.sync_permissions(&auth.permissions.all()).await?;
        for (name, permissions) in [("staff", STAFF), ("instructor", INSTRUCTOR)] {
            if find(state, name).await?.is_none() {
                let permissions: Vec<String> = permissions.iter().map(|p| (*p).to_owned()).collect();
                rbac.create_role(Actor::System, ADMIN_GUARD, name, &permissions).await?;
            }
        }
        Ok(())
    }
}

/// The admin-guard role `name`.
pub async fn find(state: &AppState, name: &str) -> anyhow::Result<Option<role::Model>> {
    Ok(role::Entity::find()
        .filter(role::Column::Name.eq(name))
        .filter(role::Column::GuardName.eq(ADMIN_GUARD))
        .one(state.db())
        .await?)
}
