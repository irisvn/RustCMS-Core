//! Rich text as a structured document (stored as JSON), never as HTML.
//!
//! The document is validated strictly on every write: unknown node types,
//! marks or attributes are refused, links are limited to `http`, `https` and
//! `mailto`, images reference media items by id, embeds name an allowed
//! provider and an id (never a URL or HTML). [`to_html`] and [`to_html_with`]
//! render it with every text and attribute escaped, for headless consumers;
//! [`outline`] lists its headings with the ids the HTML gives them.
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
//! widths are only valid in version 2. [`upgrade_value`] turns a stored
//! version 1 document into the same document in version 2 (only the version
//! changes).

mod html;
mod outline;
mod parse;

pub use html::*;
pub use outline::*;
pub use parse::*;

use serde::Deserialize;
use serde_json::Value;

/// Current document format version (the panel's editor saves the lowest
/// version a document needs: version 1 without version 2 parts).
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
    /// Video of an allowed provider (version 2).
    Embed(Embed),
    /// Highlighted paragraphs and lists, no table or callout inside (version 2).
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

/// An embedded video: a provider and its id, never a URL or HTML.
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
        }
    }

    /// The embed an address points to, from an allowlist of hosts and paths,
    /// or `None` (the same allowlist as the panel's editor, which also refuses
    /// what this refuses):
    /// `youtube.com/watch?v=ID` (also `www.`, `m.`, `youtube-nocookie.com`),
    /// `youtube.com/{embed,shorts,live,v}/ID`, `youtu.be/ID`,
    /// `vimeo.com/ID` (and `vimeo.com/ID/hash`, the hash not kept),
    /// `player.vimeo.com/video/ID`. `http` or `https`, no user, password or
    /// port; the id must have the provider's format.
    pub fn from_url(url: &str) -> Option<Embed> {
        let url = url.trim();
        let lower = url.to_ascii_lowercase();
        let rest = if lower.starts_with("https://") {
            &url[8..]
        } else if lower.starts_with("http://") {
            &url[7..]
        } else {
            return None;
        };
        if url.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return None;
        }
        let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        let (authority, rest) = rest.split_at(end);
        if authority.contains(['@', ':']) {
            return None;
        }
        let host = authority.to_ascii_lowercase();
        let host = ["www.", "m.", "mobile."].iter().find_map(|p| host.strip_prefix(p)).unwrap_or(&host).to_owned();
        let rest = rest.split('#').next().unwrap_or_default();
        let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
        let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
        let pick = |provider: EmbedProvider, id: Option<&str>| {
            id.filter(|id| provider.valid_id(id)).map(|id| Embed { provider, id: id.to_owned() })
        };
        match (host.as_str(), parts.as_slice()) {
            ("youtube.com" | "youtube-nocookie.com", ["watch"]) => {
                pick(EmbedProvider::YouTube, query.split('&').find_map(|pair| pair.strip_prefix("v=")))
            }
            ("youtube.com" | "youtube-nocookie.com", ["embed" | "shorts" | "live" | "v", id]) => {
                pick(EmbedProvider::YouTube, Some(id))
            }
            ("youtu.be", [id]) => pick(EmbedProvider::YouTube, Some(id)),
            ("vimeo.com", [id] | [id, _]) => pick(EmbedProvider::Vimeo, Some(id)),
            ("player.vimeo.com", ["video", id]) => pick(EmbedProvider::Vimeo, Some(id)),
            _ => None,
        }
    }
}

/// Embed providers (an allowlist).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum EmbedProvider {
    /// `youtube`: 11-character video ids (`[A-Za-z0-9_-]{11}`).
    YouTube,
    /// `vimeo`: numeric video ids (`[1-9][0-9]{0,14}`).
    Vimeo,
}

impl EmbedProvider {
    /// Its name in documents (`youtube`, `vimeo`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::YouTube => "youtube",
            Self::Vimeo => "vimeo",
        }
    }

    /// The provider named `name`, if allowed.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "youtube" => Some(Self::YouTube),
            "vimeo" => Some(Self::Vimeo),
            _ => None,
        }
    }

    /// Whether `id` has the provider's id format.
    pub fn valid_id(self, id: &str) -> bool {
        match self {
            Self::YouTube => id.len() == 11 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
            Self::Vimeo => {
                (1..=15).contains(&id.len()) && !id.starts_with('0') && id.bytes().all(|b| b.is_ascii_digit())
            }
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

/// A stored value in the current format [`VERSION`], for rewriting stored
/// documents: version 1 is a subset of version 2 (no node or attribute
/// changes shape), so a version 1 document comes back as version 2 with only
/// `version` changed; anything else
/// (a current document, a value that is not a document) as it is. It does not
/// validate: [`parse`] the result.
pub fn upgrade_value(value: &Value) -> Value {
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
