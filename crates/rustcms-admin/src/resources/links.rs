//! Writes to belongs-to-many relations: attach, detach and pivot updates.

use serde::Serialize;

use crate::Data;

/// A write to a belongs-to-many relation, by the ability it needs on the
/// parent record (`{slug}.attach_{relation}`, … with the default policy).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkOp {
    /// Link targets to the record.
    Attach,
    /// Unlink targets from the record.
    Detach,
    /// Change the pivot columns of one link.
    UpdatePivot,
}

impl LinkOp {
    /// The ability on the parent record for relation `relation`.
    pub fn ability(self, relation: &str) -> String {
        match self {
            Self::Attach => format!("attach_{relation}"),
            Self::Detach => format!("detach_{relation}"),
            Self::UpdatePivot => format!("update_pivot_{relation}"),
        }
    }
}

/// What a link write asks for.
#[derive(Debug, Clone)]
pub enum LinkRequest {
    /// Link `keys` (targets), with the pivot `values` of new links.
    Attach {
        /// Target keys.
        keys: Vec<String>,
        /// Pivot columns of the new links.
        values: Data,
    },
    /// Unlink `keys`.
    Detach {
        /// Target keys.
        keys: Vec<String>,
    },
    /// Change the pivot columns of the link to `key`.
    UpdatePivot {
        /// Target key.
        key: String,
        /// Pivot columns.
        values: Data,
    },
}

/// Result of a link write.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct LinkOutcome {
    /// Targets linked (attach: already or now), unlinked (detach) or updated.
    pub processed: Vec<String>,
    /// Targets not found, or not visible to the user: never touched.
    pub missing: Vec<String>,
}

/// A target offered for attaching.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct LinkOption {
    /// Target key.
    pub value: String,
    /// Target title.
    pub label: String,
    /// Already linked to the record.
    pub attached: bool,
}

/// What one write changed in a belongs-to-many relation of a record, given to
/// [`crate::Resource::after_relation_change`]. Keys are the targets' canonical
/// keys, sorted; only links that really changed are listed (attaching a
/// linked target or detaching an unlinked one changes nothing).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct RelationChange {
    /// The relation's name (`Relation::belongs_to_many`'s first argument).
    pub relation: &'static str,
    /// Targets linked by the write.
    pub attached: Vec<String>,
    /// Targets unlinked by the write.
    pub detached: Vec<String>,
    /// Targets whose pivot columns the write changed.
    pub pivot_updated: Vec<String>,
}

impl RelationChange {
    pub(crate) fn new(relation: &'static str) -> Self {
        Self { relation, ..Self::default() }
    }

    /// Whether the write changed nothing.
    pub fn is_empty(&self) -> bool {
        self.attached.is_empty() && self.detached.is_empty() && self.pivot_updated.is_empty()
    }
}
