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
    "dashboard.learners",
    "dashboard.new_enrollments",
    "dashboard.enrollments_per_week",
    "dashboard.new_courses",
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
    "learners.import",
    "enrollments.view_any",
    "enrollments.view",
    "enrollments.create",
    "enrollments.update",
    "enrollments.delete",
    "enrollments.export",
    UPDATE_PROGRESS,
    "settings.lms.view",
    "settings.lms.update",
    "dashboard.learners",
    "dashboard.new_enrollments",
    "dashboard.enrollments_per_week",
    "dashboard.new_courses",
];

/// Editors: the news (articles, categories, tags, their images), nothing of the courses.
pub const EDITOR: &[&str] = &[
    "news.view_any",
    "news.view",
    "news.create",
    "news.update",
    "news.delete",
    "news.attach_tags",
    "news.detach_tags",
    "categories.view_any",
    "categories.view",
    "categories.create",
    "categories.update",
    "categories.delete",
    "tags.view_any",
    "tags.view",
    "tags.create",
    "tags.update",
    "tags.delete",
    "media.view_any",
    "media.view",
    "media.create",
    // Every image of the news, not only one's own uploads (viewing and
    // referencing them; trashing another user's file needs `media.manage_all`).
    "media.view_all",
];

/// Viewers: read everything of the school, export enrollments, the dashboard; change nothing.
pub const VIEWER: &[&str] = &[
    ALL_COURSES,
    "courses.view_any",
    "courses.view",
    "lessons.view_any",
    "lessons.view",
    "learners.view_any",
    "learners.view",
    "enrollments.view_any",
    "enrollments.view",
    "enrollments.export",
    "news.view_any",
    "news.view",
    "categories.view_any",
    "categories.view",
    "tags.view_any",
    "tags.view",
    // The covers and images of the news (viewing only).
    "media.view_any",
    "media.view",
    "media.view_all",
    "dashboard.learners",
    "dashboard.new_enrollments",
    "dashboard.enrollments_per_week",
    "dashboard.new_courses",
];

// #region role-seeder
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
        let roles = [("staff", STAFF), ("instructor", INSTRUCTOR), ("editor", EDITOR), ("viewer", VIEWER)];
        for (name, permissions) in roles {
            if find(state, name).await?.is_none() {
                let permissions: Vec<String> = permissions.iter().map(|p| (*p).to_owned()).collect();
                rbac.create_role(Actor::System, ADMIN_GUARD, name, &permissions).await?;
            }
        }
        Ok(())
    }
}
// #endregion role-seeder

/// The admin-guard role `name`.
pub async fn find(state: &AppState, name: &str) -> anyhow::Result<Option<role::Model>> {
    Ok(role::Entity::find()
        .filter(role::Column::Name.eq(name))
        .filter(role::Column::GuardName.eq(ADMIN_GUARD))
        .one(state.db())
        .await?)
}
