//! Server templates of custom pages and slots (minijinja): HTML autoescape
//! always on, no way to mark a value safe, no includes, bounded work. The
//! output is then cleaned by [`super::html::Sanitizer`].

use std::collections::HashMap;
use std::sync::Arc;

use minijinja::{Environment, Value};
use rustcms_core::template::{Bounded, check};
use rustcms_core::{AppError, AppResult};

use crate::context::Ctx;

/// Largest rendered page or slot.
pub(crate) const MAX_HTML_BYTES: usize = 512 * 1024;

/// Every template of the panel, compiled at boot.
pub(crate) struct Templates {
    env: Environment<'static>,
    /// Template files read again at each render (local development).
    files: HashMap<String, &'static str>,
    reload: bool,
}

/// An environment with the panel's rules: escaping always on, nothing marks
/// a value safe, bounded work.
fn environment() -> Environment<'static> {
    rustcms_core::template::environment(|_| true)
}

impl Templates {
    pub(crate) fn new() -> Self {
        Self { env: environment(), files: HashMap::new(), reload: false }
    }

    /// Whether files registered with [`Self::watch`] are read again at each render.
    pub(crate) fn reload_files(&mut self, reload: bool) {
        self.reload = reload;
    }

    /// Compiles `source` under `name`.
    ///
    /// # Panics
    /// On a syntax error, or a template that includes, imports or extends
    /// another (one template per page or slot), or turns escaping off.
    pub(crate) fn add(&mut self, name: &str, source: &'static str) {
        if let Err(err) = check(name, source) {
            panic!("{err}");
        }
        if let Err(err) = self.env.add_template_owned(name.to_owned(), source) {
            panic!("template `{name}`: {err:#}");
        }
    }

    /// `name` was embedded from the file at `path`: in local development it
    /// is read from there at each render (checked as at boot).
    pub(crate) fn watch(&mut self, name: &str, path: &'static str) {
        self.files.insert(name.to_owned(), path);
    }

    /// The literal strings passed to `t("…")` in every template (for `lang:missing`).
    pub(crate) fn literals(sources: &[&str]) -> Vec<String> {
        rustcms_core::template::literal_calls(sources, "t")
    }

    /// Renders `name` for `ctx` with `data`.
    pub(crate) fn render(&self, name: &str, ctx: &Ctx, base: &str, data: &serde_json::Value) -> AppResult<String> {
        // Local development: the file as it is now, checked as at boot.
        if self.reload
            && let Some(path) = self.files.get(name)
        {
            match std::fs::read_to_string(path) {
                Ok(source) => {
                    check(name, &source).map_err(AppError::internal)?;
                    let mut env = environment();
                    env.add_template_owned(name.to_owned(), source)
                        .map_err(|err| AppError::internal(format!("template `{name}`: {err:#}")))?;
                    return Self::render_in(&env, name, ctx, base, data);
                }
                Err(err) => {
                    tracing::warn!(template = name, path, error = %err, "template file unreadable; using the embedded copy")
                }
            }
        }
        Self::render_in(&self.env, name, ctx, base, data)
    }

    fn render_in(
        env: &Environment<'_>,
        name: &str,
        ctx: &Ctx,
        base: &str,
        data: &serde_json::Value,
    ) -> AppResult<String> {
        let template = env.get_template(name).map_err(AppError::internal)?;
        let translator = ctx.translator.clone();
        let locale = ctx.locale.clone();
        let t = Value::from_function(move |text: String| translator.get(&locale, None, &text).to_owned());
        let principal = Arc::new(ctx.principal.clone());
        let can = Value::from_function(move |permission: String| principal.can(&permission));
        let panel = base.trim_end_matches('/').to_owned();
        let admin_url = Value::from_function(move |path: String| -> Result<String, minijinja::Error> {
            let valid =
                path.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'/' | b'_' | b'-'))
                    && !path.contains("//");
            if !valid {
                return Err(minijinja::Error::new(minijinja::ErrorKind::InvalidOperation, "admin_url: invalid path"));
            }
            Ok(format!("{panel}/{}", path.trim_start_matches('/')))
        });
        let context = minijinja::context! {
            data => Value::from(minijinja::value::Serde(data)),
            locale => ctx.locale.clone(),
            user => minijinja::context! { id => ctx.principal.id.clone() },
            t => t,
            can => can,
            admin_url => admin_url,
        };
        // Written into a bounded buffer: rendering stops at the limit, before
        // anything larger is allocated.
        let mut out = Bounded::new(MAX_HTML_BYTES);
        if let Err(err) = template.render_captured_to(context, &mut out) {
            if out.full() {
                return Err(AppError::internal(format!("template `{name}`: more than {MAX_HTML_BYTES} bytes")));
            }
            return Err(AppError::internal(format!("template `{name}`: {err:#}")));
        }
        String::from_utf8(out.out).map_err(AppError::internal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literals_are_found() {
        let found = Templates::literals(&[
            "{{ t(\"Seats\") }} {{ t('Free') }} {{ data.t }} {{ t( \"Spaced\" ) }}",
            r#"{%- if x %}{{ t ("Say \"hi\"") ~ t('It\'s') }}{% endif %} t("not code") {# t("comment") #}"#,
            r#"{{ data.t("x") }} {{ at("y") }} {{ t(name) }} {{ "t('in a string')" }}"#,
        ]);
        assert_eq!(found, ["Seats", "Free", "Spaced", "Say \"hi\"", "It's"]);
    }

    #[test]
    fn includes_and_syntax_errors_stop_the_boot() {
        for bad in ["{% include 'x.html' %}", "{%extends 'x'%}", "{%-\nimport 'x' as y %}", "{{ unclosed"] {
            let result = std::panic::catch_unwind(|| Templates::new().add("bad", bad));
            assert!(result.is_err(), "{bad}");
        }
    }

    #[test]
    fn escaping_cannot_be_turned_off() {
        let mut templates = Templates::new();
        templates.add("x", "{{ data.v }}");
        let env = &templates.env;
        let out = env
            .get_template("x")
            .unwrap()
            .render(minijinja::context! { data => Value::from(minijinja::value::Serde(serde_json::json!({"v": "<script>\"x\"</script>"}))) })
            .unwrap();
        assert_eq!(out, "&lt;script&gt;&quot;x&quot;&lt;&#x2f;script&gt;");
        for bypass in [
            "{{ data.v|safe }}",
            "{{ data.v|tojson }}",
            "{% autoescape false %}{{ data.v }}{% endautoescape %}",
            "{%autoescape false%}{{ data.v }}{%endautoescape%}",
            "{%-\tautoescape false -%}{{ data.v }}{% endautoescape %}",
            "{%+\n autoescape false %}{{ data.v }}{% endautoescape %}",
        ] {
            let mut templates = Templates::new();
            let compiled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| templates.add("y", bypass)));
            if compiled.is_ok() {
                let out = templates
                    .env
                    .get_template("y")
                    .unwrap()
                    .render(minijinja::context! { data => Value::from(minijinja::value::Serde(serde_json::json!({"v": "<b>"}))) });
                assert!(out.is_err() || !out.unwrap().contains("<b>"), "{bypass}");
            }
        }
    }

    #[test]
    fn output_stops_at_the_limit() {
        let mut templates = Templates::new();
        templates.add("big", "{% for i in range(10000) %}{{ data.v }}{% endfor %}");
        let data = serde_json::json!({ "v": "x".repeat(64 * 1024) });
        let mut out = Bounded::new(MAX_HTML_BYTES);
        let result = templates
            .env
            .get_template("big")
            .unwrap()
            .render_captured_to(minijinja::context! { data => Value::from(minijinja::value::Serde(data)) }, &mut out);
        assert!(result.is_err());
        assert!(out.out.len() <= MAX_HTML_BYTES, "{}", out.out.len());
    }
}
