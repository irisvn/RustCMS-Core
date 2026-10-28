//! Example accounts: an admin, a staff member, two instructors, an editor and a viewer.

use rustcms::auth::entities::role;
use rustcms::auth::{ADMIN_GUARD, Actor, AuthState, NewUser, RbacService};
use rustcms::database::Seeder;
use rustcms::models::user;
use rustcms::{AppState, async_trait};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

use super::role_seeder;

// #region accounts
/// Password of the seeded accounts (example only).
pub const PASSWORD: &str = "password123";

/// `(name, email, role)`; `None` is the super admin.
pub const ACCOUNTS: [(&str, &str, Option<&str>); 6] = [
    ("Ada Admin", "admin@example.test", None),
    ("Sam Staff", "staff@example.test", Some("staff")),
    ("Tess Teacher", "teacher@example.test", Some("instructor")),
    ("Tom Tutor", "tutor@example.test", Some("instructor")),
    ("Eve Editor", "editor@example.test", Some("editor")),
    ("Vic Viewer", "viewer@example.test", Some("viewer")),
];
// #endregion accounts

pub struct UserSeeder;

#[async_trait]
impl Seeder for UserSeeder {
    fn name(&self) -> &'static str {
        "user"
    }

    async fn run(&self, state: &AppState) -> anyhow::Result<()> {
        // Well-known passwords and a super admin: never in production, whatever
        // `db:seed --force` says.
        if state.config().app.env == rustcms::config::Environment::Production {
            anyhow::bail!("the user seeder creates accounts with a public password; refusing in production");
        }
        let auth = state.service::<AuthState>().expect("AuthProvider");
        let db = state.db();
        let rbac = RbacService::new(db, &auth.resolver);
        for (name, email, role_name) in ACCOUNTS {
            let account = match user::Entity::find().filter(user::Column::Email.eq(email)).one(db).await? {
                Some(found) => found,
                None => {
                    let password_hash = auth.passwords.hash(PASSWORD.to_owned()).await?;
                    let new = NewUser { name, email, password_hash, super_admin_guard: None };
                    rbac.create_user(Actor::System, new).await?
                }
            };
            let role_id = match role_name {
                None => role::super_admin_id(db, ADMIN_GUARD).await?.expect("super_admin role (migrations)"),
                Some(name) => {
                    let found = role_seeder::find(state, name).await?;
                    found.ok_or_else(|| anyhow::anyhow!("role `{name}` missing: run the role seeder"))?.id
                }
            };
            rbac.assign_role(Actor::System, account.id, role_id).await?;
        }
        Ok(())
    }
}

/// The seeded account with `email`.
pub async fn find(state: &AppState, email: &str) -> anyhow::Result<user::Model> {
    let found = user::Entity::find().filter(user::Column::Email.eq(email)).one(state.db()).await?;
    found.ok_or_else(|| anyhow::anyhow!("user `{email}` missing: run the user seeder"))
}
