//! Rich text as a structured document (stored as JSON), never as HTML.
//!
//! The document is validated strictly on every write: unknown node types,
//! marks or attributes are refused, links are limited to `http`, `https` and
//! `mailto`, images reference media items by id, embeds name an allowed
//! provider and an id (never a URL or HTML). [`to_html`] renders it with
//! every text and attribute escaped, for headless consumers.
//!
//! ```json
//! {"type": "doc", "version": 2, "content": [
//!   {"type": "paragraph", "content": [{"type": "text", "text": "Hi", "marks": [{"type": "bold"}]}]},
//!   {"type": "image", "attrs": {"media": "0199d3a4-…", "alt": "A cat", "caption": "Our cat", "width": "wide"}},
//!   {"type": "embed", "attrs": {"provider": "youtube", "id": "dQw4w9WgXcQ"}}
//! ]}
//! ```
//!
//! Versions: version 1 (0.2–0.9) is a subset of version 2 (0.10). Both are
//! read as they are; tables, embeds, callouts, image captions and image
//! widths are only valid in version 2. [`upgrade`] turns a version 1
//! document into the same document in version 2.

mod html;
mod parse;

pub use html::*;
pub use parse::*;

use serde::Deserialize;
use serde_json::Value;

/// Current document format version (what the panel's editor saves).
pub const VERSION: u64 = 2;
/// Oldest document format version still read (as it is).
pub const OLDEST_VERSION: u64 = 1;
/// Largest stored document (bytes of JSON).
pub const MAX_BYTES: usize = 256 * 1024;

/// A validated document.
#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    /// Format version it was written in ([`OLDEST_VERSION`]..=[`VERSION`]).
    pub version: u64,
    /// Top-level blocks.
    pub content: Vec<Node>,
}

/// A block or inline node.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Node {
    /// Paragraph.
    Paragraph(Vec<Node>),
    /// Heading, level 2 to 4.
    Heading(u8, Vec<Node>),
    /// Bulleted list of `ListItem`s.
    BulletList(Vec<Node>),
    /// Numbered list of `ListItem`s.
    OrderedList(Vec<Node>),
    /// List item (blocks).
    ListItem(Vec<Node>),
    /// Quote (blocks).
    Blockquote(Vec<Node>),
    /// Preformatted code (plain text).
    CodeBlock(String),
    /// Horizontal rule.
    HorizontalRule,
    /// Line break.
    HardBreak,
    /// Image from the media library.
    Image(Image),
    /// Table (version 2).
    Table(Table),
    /// Video or post of an allowed provider (version 2).
    Embed(Embed),
    /// Highlighted paragraphs (version 2).
    Callout(CalloutTone, Vec<Node>),
    /// Text with marks.
    Text(String, Vec<Mark>),
}

/// An image block.
#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    /// Media id.
    pub media: uuid::Uuid,
    /// Alternative text (may be empty).
    pub alt: String,
    /// Caption, plain text (empty: none; version 2).
    pub caption: String,
    /// How wide it is shown (version 2).
    pub width: ImageWidth,
}

/// How wide an image is shown: a closed set the frontend styles.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum ImageWidth {
    /// The text's width (the default).
    #[default]
    Normal,
    /// Wider than the text.
    Wide,
    /// The page's full width.
    Full,
}

impl ImageWidth {
    /// Its name in documents (`normal`, `wide`, `full`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Wide => "wide",
            Self::Full => "full",
        }
    }
}

/// A table: rows of cells, all rows of the same length; the first row may be
/// a header row.
#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    /// Whether the first row is a header row.
    pub header: bool,
    /// Rows of cells (at least one row of at least one cell).
    pub rows: Vec<Vec<Cell>>,
}

/// A table cell: paragraphs (inline marks only).
#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    /// At least one paragraph.
    pub content: Vec<Node>,
}

/// An embedded video or post: a provider and its id, never a URL or HTML.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Embed {
    /// Provider.
    pub provider: EmbedProvider,
    /// The provider's id (checked against its format).
    pub id: String,
}

impl Embed {
    /// The provider's public page for it (`https://`).
    pub fn url(&self) -> String {
        match self.provider {
            EmbedProvider::YouTube => format!("https://www.youtube.com/watch?v={}", self.id),
            EmbedProvider::Vimeo => format!("https://vimeo.com/{}", self.id),
            EmbedProvider::X => format!("https://x.com/i/status/{}", self.id),
        }
    }
}

/// Embed providers (an allowlist).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum EmbedProvider {
    /// `youtube`: 11-character video ids (`A-Z a-z 0-9 - _`).
    YouTube,
    /// `vimeo`: numeric video ids (1 to 15 digits).
    Vimeo,
    /// `x`: numeric post ids (1 to 20 digits).
    X,
}

impl EmbedProvider {
    /// Its name in documents (`youtube`, `vimeo`, `x`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::YouTube => "youtube",
            Self::Vimeo => "vimeo",
            Self::X => "x",
        }
    }

    /// The provider named `name`, if allowed.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "youtube" => Some(Self::YouTube),
            "vimeo" => Some(Self::Vimeo),
            "x" => Some(Self::X),
            _ => None,
        }
    }

    /// Whether `id` has the provider's id format.
    pub fn valid_id(self, id: &str) -> bool {
        let digits = |max: usize| (1..=max).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_digit());
        match self {
            Self::YouTube => id.len() == 11 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
            Self::Vimeo => digits(15),
            Self::X => digits(20),
        }
    }
}

/// A callout's tone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CalloutTone {
    /// `info`.
    Info,
    /// `warn`.
    Warn,
}

impl CalloutTone {
    /// Its name in documents (`info`, `warn`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warn => "warn",
        }
    }
}

/// Inline formatting.
#[derive(Debug, Clone, PartialEq)]
pub enum Mark {
    /// Bold.
    Bold,
    /// Italic.
    Italic,
    /// Strikethrough.
    Strike,
    /// Inline code.
    Code,
    /// Link (`http`, `https` or `mailto`).
    Link(String),
}

/// Why a document was refused (a field error message).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct DocumentError(pub String);

/// `http(s)://` or `mailto:`, no whitespace or control characters, bounded.
pub fn safe_href(href: &str) -> bool {
    let lower = href.to_ascii_lowercase();
    href.len() <= MAX_HREF
        && !href.chars().any(|c| c.is_whitespace() || c.is_control())
        && (lower.starts_with("https://") || lower.starts_with("http://") || lower.starts_with("mailto:"))
        && href.len() > "mailto:".len()
}

/// Calls `f` on every node in document order (a node before its children).
pub(crate) fn walk<'a>(nodes: &'a [Node], f: &mut impl FnMut(&'a Node)) {
    for node in nodes {
        f(node);
        match node {
            Node::Paragraph(c)
            | Node::Heading(_, c)
            | Node::BulletList(c)
            | Node::OrderedList(c)
            | Node::ListItem(c)
            | Node::Blockquote(c)
            | Node::Callout(_, c) => walk(c, f),
            Node::Table(table) => {
                for cell in table.rows.iter().flatten() {
                    walk(&cell.content, f);
                }
            }
            _ => {}
        }
    }
}

/// Media items the document's images reference, in order, without repeats.
pub fn media_ids(doc: &Document) -> Vec<uuid::Uuid> {
    let mut out = Vec::new();
    walk(&doc.content, &mut |node| {
        if let Node::Image(image) = node
            && !out.contains(&image.media)
        {
            out.push(image.media);
        }
    });
    out
}

/// The same document in the current format [`VERSION`]. Version 1 is a
/// subset of version 2, so only the version number changes; anything else (a
/// current document, a value that is not a document) is returned as it is.
/// It does not validate: [`parse`] the result.
pub fn upgrade(value: &Value) -> Value {
    let mut out = value.clone();
    if let Value::Object(map) = &mut out
        && map.get("type").and_then(Value::as_str) == Some("doc")
        && map.get("version").and_then(Value::as_u64) == Some(1)
    {
        map.insert("version".into(), Value::from(VERSION));
    }
    out
}

#[cfg(test)]
mod tests;
