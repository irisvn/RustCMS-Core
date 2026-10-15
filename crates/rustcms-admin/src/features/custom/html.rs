//! The second layer under templates: every rendered page and slot goes
//! through an explicit allowlist (ammonia) before it reaches the panel. No
//! script, style, event handler, form, frame, `id` or `name` survives; URLs
//! must stay in the panel (or be `https:` / `mailto:` links).

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

/// Elements a template may produce.
const TAGS: &[&str] = &[
    "a",
    "abbr",
    "article",
    "b",
    "blockquote",
    "br",
    "caption",
    "code",
    "dd",
    "details",
    "div",
    "dl",
    "dt",
    "em",
    "figcaption",
    "figure",
    "footer",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hr",
    "i",
    "img",
    "li",
    "mark",
    "meter",
    "ol",
    "p",
    "pre",
    "progress",
    "s",
    "section",
    "small",
    "span",
    "strong",
    "sub",
    "summary",
    "sup",
    "table",
    "tbody",
    "td",
    "tfoot",
    "th",
    "thead",
    "time",
    "tr",
    "u",
    "ul",
];

/// Attributes per element (besides `class`, `title`, `role`, `aria-*`, `data-*`).
const TAG_ATTRIBUTES: &[(&str, &[&str])] = &[
    ("a", &["href"]),
    ("img", &["src", "alt", "width", "height"]),
    ("td", &["colspan", "rowspan"]),
    ("th", &["colspan", "rowspan", "scope"]),
    ("time", &["datetime"]),
    ("progress", &["value", "max"]),
    ("meter", &["value", "min", "max", "low", "high", "optimum"]),
    ("ol", &["start"]),
    ("details", &["open"]),
];

/// Cleans `html` for the panel served under `base` (`/admin`).
pub(crate) struct Sanitizer {
    builder: ammonia::Builder<'static>,
}

impl Sanitizer {
    pub(crate) fn new(base: &str) -> Self {
        let base = base.trim_end_matches('/').to_owned();
        let mut builder = ammonia::Builder::empty();
        let tag_attributes: HashMap<&'static str, HashSet<&'static str>> =
            TAG_ATTRIBUTES.iter().map(|(tag, attrs)| (*tag, attrs.iter().copied().collect())).collect();
        builder
            .tags(TAGS.iter().copied().collect())
            .clean_content_tags(["script", "style", "template", "noscript"].into_iter().collect())
            .tag_attributes(tag_attributes)
            .generic_attributes(["class", "title", "role"].into_iter().collect())
            .generic_attribute_prefixes(["aria-", "data-"].into_iter().collect())
            .url_schemes(["https", "mailto"].into_iter().collect())
            .url_relative(ammonia::UrlRelative::PassThrough)
            .link_rel(Some("noopener noreferrer"))
            .strip_comments(true)
            .attribute_filter(move |element, attribute, value| match (element, attribute) {
                ("a", "href") => safe_href(&base, value).then_some(Cow::Borrowed(value)),
                ("img", "src") => safe_src(&base, value).then_some(Cow::Borrowed(value)),
                _ => Some(Cow::Borrowed(value)),
            });
        Self { builder }
    }

    pub(crate) fn clean(&self, html: &str) -> String {
        self.builder.clean(html).to_string()
    }
}

/// No controls, spaces or backslashes anywhere (`java\tscript:`, `/\evil`).
fn plain(value: &str) -> bool {
    !value.is_empty() && value.chars().all(|c| !c.is_control() && !c.is_whitespace() && c != '\\')
}

/// A path of the panel under `base` (segment boundary), not of its API or
/// assets, with no traversal (also percent-encoded) and no `//`.
fn panel_path(base: &str, value: &str) -> bool {
    let Some(rest) = value.strip_prefix(base) else { return false };
    if !(rest.is_empty() || rest.starts_with(['/', '?', '#'])) {
        return false;
    }
    let path = rest.split(['?', '#']).next().unwrap_or_default();
    let lower = path.to_ascii_lowercase();
    !path.contains("//")
        && !lower.contains("%2e")
        && !lower.contains("%2f")
        && !lower.contains("%5c")
        && !path.split('/').any(|segment| segment == "." || segment == "..")
}

/// A link: in the panel (not its API or assets), or `https://host…`, or `mailto:`.
pub(crate) fn safe_href(base: &str, value: &str) -> bool {
    if !plain(value) {
        return false;
    }
    if let Some(rest) = value.strip_prefix("https://") {
        return rest.split(['/', '?', '#']).next().is_some_and(|host| !host.is_empty() && !host.contains('@'));
    }
    if value.starts_with("mailto:") {
        return value.len() > "mailto:".len();
    }
    panel_path(base, value)
        && !value.starts_with(&format!("{base}/api/"))
        && !value.starts_with(&format!("{base}/assets/"))
}

/// An image of the panel: a media file or a panel asset (never `data:` or another site).
pub(crate) fn safe_src(base: &str, value: &str) -> bool {
    plain(value)
        && panel_path(base, value)
        && (value.starts_with(&format!("{base}/api/media/")) || value.starts_with(&format!("{base}/assets/")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clean(html: &str) -> String {
        Sanitizer::new("/admin").clean(html)
    }

    #[test]
    fn scripts_styles_handlers_and_frames_never_survive() {
        for bad in [
            "<script>alert(1)</script>",
            "<style>body{display:none}</style>",
            "<img src=/admin/assets/a.png onerror=alert(1)>",
            "<div style=\"color:red\" onclick=\"x()\">x</div>",
            "<iframe src=/admin></iframe>",
            "<form action=/admin><input name=q><button>go</button></form>",
            "<svg><script>alert(1)</script></svg>",
            "<math><mi>x</mi></math>",
            "<object data=x></object><embed src=x>",
            "<template><img src=x onerror=alert(1)></template>",
            "<noscript><p title=\"</noscript><img src=x onerror=alert(1)>\"></noscript>",
        ] {
            let out = clean(bad);
            for marker in [
                "<script", "<style", "onerror", "onclick", "style=", "<iframe", "<form", "<input", "<svg", "<object",
                "<embed", "<math",
            ] {
                assert!(!out.contains(marker), "{bad} → {out}");
            }
        }
    }

    #[test]
    fn ids_and_names_are_dropped_to_prevent_clobbering() {
        assert_eq!(clean("<div id=\"location\" name=\"x\" class=\"c\">t</div>"), "<div class=\"c\">t</div>");
    }

    #[test]
    fn links_stay_in_the_panel_or_are_https_or_mailto() {
        for (href, kept) in [
            ("/admin/r/courses", true),
            ("/admin", true),
            ("/admin/p/overview?tab=1#top", true),
            ("https://example.test/docs", true),
            ("mailto:staff@example.test", true),
            ("/admin-evil/x", false),
            ("/admin/api/queue", false),
            ("/admin/assets/x.js", false),
            ("//evil.test", false),
            ("/\\evil.test", false),
            ("/admin/../evil", false),
            ("/admin/%2e%2e/evil", false),
            ("/admin//evil.test", false),
            ("javascript:alert(1)", false),
            ("java&#9;script:alert(1)", false),
            (" javascript:alert(1)", false),
            ("JAVASCRIPT:alert(1)", false),
            ("http://example.test", false),
            ("https://user@evil.test", false),
            ("https://", false),
            ("data:text/html,<script>alert(1)</script>", false),
            ("evil.test", false),
        ] {
            let out = clean(&format!("<a href=\"{href}\">x</a>"));
            assert_eq!(out.contains("href"), kept, "{href} → {out}");
        }
    }

    #[test]
    fn images_come_from_the_panel_only() {
        for (src, kept) in [
            ("/admin/api/media/0199/file", true),
            ("/admin/assets/ext/logo.1234.png", true),
            ("/admin/r/x", false),
            ("https://evil.test/x.png", false),
            ("data:image/svg+xml,<svg onload=alert(1)>", false),
            ("data:image/png;base64,AAAA", false),
            ("/admin/api/media/../../x", false),
        ] {
            let out = clean(&format!("<img src=\"{src}\" alt=\"a\">"));
            assert_eq!(out.contains("src="), kept, "{src} → {out}");
        }
    }

    #[test]
    fn allowed_structure_and_attributes_are_kept() {
        let html = "<section class=\"rc-card\" data-x=\"1\" aria-label=\"Seats\"><meter value=\"3\" max=\"10\"></meter>\
                    <table><tr><th scope=\"col\">A</th></tr></table><a href=\"/admin/r/courses\">C</a></section>";
        let out = clean(html);
        for kept in [
            "class=\"rc-card\"",
            "data-x=\"1\"",
            "aria-label=\"Seats\"",
            "<meter value=\"3\" max=\"10\">",
            "scope=\"col\"",
            "rel=\"noopener noreferrer\"",
        ] {
            assert!(out.contains(kept), "{kept} missing in {out}");
        }
    }
}
