//! Console commands: `make:user`, `token:create`, `permission:sync`, `session:prune`.

use anyhow::{Context, bail};
use async_trait::async_trait;
use clap::{Arg, ArgAction, ArgMatches, Command};
use rustcms_core::audit::AuditContext;
use rustcms_core::models::user;
use rustcms_core::{AppCommand, AppState};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

use crate::rbac::{Actor, NewUser, RbacService};
use crate::secret::random_string;
use crate::state::{ADMIN_GUARD, AuthState};

fn auth(state: &AppState) -> anyhow::Result<std::sync::Arc<AuthState>> {
    state.service::<AuthState>().context("AuthProvider is not registered")
}

fn arg<'a>(args: &'a ArgMatches, name: &str) -> Option<&'a String> {
    args.get_one::<String>(name)
}

/// `make:user`: creates a user, optionally a super admin.
pub struct MakeUser;

#[async_trait]
impl AppCommand for MakeUser {
    fn definition(&self) -> Command {
        Command::new("make:user")
            .about("Create a user (prints a generated password if none is given)")
            .arg(Arg::new("email").long("email").required(true).help("Sign-in email"))
            .arg(Arg::new("name").long("name").required(true).help("Display name"))
            .arg(Arg::new("password").long("password").help("Password (default: generated)"))
            .arg(
                Arg::new("super-admin")
                    .long("super-admin")
                    .action(ArgAction::SetTrue)
                    .help("Grant the super_admin role"),
            )
            .arg(Arg::new("guard").long("guard").default_value(ADMIN_GUARD).help("Guard of the super_admin role"))
    }

    async fn run(&self, state: &AppState, args: &ArgMatches) -> anyhow::Result<()> {
        let auth = auth(state)?;
        let generated = arg(args, "password").is_none();
        let password = arg(args, "password").cloned().unwrap_or_else(|| random_string(20));
        if password.chars().count() < auth.config.password_min_length {
            bail!("password must be at least {} characters", auth.config.password_min_length);
        }
        if password.len() > crate::routes::MAX_PASSWORD_LEN {
            bail!("password may not be longer than {} bytes", crate::routes::MAX_PASSWORD_LEN);
        }
        let new = NewUser {
            name: arg(args, "name").map_or("", String::as_str),
            email: arg(args, "email").map_or("", String::as_str),
            password_hash: auth.passwords.hash(password.clone()).await?,
            super_admin_guard: args
                .get_flag("super-admin")
                .then(|| arg(args, "guard").map_or(ADMIN_GUARD, String::as_str)),
        };
        // User and role assignment commit together: a failure leaves nothing behind.
        let user = RbacService::new(state.db(), &auth.resolver)
            .audited(AuditContext::system(Some("make:user")))
            .create_user(Actor::System, new)
            .await
            .map_err(describe)?;
        println!("Created user #{} <{}>", user.id, user.email);
        if generated {
            println!("Password: {password}");
        }
        Ok(())
    }
}

/// Validation messages as a readable console error.
fn describe(err: rustcms_core::AppError) -> anyhow::Error {
    match err {
        rustcms_core::AppError::Validation(v) => {
            let mut lines: Vec<String> =
                v.errors.iter().map(|(field, messages)| format!("{field}: {}", messages.join(", "))).collect();
            lines.extend(v.form_errors);
            anyhow::anyhow!(lines.join("; "))
        }
        other => anyhow::anyhow!(other),
    }
}

/// `token:create`: issues an API token.
pub struct TokenCreate;

#[async_trait]
impl AppCommand for TokenCreate {
    fn definition(&self) -> Command {
        Command::new("token:create")
            .about("Issue an API token for a user (printed once)")
            .arg(Arg::new("email").long("email").required(true).help("Owner email"))
            .arg(Arg::new("name").long("name").required(true).help("Token label"))
            .arg(Arg::new("abilities").long("abilities").default_value("*").help("Comma-separated abilities"))
            .arg(
                Arg::new("expires-days")
                    .long("expires-days")
                    .value_parser(clap::value_parser!(u32).range(1..=i64::from(crate::tokens::MAX_LIFETIME_DAYS)))
                    .help("Lifetime in days (1 to 3650)"),
            )
    }

    async fn run(&self, state: &AppState, args: &ArgMatches) -> anyhow::Result<()> {
        let email = arg(args, "email").map(|e| e.trim().to_lowercase()).unwrap_or_default();
        let user = user::Entity::find()
            .filter(user::Column::Email.eq(&email))
            .one(state.db())
            .await?
            .with_context(|| format!("no user with email {email}"))?;
        let abilities: Vec<String> = arg(args, "abilities")
            .map(|a| a.split(',').map(str::trim).filter(|a| !a.is_empty()).map(str::to_owned).collect())
            .unwrap_or_default();
        let expires_at = match args.get_one::<u32>("expires-days") {
            Some(days) => Some(crate::tokens::expiry_in_days(*days).context("--expires-days is out of range")?),
            None => None,
        };
        let name = arg(args, "name").cloned().unwrap_or_default();
        let txn = rustcms_core::db::begin_write(state.db()).await?;
        let issued = crate::tokens::issue(&txn, user.id, &name, &abilities, expires_at).await?;
        let entry = crate::credentials::token_created(&issued);
        rustcms_core::audit::record(&txn, &AuditContext::system(Some("token:create")), entry).await?;
        txn.commit().await?;
        println!("{}", issued.plain_text);
        Ok(())
    }
}

/// `permission:sync`: creates declared permissions, marks undeclared ones orphaned.
pub struct PermissionSync;

#[async_trait]
impl AppCommand for PermissionSync {
    fn definition(&self) -> Command {
        Command::new("permission:sync").about("Create declared permissions and mark undeclared ones orphaned")
    }

    async fn run(&self, state: &AppState, _: &ArgMatches) -> anyhow::Result<()> {
        let auth = auth(state)?;
        let report = RbacService::new(state.db(), &auth.resolver)
            .audited(AuditContext::system(Some("permission:sync")))
            .sync_permissions(&auth.permissions.all())
            .await?;
        println!("{} created, {} orphaned", report.created.len(), report.orphaned.len());
        for name in &report.created {
            println!("  + {name}");
        }
        for name in &report.orphaned {
            println!("  ~ {name} (orphaned)");
        }
        Ok(())
    }
}

/// `session:prune`: deletes expired sessions and ended JWT sign-ins (also done hourly by `serve`).
pub struct SessionPrune;

#[async_trait]
impl AppCommand for SessionPrune {
    fn definition(&self) -> Command {
        Command::new("session:prune").about("Delete expired admin sessions and ended JWT sign-ins")
    }

    async fn run(&self, state: &AppState, _: &ArgMatches) -> anyhow::Result<()> {
        let pruned = crate::session::prune_expired(state.db()).await?;
        println!("{pruned} expired sessions deleted");
        let ended = crate::guards::jwt_session::prune(state.db()).await?;
        println!("{ended} ended JWT sign-ins deleted");
        Ok(())
    }
}

/// `role:assign`: gives an existing user a role of a guard (e.g. API access).
pub struct RoleAssign;

#[async_trait]
impl AppCommand for RoleAssign {
    fn definition(&self) -> Command {
        Command::new("role:assign")
            .about("Give an existing user a role (`super_admin` or any role) of a guard")
            .arg(Arg::new("email").long("email").required(true).help("User email"))
            .arg(Arg::new("role").long("role").required(true).help("Role name, e.g. `super_admin`"))
            .arg(Arg::new("guard").long("guard").default_value(ADMIN_GUARD).help("Guard of the role (`admin`, `api`)"))
    }

    async fn run(&self, state: &AppState, args: &ArgMatches) -> anyhow::Result<()> {
        use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
        let auth = auth(state)?;
        let email = arg(args, "email").map(|e| e.trim().to_lowercase()).unwrap_or_default();
        let name = arg(args, "role").map_or("", String::as_str);
        let guard = arg(args, "guard").map_or(ADMIN_GUARD, String::as_str);
        let user = user::Entity::find()
            .filter(user::Column::Email.eq(&email))
            .one(state.db())
            .await?
            .with_context(|| format!("no user with email {email}"))?;
        let role = crate::entities::role::Entity::find()
            .filter(crate::entities::role::Column::Name.eq(name))
            .filter(crate::entities::role::Column::GuardName.eq(guard))
            .one(state.db())
            .await?
            .with_context(|| format!("no role `{name}` in guard `{guard}`"))?;
        RbacService::new(state.db(), &auth.resolver)
            .audited(AuditContext::system(Some("role:assign")))
            .assign_role(Actor::System, user.id, role.id)
            .await
            .map_err(describe)?;
        println!("{} now holds `{name}` ({guard})", user.email);
        Ok(())
    }
}

/// Clap definitions of every auth command (for `rustcms docs:cli`).
pub fn definitions() -> Vec<Command> {
    vec![
        MakeUser.definition(),
        RoleAssign.definition(),
        TokenCreate.definition(),
        PermissionSync.definition(),
        SessionPrune.definition(),
        AuditPrune.definition(),
    ]
}

/// `audit:prune`: deletes audit entries older than `audit.retention_days`.
pub struct AuditPrune;

#[async_trait]
impl AppCommand for AuditPrune {
    fn definition(&self) -> Command {
        Command::new("audit:prune")
            .about("Delete audit log entries older than audit.retention_days (also done hourly by `serve`)")
    }

    async fn run(&self, state: &AppState, _: &ArgMatches) -> anyhow::Result<()> {
        let deleted = prune_audit(state, std::time::Duration::from_secs(3_600)).await?;
        println!("{deleted} audit entries deleted");
        Ok(())
    }
}

/// Deletes the audit entries past their retention (within `budget`).
pub(crate) async fn prune_audit(state: &AppState, budget: std::time::Duration) -> anyhow::Result<u64> {
    let config = rustcms_core::audit::AuditConfig::of(state.config())?;
    let cutoff = chrono::Duration::try_days(i64::from(config.retention_days))
        .and_then(|age| chrono::Utc::now().checked_sub_signed(age))
        .context("audit.retention_days is out of range")?;
    Ok(rustcms_core::audit::prune(state.db(), cutoff, budget).await?)
}
