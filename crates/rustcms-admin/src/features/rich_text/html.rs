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

/// What [`to_html_with`] needs to show an image: its URL, and its size when
/// known (the media row's `width` and `height`), so the browser reserves its
/// space before it loads.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ImageInfo {
    /// The image's URL (escaped when written).
    pub url: String,
    /// Width in pixels.
    pub width: Option<u32>,
    /// Height in pixels.
    pub height: Option<u32>,
}

impl ImageInfo {
    /// An image at `url`, size unknown.
    pub fn new(url: impl Into<String>) -> Self {
        Self { url: url.into(), width: None, height: None }
    }

    /// The same image, `width` × `height` pixels.
    pub fn size(mut self, width: u32, height: u32) -> Self {
        self.width = Some(width);
        self.height = Some(height);
        self
    }
}

/// HTML for a document, every text and attribute escaped. `media_url` maps an
/// image's media id to its URL (images whose URL is `None` are left out). See
/// [`to_html_with`], which also writes images' sizes.
pub fn to_html(doc: &Document, media_url: &dyn Fn(uuid::Uuid) -> Option<String>) -> String {
    render(doc, &|id| media_url(id).map(ImageInfo::new))
}

/// HTML for a document, every text and attribute escaped. `images` maps an
/// image's media id to its URL and size (images missing from it are left
/// out): load the media rows of [`media_ids`] and build it.
///
/// - Headings get stable ids (see [`outline`]): `<h2 id="slug">`.
/// - Images: `<figure><img src width height alt><figcaption>…</figcaption></figure>`;
///   `data-width="wide"` or `"full"` on the figure for those widths; `width`
///   and `height` only when both are known; no `figcaption` without a caption.
/// - Tables: `<table>`, the header row in `<thead>` with `<th>` cells.
/// - Embeds: `<figure data-embed="youtube" data-id="…"><a href="…" rel="noopener noreferrer">…</a></figure>`,
///   a link to the provider's page (no iframe, no script): frontends may
///   replace it by the provider's player.
/// - Callouts: `<aside class="callout" data-tone="info">`.
pub fn to_html_with(doc: &Document, images: &std::collections::HashMap<uuid::Uuid, ImageInfo>) -> String {
    render(doc, &|id| images.get(&id).cloned())
}

fn render(doc: &Document, image: &dyn Fn(uuid::Uuid) -> Option<ImageInfo>) -> String {
    let mut ids = outline(doc).into_iter().map(|h| h.id);
    let mut out = String::new();
    blocks(&doc.content, image, &mut ids, &mut out);
    out
}

type Images<'a> = &'a dyn Fn(uuid::Uuid) -> Option<ImageInfo>;

fn wrap(
    open: &str,
    close: &str,
    children: &[Node],
    image: Images,
    ids: &mut dyn Iterator<Item = String>,
    out: &mut String,
) {
    out.push_str(open);
    blocks(children, image, ids, out);
    out.push_str(close);
}

fn blocks(nodes: &[Node], image: Images, ids: &mut dyn Iterator<Item = String>, out: &mut String) {
    for node in nodes {
        match node {
            Node::Paragraph(c) => wrap("<p>", "</p>", c, image, ids, out),
            Node::Heading(level, c) => {
                // Headings are numbered in the same order by `outline`.
                let id = ids.next().unwrap_or_default();
                wrap(&format!("<h{level} id=\"{}\">", escape(&id)), &format!("</h{level}>"), c, image, ids, out);
            }
            Node::BulletList(c) => wrap("<ul>", "</ul>", c, image, ids, out),
            Node::OrderedList(c) => wrap("<ol>", "</ol>", c, image, ids, out),
            Node::ListItem(c) => wrap("<li>", "</li>", c, image, ids, out),
            Node::Blockquote(c) => wrap("<blockquote>", "</blockquote>", c, image, ids, out),
            Node::Callout(tone, c) => {
                let open = format!("<aside class=\"callout\" data-tone=\"{}\">", tone.as_str());
                wrap(&open, "</aside>", c, image, ids, out);
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
                            wrap(&format!("<{cell}>"), &format!("</{cell}>"), &c.content, image, ids, out);
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
                let Some(info) = image(img.media) else { continue };
                out.push_str("<figure");
                if img.width != ImageWidth::Normal {
                    out.push_str(&format!(" data-width=\"{}\"", img.width.as_str()));
                }
                out.push_str(&format!("><img src=\"{}\"", escape(&info.url)));
                if let (Some(w), Some(h)) = (info.width, info.height) {
                    out.push_str(&format!(" width=\"{w}\" height=\"{h}\""));
                }
                out.push_str(&format!(" alt=\"{}\">", escape(&img.alt)));
                if !img.caption.is_empty() {
                    out.push_str(&format!("<figcaption>{}</figcaption>", escape(&img.caption)));
                }
                out.push_str("</figure>");
            }
            Node::Embed(embed) => {
                // Provider names and ids are checked formats; escaped all the same.
                let url = escape(&embed.url());
                out.push_str(&format!(
                    "<figure data-embed=\"{}\" data-id=\"{}\"><a href=\"{url}\" rel=\"noopener noreferrer\">{url}</a></figure>",
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
