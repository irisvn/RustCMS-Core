use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use rustcms_core::models::user;
use sea_orm::{ActiveModelBehavior, ActiveModelTrait, ConnectionTrait, EntityTrait, IntoActiveModel, Set};

static SEQUENCE: AtomicU64 = AtomicU64::new(1);

/// Builds records for tests, like Laravel model factories.
///
/// Implement [`Factory::definition`]; `create` and `create_many` insert.
/// The sequence number is unique per process, so unique columns stay unique.
#[async_trait]
pub trait Factory: Sync {
    /// Entity produced.
    type Entity: EntityTrait;

    /// Column values of record number `n`.
    fn definition(&self, n: u64) -> <Self::Entity as EntityTrait>::ActiveModel;

    /// Inserts one record.
    async fn create<C: ConnectionTrait>(&self, db: &C) -> <Self::Entity as EntityTrait>::Model
    where
        <Self::Entity as EntityTrait>::Model: IntoActiveModel<<Self::Entity as EntityTrait>::ActiveModel>,
        <Self::Entity as EntityTrait>::ActiveModel: ActiveModelBehavior + Send,
    {
        self.definition(SEQUENCE.fetch_add(1, Ordering::Relaxed)).insert(db).await.expect("factory insert")
    }

    /// Inserts `count` records.
    async fn create_many<C: ConnectionTrait>(&self, db: &C, count: usize) -> Vec<<Self::Entity as EntityTrait>::Model>
    where
        <Self::Entity as EntityTrait>::Model: IntoActiveModel<<Self::Entity as EntityTrait>::ActiveModel> + Send,
        <Self::Entity as EntityTrait>::ActiveModel: ActiveModelBehavior + Send,
    {
        let mut out = Vec::with_capacity(count);
        for _ in 0..count {
            out.push(self.create(db).await);
        }
        out
    }
}

/// Users with unique emails. Without `.password(..)`, the stored hash matches
/// no password (use `TestApp::acting_as` to sign in without one).
#[derive(Debug, Clone)]
pub struct UserFactory {
    name: Option<String>,
    email: Option<String>,
    password_hash: Option<String>,
    active: bool,
}

impl Default for UserFactory {
    fn default() -> Self {
        Self { name: None, email: None, password_hash: None, active: true }
    }
}

impl UserFactory {
    /// Active user named `User <n>`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the name.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Sets the email (only use with `create`, not `create_many`).
    pub fn email(mut self, email: impl Into<String>) -> Self {
        self.email = Some(email.into());
        self
    }

    /// Sets the password (hashed with Argon2id), so the user can sign in.
    pub fn password(mut self, password: &str) -> Self {
        self.password_hash = Some(rustcms_auth::password::hash_blocking(password).expect("hash password"));
        self
    }

    /// Creates a disabled user.
    pub fn inactive(mut self) -> Self {
        self.active = false;
        self
    }
}

impl Factory for UserFactory {
    type Entity = user::Entity;

    fn definition(&self, n: u64) -> user::ActiveModel {
        user::ActiveModel {
            name: Set(self.name.clone().unwrap_or_else(|| format!("User {n}"))),
            email: Set(self.email.clone().unwrap_or_else(|| format!("user{n}@example.test"))),
            password: Set(self.password_hash.clone().unwrap_or_else(|| "!".into())),
            is_active: Set(self.active),
            ..Default::default()
        }
    }
}
