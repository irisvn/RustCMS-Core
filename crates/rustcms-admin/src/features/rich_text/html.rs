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
///
/// - Tables: `<table>`, the header row in `<thead>` with `<th>` cells.
/// - Embeds: `<figure class="embed" data-provider="youtube" data-id="…"><a href="…">…</a></figure>`,
///   a link to the provider's page (no iframe, no script): frontends may
///   replace it by the provider's player.
/// - Callouts: `<aside class="callout" data-tone="info">`.
pub fn to_html(doc: &Document, media_url: &dyn Fn(uuid::Uuid) -> Option<String>) -> String {
    let mut out = String::new();
    blocks(&doc.content, media_url, &mut out);
    out
}

type Images<'a> = &'a dyn Fn(uuid::Uuid) -> Option<String>;

fn wrap(open: &str, close: &str, children: &[Node], image: Images, out: &mut String) {
    out.push_str(open);
    blocks(children, image, out);
    out.push_str(close);
}

fn blocks(nodes: &[Node], image: Images, out: &mut String) {
    for node in nodes {
        match node {
            Node::Paragraph(c) => wrap("<p>", "</p>", c, image, out),
            Node::Heading(level, c) => wrap(&format!("<h{level}>"), &format!("</h{level}>"), c, image, out),
            Node::BulletList(c) => wrap("<ul>", "</ul>", c, image, out),
            Node::OrderedList(c) => wrap("<ol>", "</ol>", c, image, out),
            Node::ListItem(c) => wrap("<li>", "</li>", c, image, out),
            Node::Blockquote(c) => wrap("<blockquote>", "</blockquote>", c, image, out),
            Node::Callout(tone, c) => {
                let open = format!("<aside class=\"callout\" data-tone=\"{}\">", tone.as_str());
                wrap(&open, "</aside>", c, image, out);
            }
            Node::Table(table) => {
                out.push_str("<table>");
                let (head, body) = table.rows.split_at(usize::from(table.header));
                for (tag, cell, rows) in [("thead", "th", head), ("tbody", "td", body)] {
                    if rows.is_empty() {
                        continue;
                    }
                    out.push_str(&format!("<{tag}>"));
                    for row in rows {
                        out.push_str("<tr>");
                        for c in row {
                            wrap(&format!("<{cell}>"), &format!("</{cell}>"), &c.content, image, out);
                        }
                        out.push_str("</tr>");
                    }
                    out.push_str(&format!("</{tag}>"));
                }
                out.push_str("</table>");
            }
            Node::CodeBlock(text) => out.push_str(&format!("<pre><code>{}</code></pre>", escape(text))),
            Node::HorizontalRule => out.push_str("<hr>"),
            Node::HardBreak => out.push_str("<br>"),
            Node::Image(img) => {
                if let Some(url) = image(img.media) {
                    out.push_str(&format!("<img src=\"{}\" alt=\"{}\">", escape(&url), escape(&img.alt)));
                }
            }
            Node::Embed(embed) => {
                // Provider names and ids are checked formats; escaped all the same.
                let url = escape(&embed.url());
                out.push_str(&format!(
                    "<figure class=\"embed\" data-provider=\"{}\" data-id=\"{}\">\
                     <a href=\"{url}\" rel=\"noopener noreferrer nofollow\">{url}</a></figure>",
                    embed.provider.as_str(),
                    escape(&embed.id),
                ));
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
