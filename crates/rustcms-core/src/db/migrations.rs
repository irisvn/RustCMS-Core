//! Migrations a database has but this binary does not know: what an older
//! binary sees after a rollback of a deploy whose newer binary migrated. By
//! default `migrate` refuses them; with `--allow-newer` (or `[database]
//! allow_newer_migrations = true`) it logs them and applies the migrations it
//! knows that are still pending. Safe only when migrations are additive (new
//! tables, nullable columns): the application keeps to that rule.

use sea_orm::sea_query::{Alias, Order, Query};
use sea_orm::{ConnectionTrait, DbErr};
use sea_orm_migration::{MigrationName, MigrationTrait, MigratorTrait, MigratorTraitSelf, SchemaManager};
use serde::Deserialize;

/// `[database]` settings of `migrate`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct MigrationSettings {
    /// Accept migrations applied by a newer binary (see the module docs).
    pub allow_newer_migrations: bool,
}

impl MigrationSettings {
    /// The `[database]` section's migration settings.
    pub fn of(config: &crate::Config) -> Self {
        config.section("database").unwrap_or_default()
    }
}

/// Names of the migrations applied in `db` that `M` does not list, in the
/// order they were applied (read only: nothing is created).
///
/// # Errors
/// On a database error.
pub async fn unknown_migrations<M: MigratorTrait>(db: &sea_orm::DatabaseConnection) -> Result<Vec<String>, DbErr> {
    let table = M::migration_table_name();
    let manager = SchemaManager::new(db);
    if !manager.has_table(table.to_string()).await? {
        return Ok(Vec::new());
    }
    let known: std::collections::HashSet<String> = M::migrations().iter().map(|m| m.name().to_owned()).collect();
    // The migrator's own history table (it may be renamed).
    let select = Query::select()
        .column(Alias::new("version"))
        .from(table)
        .order_by(Alias::new("applied_at"), Order::Asc)
        .order_by(Alias::new("version"), Order::Asc)
        .to_owned();
    let mut versions = Vec::new();
    for row in db.query_all(&select).await? {
        let version: String = row.try_get("", "version")?;
        if !known.contains(&version) {
            versions.push(version);
        }
    }
    Ok(versions)
}

/// A migration this binary does not have, standing in for its applied row.
struct Newer(String);

impl MigrationName for Newer {
    fn name(&self) -> &str {
        &self.0
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Newer {
    async fn up(&self, _: &SchemaManager) -> Result<(), DbErr> {
        Err(DbErr::Custom(format!("migration `{}` is not in this binary", self.0)))
    }

    async fn down(&self, _: &SchemaManager) -> Result<(), DbErr> {
        Err(DbErr::Custom(format!("migration `{}` is not in this binary: it cannot be rolled back here", self.0)))
    }
}

/// `M`'s migrations plus a stand-in for each unknown applied one.
struct Tolerant<M> {
    newer: Vec<String>,
    // `fn() -> M`: Send and Sync whatever `M` is.
    marker: std::marker::PhantomData<fn() -> M>,
}

#[async_trait::async_trait]
impl<M: MigratorTrait> MigratorTraitSelf for Tolerant<M> {
    fn migrations(&self) -> Vec<Box<dyn MigrationTrait>> {
        let mut all = M::migrations();
        all.extend(self.newer.iter().map(|name| Box::new(Newer(name.clone())) as Box<dyn MigrationTrait>));
        all
    }

    fn migration_table_name(&self) -> sea_orm::DynIden {
        M::migration_table_name()
    }
}

/// Applies `M`'s pending migrations. Unknown applied migrations are refused,
/// unless `allow_newer`: then they are logged and left as they are.
///
/// # Errors
/// On unknown migrations without `allow_newer`, or a migration error.
pub async fn migrate_up<M: MigratorTrait>(db: &sea_orm::DatabaseConnection, allow_newer: bool) -> anyhow::Result<()> {
    let newer = unknown_migrations::<M>(db).await?;
    if newer.is_empty() {
        return Ok(M::up(db, None).await?);
    }
    if !allow_newer {
        anyhow::bail!(
            "the database has migrations this binary does not know ({}): it was migrated by a newer version. \
             If those migrations only add tables or nullable columns, run `migrate --allow-newer` (or set \
             [database] allow_newer_migrations = true) to apply this version's own pending migrations",
            newer.join(", ")
        );
    }
    tracing::warn!(migrations = ?newer, "the database has migrations from a newer version; they are left as they are");
    let tolerant = Tolerant::<M> { newer, marker: std::marker::PhantomData };
    Ok(MigratorTraitSelf::up(&tolerant, db, None).await?)
}

/// Every migration with its status, unknown applied ones last (`Newer`).
///
/// # Errors
/// On a database error.
pub async fn migration_status<M: MigratorTrait>(
    db: &sea_orm::DatabaseConnection,
) -> Result<Vec<(String, String)>, DbErr> {
    let newer = unknown_migrations::<M>(db).await?;
    let tolerant = Tolerant::<M> { newer: newer.clone(), marker: std::marker::PhantomData };
    let mut out: Vec<(String, String)> = MigratorTraitSelf::get_migration_with_status(&tolerant, db)
        .await?
        .into_iter()
        .filter(|m| !newer.iter().any(|n| n == m.name()))
        .map(|m| (format!("{:?}", m.status()), m.name().to_owned()))
        .collect();
    out.extend(newer.into_iter().map(|name| ("Newer".to_owned(), name)));
    Ok(out)
}
