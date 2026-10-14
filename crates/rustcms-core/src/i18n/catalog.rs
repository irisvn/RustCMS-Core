//! Translation catalogs keyed by English source strings.

use std::collections::{BTreeSet, HashMap};

use anyhow::{Context, bail, ensure};

/// One locale's strings: global, and per scope (a resource slug).
#[derive(Debug, Default, Clone)]
struct Catalog {
    global: HashMap<String, String>,
    scoped: HashMap<String, HashMap<String, String>>,
}

impl Catalog {
    fn parse(source: &str) -> anyhow::Result<Self> {
        let table: toml::Table = toml::from_str(source).context("not valid TOML")?;
        let mut catalog = Self::default();
        for (key, value) in table {
            match value {
                toml::Value::String(text) => {
                    check(&key, &text)?;
                    if !text.is_empty() {
                        catalog.global.insert(unescape(&key), unescape(&text));
                    }
                }
                toml::Value::Table(scope) => {
                    let strings = catalog.scoped.entry(key.clone()).or_default();
                    for (source, value) in scope {
                        let toml::Value::String(text) = value else {
                            bail!("[{key}] {source:?}: translations are strings");
                        };
                        check(&source, &text).with_context(|| format!("in [{key}]"))?;
                        if !text.is_empty() {
                            strings.insert(unescape(&source), unescape(&text));
                        }
                    }
                }
                _ => bail!("{key:?}: translations are strings, or tables of strings per resource"),
            }
        }
        Ok(catalog)
    }

    fn get(&self, scope: Option<&str>, source: &str) -> Option<&str> {
        scope
            .and_then(|s| self.scoped.get(s))
            .and_then(|strings| strings.get(source))
            .or_else(|| self.global.get(source))
            .map(String::as_str)
    }
}

/// A translation must keep the placeholders of its source (an empty one means
/// "not translated yet").
fn check(source: &str, translation: &str) -> anyhow::Result<()> {
    let expected = placeholders(source).with_context(|| format!("{source:?}"))?;
    if translation.is_empty() {
        return Ok(());
    }
    let found = placeholders(translation).with_context(|| format!("translation of {source:?}"))?;
    ensure!(expected == found, "translation of {source:?} must use the placeholders {expected:?}, not {found:?}");
    Ok(())
}

/// `{{` and `}}` as the braces they stand for (placeholders are kept).
fn unescape(text: &str) -> String {
    text.replace("{{", "{").replace("}}", "}")
}

/// The `{name}` placeholders of `text`; `{{` and `}}` are literal braces.
fn placeholders(text: &str) -> anyhow::Result<BTreeSet<&str>> {
    let mut found = BTreeSet::new();
    let mut rest = text;
    while let Some(i) = rest.find(['{', '}']) {
        let after = &rest[i + 1..];
        if rest[i..].starts_with("{{") || rest[i..].starts_with("}}") {
            rest = &after[1..];
            continue;
        }
        ensure!(rest.as_bytes()[i] == b'{', "unmatched `}}`");
        let end = after.find('}').context("unclosed `{`")?;
        let name = &after[..end];
        ensure!(
            !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
            "placeholder `{{{name}}}` is not a name"
        );
        found.insert(name);
        rest = &after[end + 1..];
    }
    Ok(found)
}

/// Every catalog of the application: its own, then the framework's, per locale.
///
/// Built at boot from strings compiled into the binary; a malformed catalog or
/// a translation with other placeholders than its source fails the boot.
#[derive(Debug, Default, Clone)]
pub struct Translator {
    app: HashMap<String, Catalog>,
    framework: HashMap<String, Vec<(String, Catalog)>>,
}

impl Translator {
    /// Adds the application's catalog for `locale` (once per locale).
    pub fn add(&mut self, locale: &str, toml: &str) -> anyhow::Result<()> {
        ensure!(super::is_locale_tag(locale), "invalid locale {locale:?}");
        ensure!(!self.app.contains_key(locale), "translations for {locale:?} are registered twice");
        let catalog = Catalog::parse(toml).with_context(|| format!("translations for {locale:?}"))?;
        self.app.insert(locale.to_owned(), catalog);
        Ok(())
    }

    /// Adds a framework catalog named `name` (a crate) for `locale`.
    pub fn add_framework(&mut self, name: &str, locale: &str, toml: &str) -> anyhow::Result<()> {
        ensure!(super::is_locale_tag(locale), "invalid locale {locale:?}");
        let layers = self.framework.entry(locale.to_owned()).or_default();
        ensure!(!layers.iter().any(|(n, _)| n == name), "{name} translations for {locale:?} are registered twice");
        let catalog = Catalog::parse(toml).with_context(|| format!("{name} translations for {locale:?}"))?;
        layers.push((name.to_owned(), catalog));
        Ok(())
    }

    /// Whether any catalog exists for `locale`.
    pub fn has(&self, locale: &str) -> bool {
        self.app.contains_key(locale) || self.framework.contains_key(locale)
    }

    /// The translation of `source` (scoped first, then global; the
    /// application's catalog before the framework's), or `source` itself.
    pub fn get<'a>(&'a self, locale: &str, scope: Option<&str>, source: &'a str) -> &'a str {
        self.find(locale, scope, source).unwrap_or(source)
    }

    /// The translation of `source`, if any catalog has one.
    pub fn find(&self, locale: &str, scope: Option<&str>, source: &str) -> Option<&str> {
        let app = self.app.get(locale).and_then(|c| c.get(scope, source));
        app.or_else(|| self.framework.get(locale)?.iter().find_map(|(_, c)| c.get(scope, source)))
    }

    /// A translator bound to one locale.
    pub fn tr<'a>(&'a self, locale: &'a str) -> Tr<'a> {
        Tr { translator: self, locale }
    }
}

/// A [`Translator`] bound to the locale of one request.
#[derive(Debug, Clone, Copy)]
pub struct Tr<'a> {
    translator: &'a Translator,
    locale: &'a str,
}

impl<'a> Tr<'a> {
    /// English (no catalog): returns sources unchanged.
    pub fn english() -> Tr<'static> {
        static EMPTY: std::sync::OnceLock<Translator> = std::sync::OnceLock::new();
        Tr { translator: EMPTY.get_or_init(Translator::default), locale: "en" }
    }

    /// The locale.
    pub fn locale(&self) -> &'a str {
        self.locale
    }

    /// Translates `source`.
    pub fn t(&self, source: &str) -> String {
        self.translator.get(self.locale, None, source).to_owned()
    }

    /// Translates `source`, looking in the table of `scope` (a resource slug) first.
    pub fn t_in(&self, scope: &str, source: &str) -> String {
        self.translator.get(self.locale, Some(scope), source).to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_before_framework_and_scope_before_global() {
        let mut t = Translator::default();
        t.add_framework(
            "admin",
            "vi",
            r#""Save" = "Lưu"
"Title" = "Tên""#,
        )
        .unwrap();
        t.add("vi", "\"Title\" = \"Tiêu đề\"\n\"Body\" = \"\"\n[posts]\n\"Title\" = \"Tiêu đề bài\"").unwrap();
        let tr = t.tr("vi");
        assert_eq!(tr.t("Save"), "Lưu");
        assert_eq!(tr.t("Title"), "Tiêu đề");
        assert_eq!(tr.t_in("posts", "Title"), "Tiêu đề bài");
        assert_eq!(tr.t_in("pages", "Title"), "Tiêu đề");
        // Empty means not translated yet; unknown strings stay as written.
        assert_eq!(tr.t("Body"), "Body");
        assert_eq!(tr.t("Nope"), "Nope");
        assert_eq!(t.tr("fr").t("Save"), "Save");
        assert!(t.has("vi") && !t.has("fr"));
    }

    #[test]
    fn malformed_catalogs_are_refused() {
        let mut t = Translator::default();
        for bad in [
            "not toml =",
            "\"x\" = 1",
            "[posts]\n\"x\" = 1",
            "\"at most {max}\" = \"tối đa {n}\"",
            "\"at most {max}\" = \"tối đa\"",
            "\"a\" = \"b {\"",
            "\"a\" = \"b }\"",
            "\"a\" = \"{x y}\"",
        ] {
            assert!(t.add("vi", bad).is_err(), "{bad}");
        }
        t.add("vi", "\"at most {max}\" = \"tối đa {max}\"\n\"{{literal}}\" = \"{{nguyên văn}}\"").unwrap();
        // Escaped braces are braces; placeholders are kept for the caller.
        assert_eq!(t.tr("vi").t("{literal}"), "{nguyên văn}");
        assert_eq!(t.tr("vi").t("at most {max}"), "tối đa {max}");
        assert!(t.add("vi", "").is_err(), "registered twice");
        assert!(t.add("v i", "").is_err());
        t.add_framework("admin", "vi", "").unwrap();
        assert!(t.add_framework("admin", "vi", "").is_err());
    }
}
