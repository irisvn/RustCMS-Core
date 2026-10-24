//! Headings: stable ids and the document's outline (a table of contents).

use std::collections::{HashMap, HashSet};

use unicode_normalization::UnicodeNormalization;

use super::*;

/// Longest heading id before deduplication (characters).
pub const MAX_SLUG: usize = 80;

/// A heading of the document, in document order.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Heading {
    /// 2 to 4.
    pub level: u8,
    /// Its text (marks dropped, line breaks as spaces, trimmed).
    pub text: String,
    /// Its id in [`to_html`]'s output: unique in the document.
    pub id: String,
}

/// The document's headings with the ids [`to_html`] gives them: the
/// [`slug`] of their text, `-2`, `-3`, … added to repeats (`section` for a
/// heading without letters or digits). Ids depend only on the headings'
/// texts and order, so they stay the same across renders.
pub fn outline(doc: &Document) -> Vec<Heading> {
    let mut used = HashSet::new();
    // The last suffix tried for each base: every smaller one is taken (the
    // set only grows), so repeats cost the same however many there are.
    let mut next: HashMap<String, usize> = HashMap::new();
    let mut out = Vec::new();
    walk(&doc.content, &mut |node| {
        let Node::Heading(level, children) = node else { return };
        let mut text = String::new();
        walk(children, &mut |inline| match inline {
            Node::Text(t, _) => text.push_str(t),
            Node::HardBreak => text.push(' '),
            _ => {}
        });
        let text = text.trim().to_owned();
        let base = Some(slug(&text)).filter(|s| !s.is_empty()).unwrap_or_else(|| "section".to_owned());
        let n = next.entry(base.clone()).or_insert(1);
        let mut id = if *n == 1 { base.clone() } else { format!("{base}-{n}") };
        while !used.insert(id.clone()) {
            *n += 1;
            id = format!("{base}-{n}");
        }
        out.push(Heading { level: *level, text, id });
    });
    out
}

/// `text` as an id: diacritics folded (`Tiếng Việt` → `tieng-viet`,
/// `Đà Nẵng` → `da-nang`), lower case, letters and digits kept, every other
/// run of characters a single `-`, none at the ends; at most [`MAX_SLUG`]
/// characters. ASCII only (deterministic): letters without a Latin form
/// (`日本`) are separators, like punctuation.
pub fn slug(text: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    let mut len = 0;
    for c in text.to_lowercase().nfd().flat_map(fold) {
        if c.is_ascii_alphanumeric() {
            let separate = dash && !out.is_empty();
            if len + usize::from(separate) >= MAX_SLUG {
                break;
            }
            if separate {
                out.push('-');
                len += 1;
            }
            out.push(c);
            len += 1;
            dash = false;
        } else {
            dash = true;
        }
    }
    out
}

/// A decomposed character without its diacritics: combining marks dropped,
/// letters NFD leaves alone (đ, ø, ł, ß, æ, œ) spelled in ASCII.
fn fold(c: char) -> impl Iterator<Item = char> {
    let combining = matches!(c, '\u{0300}'..='\u{036F}'
        | '\u{1AB0}'..='\u{1AFF}'
        | '\u{1DC0}'..='\u{1DFF}'
        | '\u{20D0}'..='\u{20FF}'
        | '\u{FE20}'..='\u{FE2F}');
    let spelled: &'static str = match c {
        'đ' | 'Đ' => "d",
        'ø' | 'Ø' => "o",
        'ł' | 'Ł' => "l",
        'ß' => "ss",
        'æ' | 'Æ' => "ae",
        'œ' | 'Œ' => "oe",
        _ => "",
    };
    let keep = (!combining && spelled.is_empty()).then_some(c);
    keep.into_iter().chain(spelled.chars())
}
