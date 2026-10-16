//! Mail templates: `{name}.subject.txt`, `{name}.txt` and optionally
//! `{name}.html` per language, embedded in the binary
//! ([`mail_template!`](crate::mail_template)); in local development (debug
//! build, `APP_ENV=local`) read again from their files at each render.
//! Text is not escaped, HTML always is (nothing marks a value safe);
//! work, recursion and output are bounded.

use std::collections::HashMap;

use minijinja::Value;
use serde_json::Value as Json;

use super::{MAX_DATA_BYTES, MailError};
use crate::template::{Bounded, check, environment};

/// Largest rendered part.
const MAX_PART_BYTES: usize = 256 * 1024;
/// Longest subject, in characters.
const MAX_SUBJECT: usize = 255;

/// One mail template in one language.
#[derive(Debug, Clone)]
pub struct MailTemplate {
    /// Name (`welcome`, `auth.password_reset`).
    pub name: &'static str,
    /// Language (`en`, `vi`).
    pub locale: &'static str,
    /// Subject template (one line).
    pub subject: &'static str,
    /// Text template.
    pub text: &'static str,
    /// HTML template.
    pub html: Option<&'static str>,
    /// The files' path without extension, read again in local development.
    pub path: Option<&'static str>,
}

/// Embeds `resources/views/mail/{locale}/{name}.subject.txt` and `.txt` (and
/// `.html` with `html`) of the calling crate as a [`MailTemplate`].
///
/// ```rust,ignore
/// app.mail_template(rustcms::mail_template!("welcome", "vi", html));
/// ```
#[macro_export]
macro_rules! mail_template {
    ($name:literal, $locale:literal) => {
        $crate::mail::MailTemplate {
            name: $name,
            locale: $locale,
            subject: include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/resources/views/mail/",
                $locale,
                "/",
                $name,
                ".subject.txt"
            )),
            text: include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/resources/views/mail/",
                $locale,
                "/",
                $name,
                ".txt"
            )),
            html: None,
            path: Some(concat!(env!("CARGO_MANIFEST_DIR"), "/resources/views/mail/", $locale, "/", $name)),
        }
    };
    ($name:literal, $locale:literal, html) => {
        $crate::mail::MailTemplate {
            html: Some(include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/resources/views/mail/",
                $locale,
                "/",
                $name,
                ".html"
            ))),
            ..$crate::mail_template!($name, $locale)
        }
    };
}

/// A rendered mail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    /// Subject.
    pub subject: String,
    /// Text body.
    pub text: String,
    /// HTML body.
    pub html: Option<String>,
}

/// Every mail template of the application (see `AppBuilder::mail_template`).
#[derive(Debug, Clone, Default)]
pub struct MailTemplates {
    templates: HashMap<(String, String), MailTemplate>,
}

fn is_html(name: &str) -> bool {
    name.ends_with(".html")
}

impl MailTemplates {
    /// Adds `template` (checked: syntax, no banned statements).
    ///
    /// # Panics
    /// On an invalid template, or one added twice.
    pub fn add(&mut self, template: MailTemplate) {
        let key = (template.name.to_owned(), template.locale.to_owned());
        if let Err(err) = compile(template.name, template.subject, template.text, template.html) {
            panic!("mail template `{}` ({}): {err}", template.name, template.locale);
        }
        assert!(
            self.templates.insert(key, template.clone()).is_none(),
            "mail template `{}` ({}) added twice",
            template.name,
            template.locale
        );
    }

    /// Renders `name` in `locale`, else in `app.locale`.
    pub fn render(&self, config: &crate::Config, name: &str, locale: &str, data: &Json) -> Result<Rendered, MailError> {
        let template = self
            .templates
            .get(&(name.to_owned(), locale.to_owned()))
            .or_else(|| self.templates.get(&(name.to_owned(), config.app.locale.clone())))
            .ok_or_else(|| {
                tracing::warn!(template = name, locale, "mail template missing");
                MailError::TEMPLATE
            })?;
        if serde_json::to_vec(data).map_or(true, |b| b.len() > MAX_DATA_BYTES) {
            return Err(MailError::TEMPLATE);
        }
        let reload = cfg!(debug_assertions) && config.app.env == crate::Environment::Local;
        let files = template.path.filter(|_| reload).and_then(|path| {
            let read = |ext: &str| std::fs::read_to_string(format!("{path}.{ext}")).ok();
            Some((read("subject.txt")?, read("txt")?, template.html.and_then(|_| read("html"))))
        });
        let (subject, text, html) = match &files {
            Some((s, t, h)) => (s.as_str(), t.as_str(), h.as_deref().or(template.html)),
            None => (template.subject, template.text, template.html),
        };
        let env = compile(name, subject, text, html).map_err(|err| {
            tracing::warn!(template = name, error = %err, "mail template invalid");
            MailError::TEMPLATE
        })?;
        let context = minijinja::context! {
            data => Value::from(minijinja::value::Serde(data)),
            app_name => config.app.name.clone(),
            app_url => config.app.url.trim_end_matches('/').to_owned(),
            locale => template.locale,
        };
        let part = |part: &str| -> Result<String, MailError> {
            let compiled = env.get_template(part).map_err(|_| MailError::TEMPLATE)?;
            let mut out = Bounded::new(MAX_PART_BYTES);
            // The template's error may quote data: only its kind is logged.
            compiled.render_captured_to(&context, &mut out).map_err(|err| {
                tracing::warn!(template = part, kind = ?err.kind(), "mail template failed");
                MailError::TEMPLATE
            })?;
            String::from_utf8(out.out).map_err(|_| MailError::TEMPLATE)
        };
        let subject = part("subject.txt")?;
        let subject = subject.trim();
        if subject.is_empty() || subject.chars().count() > MAX_SUBJECT || subject.chars().any(char::is_control) {
            tracing::warn!(template = name, "mail subject empty, too long or not one line");
            return Err(MailError::TEMPLATE);
        }
        Ok(Rendered {
            subject: subject.to_owned(),
            text: part("txt")?,
            html: html.map(|_| part("html.html")).transpose()?,
        })
    }
}

/// The three parts of a template in one environment (`subject.txt`, `txt`, `html`).
fn compile(
    name: &str,
    subject: &str,
    text: &str,
    html: Option<&str>,
) -> Result<minijinja::Environment<'static>, String> {
    let mut env = environment(is_html);
    let parts = [("subject.txt", Some(subject)), ("txt", Some(text)), ("html", html)];
    for (part, source) in parts {
        let Some(source) = source else { continue };
        let full = if part == "html" { format!("{name}.html") } else { format!("{name}.{part}") };
        check(&full, source)?;
        // Named by part; HTML escaping by the `.html` suffix.
        let key = if part == "html" { "html.html".to_owned() } else { part.to_owned() };
        env.add_template_owned(key, source.to_owned()).map_err(|err| format!("{full}: {}", err.kind()))?;
    }
    Ok(env)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn template(subject: &'static str, text: &'static str, html: Option<&'static str>) -> MailTemplate {
        MailTemplate { name: "t", locale: "en", subject, text, html, path: None }
    }

    fn config() -> crate::Config {
        use figment::providers::Serialized;
        let figment = crate::Config::figment(std::path::Path::new("/nonexistent")).unwrap().merge(
            Serialized::defaults(serde_json::json!({ "app": {
                "name": "rustcms", "env": "testing", "debug": false, "url": "http://localhost:8000",
                "listen": "127.0.0.1:8000", "locale": "en", "locales": ["en", "vi"],
                "key": crate::AppKey::generate().encode(),
            }, "database": { "url": "sqlite::memory:" } })),
        );
        crate::Config::from_figment(figment).unwrap()
    }

    #[test]
    fn text_is_plain_html_is_escaped() {
        let mut templates = MailTemplates::default();
        templates.add(template(
            "Hi {{ data.name }}",
            "Hello {{ data.name }} at {{ app_url }}",
            Some("<p>{{ data.name }}</p>"),
        ));
        let out = templates.render(&config(), "t", "vi", &serde_json::json!({"name": "<A&B>"})).unwrap();
        assert_eq!(out.subject, "Hi <A&B>");
        assert_eq!(out.text, "Hello <A&B> at http://localhost:8000");
        assert_eq!(out.html.as_deref(), Some("<p>&lt;A&amp;B&gt;</p>"));
    }

    #[test]
    fn subjects_are_one_line_and_bypasses_refused() {
        let mut templates = MailTemplates::default();
        templates.add(template("Hi\r\nBcc: x@evil.test {{ data.v }}", "x", None));
        assert_eq!(templates.render(&config(), "t", "en", &serde_json::json!({"v": 1})), Err(MailError::TEMPLATE));
        let mut injected = MailTemplates::default();
        injected.add(template("Hi {{ data.v }}", "x", None));
        assert_eq!(
            injected.render(&config(), "t", "en", &serde_json::json!({"v": "a\nBcc: x@evil.test"})),
            Err(MailError::TEMPLATE)
        );
        // Refused when added, or failing when rendered: never unescaped output.
        for bad in [
            "{{ data.v|safe }}",
            "{{ data.v|tojson }}",
            "{% autoescape false %}{{ data.v }}{% endautoescape %}",
            "{% include 'x' %}",
        ] {
            let mut templates = MailTemplates::default();
            let added = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                templates.add(MailTemplate { html: Some(bad), ..template("s", "t", None) })
            }));
            if added.is_ok() {
                let out = templates.render(&config(), "t", "en", &serde_json::json!({"v": "<b>"}));
                assert_eq!(out, Err(MailError::TEMPLATE), "{bad}");
            }
        }
    }

    #[test]
    fn missing_template_or_large_data_fail() {
        let templates = MailTemplates::default();
        assert_eq!(templates.render(&config(), "nope", "en", &serde_json::json!({})), Err(MailError::TEMPLATE));
        let mut templates = MailTemplates::default();
        templates.add(template("s", "{{ data.v }}", None));
        let big = serde_json::json!({ "v": "x".repeat(MAX_DATA_BYTES) });
        assert_eq!(templates.render(&config(), "t", "en", &big), Err(MailError::TEMPLATE));
    }
}
