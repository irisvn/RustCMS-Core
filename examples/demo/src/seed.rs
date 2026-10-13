//! `demo:seed`: sample users, roles and content for trying the admin panel.

use rustcms::auth::entities::role;
use rustcms::auth::{ADMIN_GUARD, Actor, AuthState, NewUser, RbacService};
use rustcms::models::user;
use rustcms::{AppCommand, AppState};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};

use crate::models::{category, post};

/// Password of the seeded accounts (demo only).
pub const PASSWORD: &str = "password123";

pub struct Seed;

#[async_trait::async_trait]
impl AppCommand for Seed {
    fn definition(&self) -> clap::Command {
        clap::Command::new("demo:seed")
            .about("Create demo accounts (admin@example.test, editor@example.test) and content")
    }

    async fn run(&self, state: &AppState, _args: &clap::ArgMatches) -> anyhow::Result<()> {
        // Well-known passwords and a super admin: never in production.
        if state.config().app.env == rustcms::config::Environment::Production {
            anyhow::bail!("demo:seed creates accounts with a public password; refusing in production");
        }
        seed(state).await?;
        println!("Seeded. Sign in at /admin as admin@example.test or editor@example.test (password: {PASSWORD}).");
        Ok(())
    }
}

/// Every step finds its rows by natural key before creating them, so a run
/// interrupted anywhere is completed by running it again.
pub async fn seed(state: &AppState) -> anyhow::Result<()> {
    let auth = state.service::<AuthState>().expect("AuthProvider");
    let db = state.db();
    let rbac = RbacService::new(db, &auth.resolver);
    rbac.sync_permissions(&auth.permissions.all()).await?;

    let user = async |name: &str, email: &str| -> anyhow::Result<user::Model> {
        if let Some(found) = user::Entity::find().filter(user::Column::Email.eq(email)).one(db).await? {
            return Ok(found);
        }
        let new = NewUser {
            name,
            email,
            password_hash: auth.passwords.hash(PASSWORD.to_owned()).await?,
            super_admin_guard: None,
        };
        Ok(rbac.create_user(Actor::System, new).await?)
    };
    let admin = user("Ada Admin", "admin@example.test").await?;
    let super_admin = role::super_admin_id(db, ADMIN_GUARD).await?.expect("super_admin role (migrations)");
    rbac.assign_role(Actor::System, admin.id, super_admin).await?;
    // Super admin of the API guard too: replacing other users' credentials needs every guard.
    let api_super = role::super_admin_id(db, rustcms::auth::API_GUARD).await?.expect("api super_admin role");
    rbac.assign_role(Actor::System, admin.id, api_super).await?;
    let editor = user("Eddie Editor", "editor@example.test").await?;
    let permissions: Vec<String> = [
        "posts.create",
        "posts.update_own",
        "categories.view_any",
        "categories.view",
        "media.view_any",
        "media.view",
        "media.create",
    ]
    .map(String::from)
    .to_vec();
    let editor_role = match role::Entity::find()
        .filter(role::Column::Name.eq("editor"))
        .filter(role::Column::GuardName.eq(ADMIN_GUARD))
        .one(db)
        .await?
    {
        Some(found) => found,
        None => rbac.create_role(Actor::System, ADMIN_GUARD, "editor", &permissions).await?,
    };
    rbac.assign_role(Actor::System, editor.id, editor_role.id).await?;

    let mut categories = Vec::new();
    for (name, slug) in [("News", "news"), ("Guides", "guides"), ("Releases", "releases")] {
        let found = category::Entity::find().filter(category::Column::Slug.eq(slug)).one(db).await?;
        let model = match found {
            Some(found) => found,
            None => {
                category::ActiveModel { name: Set(name.into()), slug: Set(slug.into()), ..Default::default() }
                    .insert(db)
                    .await?
            }
        };
        categories.push(model.id);
    }
    let now = chrono::Utc::now();
    for i in 1..=30 {
        let title = format!("Post number {i}");
        if post::Entity::find().filter(post::Column::Title.eq(&title)).one(db).await?.is_some() {
            continue;
        }
        post::ActiveModel {
            title: Set(title),
            body: Set(Some(format!("Body of post {i}."))),
            category_id: Set(Some(categories[i % categories.len()])),
            author_id: Set(Some(if i % 3 == 0 { editor.id } else { admin.id })),
            published: Set(i % 2 == 0),
            published_at: Set((i % 2 == 0).then(|| now - chrono::Duration::days(i as i64))),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(db)
        .await?;
    }
    Ok(())
}
