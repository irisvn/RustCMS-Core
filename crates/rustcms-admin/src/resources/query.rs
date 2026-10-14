//! What a list asks for and what it returns: query, filters, trash, limits,
//! records and pages.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::Data;

/// Operator of a list filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilterOp {
    /// Equal.
    Eq,
    /// Member of a list.
    In,
    /// Greater than.
    Gt,
    /// Greater or equal.
    Gte,
    /// Less than.
    Lt,
    /// Less or equal.
    Lte,
}

/// One list filter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Filter {
    /// Field name.
    pub field: String,
    /// Operator.
    pub op: FilterOp,
    /// Value in wire format (an array for `in`).
    pub value: serde_json::Value,
}

/// Records by deletion state, for resources with soft deletes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Trashed {
    /// Active records only.
    #[default]
    Without,
    /// Active and trashed records.
    With,
    /// Trashed records only.
    Only,
}

/// Parameters of a list request.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListQuery {
    /// 1-based page.
    pub page: Option<u64>,
    /// Page size (capped).
    pub per_page: Option<u64>,
    /// Field to sort by, `-` prefix for descending.
    pub sort: Option<String>,
    /// Text searched in searchable fields.
    pub search: Option<String>,
    /// Filters, combined with AND.
    #[serde(default)]
    pub filters: Vec<Filter>,
    /// Which records by deletion state (resources with soft deletes only).
    #[serde(default)]
    pub trashed: Trashed,
    /// Only the records whose `field` holds this key (relation lists: the
    /// children of one record). Never read from a request; trusted Rust callers
    /// may set it.
    #[serde(skip)]
    pub parent: Option<(String, String)>,
    /// Filter values came from a URL query string, so they are all strings and
    /// are interpreted by field type (`5`, `true`, comma-separated `in` lists).
    /// JSON clients keep the strict wire format.
    #[serde(skip)]
    pub from_query_string: bool,
}

/// Request size limits (see plan §4). Exceeding one is a 422.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Maximum page size.
    pub per_page: u64,
    /// Maximum search length (chars).
    pub search_len: usize,
    /// Maximum number of filters.
    pub filters: usize,
    /// Maximum items in an `in` filter.
    pub in_items: usize,
    /// Maximum keys in an action or lookup.
    pub keys: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self { per_page: 100, search_len: 100, filters: 10, in_items: 100, keys: 500 }
    }
}

/// A record as sent to clients.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Envelope {
    /// Encoded primary key.
    pub key: String,
    /// Record title, when the title field is readable.
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Record-level abilities computed by the policy.
    pub abilities: BTreeMap<&'static str, bool>,
    /// Readable fields only.
    pub data: Data,
    /// The record is in the trash (soft deletes); then only `view`, `restore`
    /// and `force_delete` apply.
    #[cfg_attr(feature = "ts", ts(as = "Option<bool>", optional))]
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub trashed: bool,
    /// Titles of related records for `belongs_to` fields (`#key` when not visible).
    #[cfg_attr(feature = "ts", ts(as = "Option<BTreeMap<String, String>>", optional))]
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub labels: BTreeMap<String, String>,
}

/// Pagination metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct PageMeta {
    /// Current page (1-based).
    pub page: u64,
    /// Page size.
    pub per_page: u64,
    /// Total records in scope matching the query.
    pub total: u64,
    /// Last page number.
    pub last_page: u64,
}

/// One page of records.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Page {
    /// Records.
    pub data: Vec<Envelope>,
    /// Pagination.
    pub meta: PageMeta,
}
