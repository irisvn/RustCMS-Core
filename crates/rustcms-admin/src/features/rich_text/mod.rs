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

mod html;
mod parse;

pub use html::*;
pub use parse::*;

use serde::Deserialize;
use serde_json::Value;

/// Document format version.
pub const VERSION: u64 = 1;
/// Largest stored document (bytes of JSON).
pub const MAX_BYTES: usize = 256 * 1024;

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
