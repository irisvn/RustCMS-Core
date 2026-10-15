//! Demo accounts: an admin (super admin) and an editor (own posts only).

use rustcms::auth::entities::role;
use rustcms::auth::{ADMIN_GUARD, API_GUARD, Actor, AuthState, NewUser, RbacService};
use rustcms::database::Seeder;
use rustcms::models::user;
use rustcms::{AppState, async_trait};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

/// Password of the seeded accounts (demo only).
pub const PASSWORD: &str = "password123";

/// What editors may do.
const EDITOR: &[&str] = &[
    "posts.create",
    "posts.update_own",
    "categories.view_any",
    "categories.view",
    "tags.view_any",
    "tags.view",
    "media.view_any",
    "media.view",
    "media.create",
];

pub struct UserSeeder;

#[async_trait]
impl Seeder for UserSeeder {
    fn name(&self) -> &'static str {
        "user"
    }

    async fn run(&self, state: &AppState) -> anyhow::Result<()> {
        refuse_in_production(state)?;
        let auth = state.service::<AuthState>().expect("AuthProvider");
        let db = state.db();
        let rbac = RbacService::new(db, &auth.resolver);
        rbac.sync_permissions(&auth.permissions.all()).await?;
        let admin = account(state, "Ada Admin", "admin@example.test").await?;
        // Super admin of both guards: replacing other users' credentials needs every guard.
        for guard in [ADMIN_GUARD, API_GUARD] {
            let super_admin = role::super_admin_id(db, guard).await?.expect("super_admin role (migrations)");
            rbac.assign_role(Actor::System, admin.id, super_admin).await?;
        }
        let editor = account(state, "Eddie Editor", "editor@example.test").await?;
        let found = role::Entity::find()
            .filter(role::Column::Name.eq("editor"))
            .filter(role::Column::GuardName.eq(ADMIN_GUARD))
            .one(db)
            .await?;
        let editor_role = match found {
            Some(found) => found,
            None => {
                let permissions: Vec<String> = EDITOR.iter().map(|p| (*p).to_owned()).collect();
                rbac.create_role(Actor::System, ADMIN_GUARD, "editor", &permissions).await?
            }
        };
        rbac.assign_role(Actor::System, editor.id, editor_role.id).await?;
        Ok(())
    }
}

/// Well-known passwords and a super admin: never in production, whatever
/// `db:seed --force` says.
pub fn refuse_in_production(state: &AppState) -> anyhow::Result<()> {
    if state.config().app.env == rustcms::config::Environment::Production {
        anyhow::bail!("the demo seeders create accounts with a public password; refusing in production");
    }
    Ok(())
}

/// The user with `email`, created (password [`PASSWORD`]) if missing.
pub async fn account(state: &AppState, name: &str, email: &str) -> anyhow::Result<user::Model> {
    refuse_in_production(state)?;
    let db = state.db();
    if let Some(found) = user::Entity::find().filter(user::Column::Email.eq(email)).one(db).await? {
        return Ok(found);
    }
    let auth = state.service::<AuthState>().expect("AuthProvider");
    let password_hash = auth.passwords.hash(PASSWORD.to_owned()).await?;
    let new = NewUser { name, email, password_hash, super_admin_guard: None };
    Ok(RbacService::new(db, &auth.resolver).create_user(Actor::System, new).await?)
}
