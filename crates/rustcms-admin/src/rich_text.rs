//! Rich text as a structured document (stored as JSON), never as HTML.
//!
//! The document is validated strictly on every write: unknown node types,
//! marks or attributes are refused, links are limited to `http`, `https` and
//! `mailto`, images reference media items by id. [`to_html`] renders it with
//! every text and attribute escaped, for headless consumers.
//!
//! ```json
//! {"type": "doc", "version": 1, "content": [
//!   {"type": "paragraph", "content": [{"type": "text", "text": "Hi", "marks": [{"type": "bold"}]}]},
//!   {"type": "image", "attrs": {"media": "0199d3a4-…", "alt": "A cat"}}
//! ]}
//! ```

use serde::Deserialize;
use serde_json::Value;

/// Document format version.
pub const VERSION: u64 = 1;
/// Largest stored document (bytes of JSON).
pub const MAX_BYTES: usize = 256 * 1024;
const MAX_NODES: usize = 10_000;
const MAX_DEPTH: usize = 16;
const MAX_IMAGES: usize = 100;
const MAX_HREF: usize = 2_048;
const MAX_ALT: usize = 500;
/// Marks on one text (each kind at most once).
const MAX_MARKS: usize = 5;

/// A validated document.
#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    /// Top-level blocks.
    pub content: Vec<Node>,
}

/// A block or inline node.
#[derive(Debug, Clone, PartialEq)]
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
    Image {
        /// Media id.
        media: uuid::Uuid,
        /// Alternative text.
        alt: String,
    },
    /// Text with marks.
    Text(String, Vec<Mark>),
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

fn err(message: impl Into<String>) -> DocumentError {
    DocumentError(message.into())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawNode {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    content: Option<Vec<RawNode>>,
    #[serde(default)]
    attrs: Option<serde_json::Map<String, Value>>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    marks: Option<Vec<RawMark>>,
    #[serde(default)]
    version: Option<u64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMark {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    attrs: Option<serde_json::Map<String, Value>>,
}

struct Budget {
    nodes: usize,
    images: usize,
}

/// Parses and validates a stored or submitted document.
pub fn parse(value: &Value) -> Result<Document, DocumentError> {
    // Bounded before any deep work.
    let bytes = serde_json::to_vec(value).map_err(|_| err("is not a document"))?.len();
    if bytes > MAX_BYTES {
        return Err(err(format!("may not be larger than {} KiB", MAX_BYTES / 1024)));
    }
    if depth(value, 0) > MAX_DEPTH * 2 + 4 {
        return Err(err("is nested too deeply"));
    }
    let raw: RawNode = serde_json::from_value(value.clone()).map_err(|_| err("is not a valid document"))?;
    if raw.kind != "doc"
        || raw.version != Some(VERSION)
        || raw.attrs.is_some()
        || raw.text.is_some()
        || raw.marks.is_some()
    {
        return Err(err("is not a valid document (version 1)"));
    }
    let mut budget = Budget { nodes: 0, images: 0 };
    let content = raw.content.unwrap_or_default();
    // List items only inside lists (as the editor's schema requires).
    if content.iter().any(|n| n.kind == "listItem") {
        return Err(err("has a list item outside a list"));
    }
    let content = blocks(content, &mut budget, 1)?;
    Ok(Document { content })
}

/// JSON nesting depth (arrays and objects).
fn depth(value: &Value, level: usize) -> usize {
    match value {
        Value::Array(items) => items.iter().map(|v| depth(v, level + 1)).max().unwrap_or(level + 1),
        Value::Object(map) => map.values().map(|v| depth(v, level + 1)).max().unwrap_or(level + 1),
        _ => level,
    }
}

fn count(budget: &mut Budget, level: usize) -> Result<(), DocumentError> {
    budget.nodes += 1;
    if budget.nodes > MAX_NODES {
        return Err(err(format!("may not have more than {MAX_NODES} elements")));
    }
    if level > MAX_DEPTH {
        return Err(err("is nested too deeply"));
    }
    Ok(())
}

fn no_extras(raw: &RawNode, attrs: bool, text: bool, marks: bool, content: bool) -> Result<(), DocumentError> {
    let ok = (attrs || raw.attrs.is_none())
        && (text || raw.text.is_none())
        && (marks || raw.marks.is_none())
        && (content || raw.content.is_none())
        && raw.version.is_none();
    if ok { Ok(()) } else { Err(err(format!("has unexpected properties on `{}`", raw.kind))) }
}

fn blocks(raw: Vec<RawNode>, budget: &mut Budget, level: usize) -> Result<Vec<Node>, DocumentError> {
    raw.into_iter().map(|node| block(node, budget, level)).collect()
}

fn block(raw: RawNode, budget: &mut Budget, level: usize) -> Result<Node, DocumentError> {
    count(budget, level)?;
    Ok(match raw.kind.as_str() {
        "paragraph" => {
            no_extras(&raw, false, false, false, true)?;
            Node::Paragraph(inlines(raw.content.unwrap_or_default(), budget, level + 1)?)
        }
        "heading" => {
            no_extras(&raw, true, false, false, true)?;
            let attrs = raw.attrs.clone().unwrap_or_default();
            let level_attr = attrs.get("level").and_then(Value::as_u64).filter(|l| (2..=4).contains(l));
            let Some(heading) = level_attr.filter(|_| attrs.len() == 1) else {
                return Err(err("has a heading without a level from 2 to 4"));
            };
            Node::Heading(heading as u8, inlines(raw.content.unwrap_or_default(), budget, level + 1)?)
        }
        "bulletList" | "orderedList" => {
            no_extras(&raw, false, false, false, true)?;
            let kind = raw.kind.clone();
            let items = raw.content.unwrap_or_default();
            if items.iter().any(|i| i.kind != "listItem") {
                return Err(err("has a list holding something else than list items"));
            }
            let items = blocks(items, budget, level + 1)?;
            if kind == "bulletList" { Node::BulletList(items) } else { Node::OrderedList(items) }
        }
        "listItem" | "blockquote" => {
            no_extras(&raw, false, false, false, true)?;
            let kind = raw.kind.clone();
            let items = raw.content.unwrap_or_default();
            if items.iter().any(|n| n.kind == "listItem") {
                return Err(err("has a list item outside a list"));
            }
            let items = blocks(items, budget, level + 1)?;
            if kind == "listItem" { Node::ListItem(items) } else { Node::Blockquote(items) }
        }
        "codeBlock" => {
            no_extras(&raw, false, false, false, true)?;
            let mut text = String::new();
            for part in raw.content.unwrap_or_default() {
                match part {
                    RawNode { kind, text: Some(t), marks: None, attrs: None, content: None, version: None }
                        if kind == "text" =>
                    {
                        count(budget, level + 1)?;
                        text.push_str(&t);
                    }
                    _ => return Err(err("has a code block holding something else than plain text")),
                }
            }
            Node::CodeBlock(text)
        }
        "horizontalRule" => {
            no_extras(&raw, false, false, false, false)?;
            Node::HorizontalRule
        }
        "image" => image(raw, budget)?,
        other => return Err(err(format!("has an unknown block `{}`", other.escape_debug()))),
    })
}

fn image(raw: RawNode, budget: &mut Budget) -> Result<Node, DocumentError> {
    no_extras(&raw, true, false, false, false)?;
    budget.images += 1;
    if budget.images > MAX_IMAGES {
        return Err(err(format!("may not have more than {MAX_IMAGES} images")));
    }
    let attrs = raw.attrs.unwrap_or_default();
    if attrs.keys().any(|k| k != "media" && k != "alt") {
        return Err(err("has an image with unexpected attributes"));
    }
    let media = attrs
        .get("media")
        .and_then(Value::as_str)
        .and_then(|id| uuid::Uuid::parse_str(id).ok())
        .ok_or_else(|| err("has an image that is not a media item"))?;
    let alt = match attrs.get("alt") {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(alt)) if alt.chars().count() <= MAX_ALT => alt.clone(),
        _ => return Err(err(format!("has an image description longer than {MAX_ALT} characters"))),
    };
    Ok(Node::Image { media, alt })
}

fn inlines(raw: Vec<RawNode>, budget: &mut Budget, level: usize) -> Result<Vec<Node>, DocumentError> {
    let mut out = Vec::with_capacity(raw.len());
    for node in raw {
        count(budget, level)?;
        out.push(match node.kind.as_str() {
            "text" => {
                no_extras(&node, false, true, true, false)?;
                let text = node.text.ok_or_else(|| err("has a text without text"))?;
                if text.is_empty() {
                    return Err(err("has an empty text"));
                }
                let raw_marks = node.marks.unwrap_or_default();
                if raw_marks.len() > MAX_MARKS {
                    return Err(err("has a text with too many marks"));
                }
                let marks: Vec<Mark> = raw_marks.into_iter().map(mark).collect::<Result<_, _>>()?;
                let kinds: std::collections::HashSet<_> = marks.iter().map(std::mem::discriminant).collect();
                if kinds.len() != marks.len() {
                    return Err(err("has a text with the same mark twice"));
                }
                Node::Text(text, marks)
            }
            "hardBreak" => {
                no_extras(&node, false, false, false, false)?;
                Node::HardBreak
            }
            // Images are blocks (the editor's schema): never inside text.
            other => return Err(err(format!("has an unknown inline `{}`", other.escape_debug()))),
        });
    }
    Ok(out)
}

fn mark(raw: RawMark) -> Result<Mark, DocumentError> {
    let plain = |mark: Mark| if raw.attrs.is_none() { Ok(mark) } else { Err(err("has a mark with attributes")) };
    match raw.kind.as_str() {
        "bold" => plain(Mark::Bold),
        "italic" => plain(Mark::Italic),
        "strike" => plain(Mark::Strike),
        "code" => plain(Mark::Code),
        "link" => {
            let attrs = raw.attrs.unwrap_or_default();
            if attrs.keys().any(|k| k != "href") {
                return Err(err("has a link with unexpected attributes"));
            }
            let href = attrs.get("href").and_then(Value::as_str).unwrap_or_default();
            if safe_href(href) {
                Ok(Mark::Link(href.to_owned()))
            } else {
                Err(err("has a link that is not http, https or mailto"))
            }
        }
        other => Err(err(format!("has an unknown mark `{}`", other.escape_debug()))),
    }
}

/// `http(s)://` or `mailto:`, no whitespace or control characters, bounded.
pub fn safe_href(href: &str) -> bool {
    let lower = href.to_ascii_lowercase();
    href.len() <= MAX_HREF
        && !href.chars().any(|c| c.is_whitespace() || c.is_control())
        && (lower.starts_with("https://") || lower.starts_with("http://") || lower.starts_with("mailto:"))
        && href.len() > "mailto:".len()
}

/// Media items the document's images reference, in order, without repeats.
pub fn media_ids(doc: &Document) -> Vec<uuid::Uuid> {
    fn walk(nodes: &[Node], out: &mut Vec<uuid::Uuid>) {
        for node in nodes {
            match node {
                Node::Image { media, .. } if !out.contains(media) => out.push(*media),
                Node::Paragraph(c)
                | Node::Heading(_, c)
                | Node::BulletList(c)
                | Node::OrderedList(c)
                | Node::ListItem(c)
                | Node::Blockquote(c) => walk(c, out),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(&doc.content, &mut out);
    out
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// HTML for a document, every text and attribute escaped. `media_url` maps an
/// image's media id to its URL (images whose URL is `None` are left out).
pub fn to_html(doc: &Document, media_url: &dyn Fn(uuid::Uuid) -> Option<String>) -> String {
    fn inner(nodes: &[Node], media_url: &dyn Fn(uuid::Uuid) -> Option<String>, out: &mut String) {
        for node in nodes {
            let wrap = |tag: &str, children: &[Node], out: &mut String| {
                out.push_str(&format!("<{tag}>"));
                inner(children, media_url, out);
                out.push_str(&format!("</{tag}>"));
            };
            match node {
                Node::Paragraph(c) => wrap("p", c, out),
                Node::Heading(level, c) => wrap(&format!("h{level}"), c, out),
                Node::BulletList(c) => wrap("ul", c, out),
                Node::OrderedList(c) => wrap("ol", c, out),
                Node::ListItem(c) => wrap("li", c, out),
                Node::Blockquote(c) => wrap("blockquote", c, out),
                Node::CodeBlock(text) => out.push_str(&format!("<pre><code>{}</code></pre>", escape(text))),
                Node::HorizontalRule => out.push_str("<hr>"),
                Node::HardBreak => out.push_str("<br>"),
                Node::Image { media, alt } => {
                    if let Some(url) = media_url(*media) {
                        out.push_str(&format!("<img src=\"{}\" alt=\"{}\">", escape(&url), escape(alt)));
                    }
                }
                Node::Text(text, marks) => {
                    let mut html = escape(text);
                    for mark in marks {
                        html = match mark {
                            Mark::Bold => format!("<strong>{html}</strong>"),
                            Mark::Italic => format!("<em>{html}</em>"),
                            Mark::Strike => format!("<s>{html}</s>"),
                            Mark::Code => format!("<code>{html}</code>"),
                            Mark::Link(href) => {
                                format!("<a href=\"{}\" rel=\"noopener noreferrer nofollow\">{html}</a>", escape(href))
                            }
                        };
                    }
                    out.push_str(&html);
                }
            }
        }
    }
    let mut out = String::new();
    inner(&doc.content, media_url, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn doc(content: Value) -> Value {
        json!({"type": "doc", "version": 1, "content": content})
    }

    #[test]
    fn valid_documents_round_trip_to_escaped_html() {
        let media = "0199d3a4-1b2c-7d3e-8f40-123456789abc";
        let value = doc(json!([
            {"type": "heading", "attrs": {"level": 2}, "content": [{"type": "text", "text": "Title <script>"}]},
            {"type": "paragraph", "content": [
                {"type": "text", "text": "a \"b\" & 'c'", "marks": [{"type": "bold"}, {"type": "link", "attrs": {"href": "https://x.test/?a=1&b=\"2\""}}]},
                {"type": "hardBreak"}
            ]},
            {"type": "bulletList", "content": [{"type": "listItem", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "i"}]}]}]},
            {"type": "codeBlock", "content": [{"type": "text", "text": "<b>x</b>"}]},
            {"type": "image", "attrs": {"media": media, "alt": "\" onerror=\"x"}},
            {"type": "horizontalRule"}
        ]));
        let parsed = parse(&value).unwrap();
        assert_eq!(media_ids(&parsed), vec![uuid::Uuid::parse_str(media).unwrap()]);
        let html = to_html(&parsed, &|id| Some(format!("/media/{id}")));
        assert_eq!(
            html,
            "<h2>Title &lt;script&gt;</h2><p><a href=\"https://x.test/?a=1&amp;b=&quot;2&quot;\" rel=\"noopener noreferrer nofollow\">\
             <strong>a &quot;b&quot; &amp; &#39;c&#39;</strong></a><br></p><ul><li><p>i</p></li></ul>\
             <pre><code>&lt;b&gt;x&lt;/b&gt;</code></pre><img src=\"/media/0199d3a4-1b2c-7d3e-8f40-123456789abc\" alt=\"&quot; onerror=&quot;x\"><hr>"
        );
        assert!(!to_html(&parsed, &|_| None).contains("<img"));
    }

    #[test]
    fn anything_else_is_refused() {
        let text = |t: &str| json!({"type": "text", "text": t});
        let para = |inline: Value| json!({"type": "paragraph", "content": [inline]});
        for bad in [
            json!({"type": "doc", "content": []}),
            json!({"type": "doc", "version": 2, "content": []}),
            json!("<p>html</p>"),
            doc(json!([{"type": "script", "content": []}])),
            doc(json!([{"type": "paragraph", "attrs": {"style": "x"}, "content": []}])),
            doc(json!([para(
                json!({"type": "text", "text": "x", "marks": [{"type": "link", "attrs": {"href": "javascript:alert(1)"}}]})
            )])),
            doc(json!([para(
                json!({"type": "text", "text": "x", "marks": [{"type": "link", "attrs": {"href": "data:text/html,x"}}]})
            )])),
            doc(json!([para(
                json!({"type": "text", "text": "x", "marks": [{"type": "link", "attrs": {"href": "https://a b"}}]})
            )])),
            doc(json!([para(
                json!({"type": "text", "text": "x", "marks": [{"type": "link", "attrs": {"href": "https://x", "target": "_blank"}}]})
            )])),
            doc(json!([para(json!({"type": "text", "text": "x", "marks": [{"type": "underline"}]}))])),
            doc(json!([para(json!({"type": "text", "text": "", "marks": []}))])),
            doc(json!([para(json!({"type": "text", "text": "x", "onclick": "y"}))])),
            doc(json!([{"type": "heading", "attrs": {"level": 1}, "content": [text("x")]}])),
            doc(json!([{"type": "image", "attrs": {"src": "https://evil.test/x.png"}}])),
            doc(json!([{"type": "image", "attrs": {"media": "not-a-uuid"}}])),
            doc(json!([{"type": "bulletList", "content": [para(text("x"))]}])),
            doc(
                json!([{"type": "codeBlock", "content": [{"type": "text", "text": "x", "marks": [{"type": "bold"}]}]}]),
            ),
            doc(json!([para(json!({"type": "text", "text": "x", "marks": [{"type": "bold"}, {"type": "bold"}]}))])),
            doc(json!([para(json!({"type": "image", "attrs": {"media": "0199d3a4-1b2c-7d3e-8f40-123456789abc"}}))])),
            doc(json!([{"type": "listItem", "content": [para(text("x"))]}])),
            json!({"type": "doc", "version": 1, "content": [], "marks": [{"type": "evil", "attrs": {"x": 1}}]}),
        ] {
            assert!(parse(&bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn documents_are_bounded() {
        let many: Vec<Value> = (0..10_001).map(|_| json!({"type": "horizontalRule"})).collect();
        assert!(parse(&doc(Value::Array(many))).is_err());
        let mut nested = json!({"type": "paragraph", "content": [{"type": "text", "text": "x"}]});
        for _ in 0..20 {
            nested = json!({"type": "blockquote", "content": [nested]});
        }
        assert!(parse(&doc(json!([nested]))).is_err());
        let marks: Vec<Value> = (0..15_000).map(|_| json!({"type": "bold"})).collect();
        assert!(
            parse(&doc(json!([{"type": "paragraph", "content": [{"type": "text", "text": "x", "marks": marks}]}])))
                .is_err()
        );
        let big = "x".repeat(MAX_BYTES);
        assert!(parse(&doc(json!([{"type": "paragraph", "content": [{"type": "text", "text": big}]}]))).is_err());
    }
}
