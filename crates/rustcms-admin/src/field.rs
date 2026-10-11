//! Field declarations and their per-request access rules.

use std::sync::Arc;

use sea_orm::IdenStatic;

use crate::context::Ctx;

/// Predicate evaluated against the current request.
pub type Rule = Arc<dyn Fn(&Ctx) -> bool + Send + Sync>;

/// Write operation a field rule is evaluated for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    /// Creating a record.
    Create,
    /// Updating a record.
    Update,
}

/// A column exposed by a resource, with its access rules.
///
/// Fields are readable and writable by default; `.guarded()` fields are never
/// written from client input and are only readable when `.readable(..)` is set
/// explicitly. Client-side visibility never grants write access.
#[derive(Clone)]
pub struct Field {
    pub(crate) name: String,
    pub(crate) readable: Option<Rule>,
    pub(crate) creatable: Option<Rule>,
    pub(crate) updatable: Option<Rule>,
    pub(crate) guarded: bool,
    pub(crate) required: bool,
    pub(crate) sortable: bool,
    pub(crate) searchable: bool,
    pub(crate) filterable: bool,
    pub(crate) belongs_to: Option<&'static str>,
}

impl Field {
    /// Declares a field by column name.
    pub fn make(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            readable: None,
            creatable: None,
            updatable: None,
            guarded: false,
            required: false,
            sortable: false,
            searchable: false,
            filterable: false,
            belongs_to: None,
        }
    }

    /// Declares a field from a typed SeaORM column.
    pub fn column(column: impl IdenStatic) -> Self {
        Self::make(column.as_str())
    }

    /// Restricts who may read the field.
    pub fn readable(mut self, rule: impl Fn(&Ctx) -> bool + Send + Sync + 'static) -> Self {
        self.readable = Some(Arc::new(rule));
        self
    }

    /// Restricts who may set the field on create.
    pub fn creatable(mut self, rule: impl Fn(&Ctx) -> bool + Send + Sync + 'static) -> Self {
        self.creatable = Some(Arc::new(rule));
        self
    }

    /// Restricts who may change the field on update.
    pub fn updatable(mut self, rule: impl Fn(&Ctx) -> bool + Send + Sync + 'static) -> Self {
        self.updatable = Some(Arc::new(rule));
        self
    }

    /// Never written from client input (hooks may still set it).
    pub fn guarded(mut self) -> Self {
        self.guarded = true;
        self
    }

    /// Must be present and non-null on create.
    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    /// Allows sorting the list by this field.
    pub fn sortable(mut self) -> Self {
        self.sortable = true;
        self
    }

    /// Includes this field in the list search.
    pub fn searchable(mut self) -> Self {
        self.searchable = true;
        self
    }

    /// Allows filtering the list by this field.
    pub fn filterable(mut self) -> Self {
        self.filterable = true;
        self
    }

    /// Marks this foreign key as pointing to the resource registered under `slug`.
    pub fn belongs_to(mut self, slug: &'static str) -> Self {
        self.belongs_to = Some(slug);
        self
    }

    /// Whether the current user may read this field.
    pub fn can_read(&self, ctx: &Ctx) -> bool {
        match &self.readable {
            Some(rule) => rule(ctx),
            None => !self.guarded,
        }
    }

    /// Whether the current user may write this field for `op`.
    pub fn can_write(&self, op: Operation, ctx: &Ctx) -> bool {
        if self.guarded {
            return false;
        }
        let rule = match op {
            Operation::Create => &self.creatable,
            Operation::Update => &self.updatable,
        };
        rule.as_ref().is_none_or(|r| r(ctx))
    }
}
