//! Translations of the admin interface (ADR 0013).
//!
//! - **Locale** of a request: the user's saved choice when supported, else the
//!   best `Accept-Language` match, else `app.locale` ([`negotiate`]).
//! - **Catalogs** map English source strings to translations, optionally
//!   scoped by resource (`[posts] "Title" = "…"`), and are compiled into the
//!   binary ([`Translator`]).
//! - **Validation messages** stay English on the wire and carry a stable
//!   code ([`Issue`]) that the panel translates.

mod catalog;
mod issues;

pub use catalog::{Tr, Translator};
pub use issues::{CUSTOM, Issue, TEMPLATES};

/// The locale to use: `preferred` (a user's saved choice) when supported, else
/// the best supported `Accept-Language` entry, else `default`.
pub fn negotiate<'a>(
    preferred: Option<&str>,
    accept_language: Option<&str>,
    supported: &'a [String],
    default: &'a str,
) -> &'a str {
    if let Some(found) = preferred.and_then(|p| find(supported, p)) {
        return found;
    }
    let mut ranked: Vec<(f32, usize, &str)> = Vec::new();
    for (i, entry) in accept_language.unwrap_or_default().split(',').take(20).enumerate() {
        let mut parts = entry.split(';');
        let tag = parts.next().unwrap_or_default().trim();
        let q = parts
            .find_map(|p| p.trim().strip_prefix("q="))
            .map_or(Some(1.0), |q| q.trim().parse::<f32>().ok())
            .unwrap_or(0.0);
        if q > 0.0 && !tag.is_empty() && tag != "*" {
            ranked.push((q, i, tag));
        }
    }
    // Highest quality first; ties keep the client's order.
    ranked.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    for (_, _, tag) in ranked {
        let primary = tag.split(['-', '_']).next().unwrap_or(tag);
        if let Some(found) = find(supported, tag).or_else(|| find(supported, primary)) {
            return found;
        }
    }
    default
}

fn find<'a>(supported: &'a [String], tag: &str) -> Option<&'a str> {
    supported.iter().find(|s| s.eq_ignore_ascii_case(tag)).map(String::as_str)
}

/// Whether `locale` is a well-formed tag (`en`, `vi`, `pt-BR`): letters,
/// digits and `-`, at most 35 characters.
pub fn is_locale_tag(locale: &str) -> bool {
    !locale.is_empty()
        && locale.len() <= 35
        && locale.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        && locale.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn supported() -> Vec<String> {
        vec!["en".into(), "vi".into()]
    }

    #[test]
    fn saved_choice_then_header_then_default() {
        let s = supported();
        assert_eq!(negotiate(Some("vi"), Some("en"), &s, "en"), "vi");
        assert_eq!(negotiate(Some("fr"), Some("vi-VN,en;q=0.5"), &s, "en"), "vi");
        assert_eq!(negotiate(None, Some("fr, en;q=0.1, vi;q=0.9"), &s, "en"), "vi");
        assert_eq!(negotiate(None, Some("fr, de"), &s, "vi"), "vi");
        assert_eq!(negotiate(None, Some("vi;q=0, en;q=0.2"), &s, "vi"), "en");
        assert_eq!(negotiate(None, Some("VI"), &s, "en"), "vi");
        assert_eq!(negotiate(None, Some(";;;,q=,*"), &s, "en"), "en");
        assert_eq!(negotiate(None, None, &s, "en"), "en");
    }

    #[test]
    fn locale_tags() {
        for ok in ["en", "vi", "pt-BR", "zh-Hant"] {
            assert!(is_locale_tag(ok), "{ok}");
        }
        for bad in ["", "1en", "en_US", "e n", "<x>", &"a".repeat(36)] {
            assert!(!is_locale_tag(bad), "{bad}");
        }
    }
}
