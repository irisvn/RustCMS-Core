//! Export as escaped HTML (headless consumers).

use super::*;

pub(super) fn escape(text: &str) -> String {
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
