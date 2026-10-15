//! Seeders and model factories (`src/database/` of an application).
//!
//! - [`Seeder`]: fills the database; registered with [`crate::AppBuilder::seeder`]
//!   and run by `db:seed` (the one named `database` by default, like Laravel's
//!   `DatabaseSeeder`).
//! - [`Factory`]: builds records from a definition, for seeders and tests.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use sea_orm::{ActiveModelBehavior, ActiveModelTrait, ConnectionTrait, DbErr, EntityTrait, IntoActiveModel};

use crate::AppState;

/// Name of the seeder `db:seed` runs without `--class`.
pub const DEFAULT_SEEDER: &str = "database";

/// Fills the database with records an application needs or shows off.
///
/// Seeders run against a live database, possibly more than once: find rows by
/// their natural key before creating them, and write through the services
/// that enforce your rules.
#[async_trait]
pub trait Seeder: Send + Sync + 'static {
    /// Name for `db:seed --class <name>`: 1–64 of `a-z`, `0-9`, `_`.
    fn name(&self) -> &'static str;

    /// Seeds.
    async fn run(&self, state: &AppState) -> anyhow::Result<()>;
}

/// Runs `seeders` in order (from a seeder that groups others).
pub async fn call(state: &AppState, seeders: &[&dyn Seeder]) -> anyhow::Result<()> {
    for seeder in seeders {
        println!("Seeding {}…", seeder.name());
        seeder.run(state).await.map_err(|err| err.context(format!("seeder `{}` failed", seeder.name())))?;
    }
    Ok(())
}

/// The seeders an application registered (see [`crate::AppBuilder::seeder`]).
#[derive(Default, Clone)]
pub struct Seeders {
    seeders: Vec<Arc<dyn Seeder>>,
}

impl Seeders {
    /// Registers `seeder`.
    ///
    /// # Panics
    /// On an invalid or repeated name.
    pub fn register(&mut self, seeder: Arc<dyn Seeder>) {
        let name = seeder.name();
        assert!(valid_name(name), "seeder `{name}`: names are 1–64 of `a-z`, `0-9`, `_`");
        assert!(self.get(name).is_none(), "seeder `{name}` registered twice");
        self.seeders.push(seeder);
    }

    /// The seeder named `name`.
    pub fn get(&self, name: &str) -> Option<&Arc<dyn Seeder>> {
        self.seeders.iter().find(|s| s.name() == name)
    }

    /// Registered names, in registration order.
    pub fn names(&self) -> Vec<&'static str> {
        self.seeders.iter().map(|s| s.name()).collect()
    }
}

fn valid_name(name: &str) -> bool {
    (1..=64).contains(&name.len()) && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

static SEQUENCE: AtomicU64 = AtomicU64::new(1);

/// Builds records from a definition, like Laravel model factories.
///
/// Implement [`Factory::definition`]; `try_create` inserts (seeders), `create`
/// inserts or panics (tests). The sequence number is unique per process, so
/// unique columns stay unique.
#[async_trait]
pub trait Factory: Sync {
    /// Entity produced.
    type Entity: EntityTrait;

    /// Column values of record number `n`.
    fn definition(&self, n: u64) -> <Self::Entity as EntityTrait>::ActiveModel;

    /// Inserts one record.
    async fn try_create<C: ConnectionTrait>(&self, db: &C) -> Result<<Self::Entity as EntityTrait>::Model, DbErr>
    where
        <Self::Entity as EntityTrait>::Model: IntoActiveModel<<Self::Entity as EntityTrait>::ActiveModel>,
        <Self::Entity as EntityTrait>::ActiveModel: ActiveModelBehavior + Send,
    {
        self.definition(SEQUENCE.fetch_add(1, Ordering::Relaxed)).insert(db).await
    }

    /// Inserts `count` records.
    async fn try_create_many<C: ConnectionTrait>(
        &self,
        db: &C,
        count: usize,
    ) -> Result<Vec<<Self::Entity as EntityTrait>::Model>, DbErr>
    where
        <Self::Entity as EntityTrait>::Model: IntoActiveModel<<Self::Entity as EntityTrait>::ActiveModel> + Send,
        <Self::Entity as EntityTrait>::ActiveModel: ActiveModelBehavior + Send,
    {
        let mut out = Vec::with_capacity(count);
        for _ in 0..count {
            out.push(self.try_create(db).await?);
        }
        Ok(out)
    }

    /// Inserts one record (panics on a database error: for tests).
    async fn create<C: ConnectionTrait>(&self, db: &C) -> <Self::Entity as EntityTrait>::Model
    where
        <Self::Entity as EntityTrait>::Model: IntoActiveModel<<Self::Entity as EntityTrait>::ActiveModel>,
        <Self::Entity as EntityTrait>::ActiveModel: ActiveModelBehavior + Send,
    {
        self.try_create(db).await.expect("factory insert")
    }

    /// Inserts `count` records (panics on a database error: for tests).
    async fn create_many<C: ConnectionTrait>(&self, db: &C, count: usize) -> Vec<<Self::Entity as EntityTrait>::Model>
    where
        <Self::Entity as EntityTrait>::Model: IntoActiveModel<<Self::Entity as EntityTrait>::ActiveModel> + Send,
        <Self::Entity as EntityTrait>::ActiveModel: ActiveModelBehavior + Send,
    {
        self.try_create_many(db, count).await.expect("factory insert")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Named(&'static str);

    #[async_trait]
    impl Seeder for Named {
        fn name(&self) -> &'static str {
            self.0
        }

        async fn run(&self, _state: &AppState) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn seeders_have_unique_valid_names() {
        let mut seeders = Seeders::default();
        seeders.register(Arc::new(Named("database")));
        seeders.register(Arc::new(Named("courses_2")));
        assert_eq!(seeders.names(), ["database", "courses_2"]);
        assert!(seeders.get("courses_2").is_some() && seeders.get("missing").is_none());
        for bad in ["", "Courses", "courses-x", "a b"] {
            let result = std::panic::catch_unwind(move || Seeders::default().register(Arc::new(Named(bad))));
            assert!(result.is_err(), "{bad:?}");
        }
        let twice = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            seeders.register(Arc::new(Named("database")))
        }));
        assert!(twice.is_err());
    }
}
