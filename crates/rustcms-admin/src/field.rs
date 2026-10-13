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
    pub(crate) max_length: Option<usize>,
    pub(crate) max_items: Option<usize>,
    pub(crate) key_value: bool,
    pub(crate) media: Option<MediaRule>,
    pub(crate) rich_text: bool,
    pub(crate) label: Option<String>,
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
            max_length: None,
            max_items: None,
            key_value: false,
            media: None,
            rich_text: false,
            label: None,
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

    /// Maximum length in characters (strings), enforced on every write.
    pub fn max_length(mut self, max: usize) -> Self {
        self.max_length = Some(max);
        self
    }

    /// Holds key/value pairs: a JSON object of text values, checked on every
    /// write whatever the form shows (implied by a `KeyValue` input).
    pub fn key_value(mut self) -> Self {
        self.key_value = true;
        self
    }

    /// Most key/value pairs (default 50), enforced on every write; makes the
    /// field a [`Field::key_value`] field.
    pub fn max_items(mut self, max: usize) -> Self {
        self.max_items = Some(max);
        self.key_value = true;
        self
    }

    /// References one media item (a `Uuid` column): on every write it must
    /// exist, not be trashed, be visible to the user, and match `accept` /
    /// `max_size`. Needs the media library (`AdminProvider::media`).
    pub fn media(mut self) -> Self {
        self.media = Some(MediaRule { multiple: false, max_items: 1, accept: Vec::new(), max_size: None });
        self
    }

    /// References up to `max` media items, in order (a JSON array of ids).
    pub fn media_list(mut self, max: usize) -> Self {
        self.media = Some(MediaRule { multiple: true, max_items: max.max(1), accept: Vec::new(), max_size: None });
        self
    }

    /// Accepted MIME types of referenced media (`image/*`, `application/pdf`).
    pub fn accept(mut self, types: &[&str]) -> Self {
        if let Some(rule) = &mut self.media {
            rule.accept = types.iter().map(|t| (*t).to_owned()).collect();
        }
        self
    }

    /// Largest accepted referenced media, in bytes.
    pub fn max_size(mut self, bytes: u64) -> Self {
        if let Some(rule) = &mut self.media {
            rule.max_size = Some(bytes);
        }
        self
    }

    /// Holds a rich text document (a JSON column; see [`crate::rich_text`]):
    /// validated on every write; its images must be visible media items.
    pub fn rich_text(mut self) -> Self {
        self.rich_text = true;
        self
    }

    /// Label used by generated forms and tables.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Label, or the humanized name.
    pub(crate) fn display_label(&self) -> String {
        self.label.clone().unwrap_or_else(|| crate::ui::humanize(&self.name))
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

/// What a media field may reference.
#[derive(Debug, Clone)]
pub(crate) struct MediaRule {
    pub(crate) multiple: bool,
    pub(crate) max_items: usize,
    pub(crate) accept: Vec<String>,
    pub(crate) max_size: Option<u64>,
}

impl MediaRule {
    /// Whether `mime` is accepted (`image/*` matches every image type).
    pub(crate) fn accepts(&self, mime: &str) -> bool {
        self.accept.is_empty()
            || self
                .accept
                .iter()
                .any(|a| a == mime || a.strip_suffix("/*").is_some_and(|p| mime.starts_with(&format!("{p}/"))))
    }
}
