//! Server templates of custom pages and slots (minijinja): HTML autoescape
//! always on, no way to mark a value safe, no includes, bounded work. The
//! output is then cleaned by [`super::html::Sanitizer`].

use std::sync::Arc;

use minijinja::{AutoEscape, Environment, UndefinedBehavior, Value};
use rustcms_core::{AppError, AppResult};

use crate::context::Ctx;

/// Work a single render may do (minijinja fuel).
const FUEL: u64 = 200_000;
/// Nesting of loops, conditions and calls.
const RECURSION: usize = 32;
/// Largest rendered page or slot.
pub(crate) const MAX_HTML_BYTES: usize = 512 * 1024;

/// Every template of the panel, compiled at boot.
pub(crate) struct Templates {
    env: Environment<'static>,
}

impl Templates {
    pub(crate) fn new() -> Self {
        let mut env = Environment::new();
        env.set_auto_escape_callback(|_| AutoEscape::Html);
        env.set_undefined_behavior(UndefinedBehavior::Strict);
        env.set_fuel(Some(FUEL));
        env.set_recursion_limit(RECURSION);
        // Nothing may bypass escaping.
        for filter in ["safe", "tojson", "escape", "e"] {
            env.remove_filter(filter);
        }
        Self { env }
    }

    /// Compiles `source` under `name`.
    ///
    /// # Panics
    /// On a syntax error, or a template that includes, imports or extends
    /// another (one template per page or slot), or turns escaping off.
    pub(crate) fn add(&mut self, name: &str, source: &'static str) {
        // One template per page or slot, escaping always on.
        for tag in ["include", "import", "from", "extends", "autoescape"] {
            for open in ["{%", "{%-", "{%+"] {
                let needle = format!("{open} {tag}");
                assert!(!source.contains(&needle), "template `{name}`: `{needle} …` is not supported");
            }
        }
        if let Err(err) = self.env.add_template_owned(name.to_owned(), source) {
            panic!("template `{name}`: {err:#}");
        }
    }

    /// The literal strings passed to `t("…")` in every template (for `lang:missing`).
    pub(crate) fn literals(sources: &[&str]) -> Vec<String> {
        let mut out = Vec::new();
        for source in sources {
            for quote in ['"', '\''] {
                let open = format!("t({quote}");
                let mut rest = *source;
                while let Some(start) = rest.find(&open) {
                    rest = &rest[start + open.len()..];
                    if let Some(end) = rest.find(quote) {
                        out.push(rest[..end].to_owned());
                        rest = &rest[end..];
                    }
                }
            }
        }
        out
    }

    /// Renders `name` for `ctx` with `data`.
    pub(crate) fn render(&self, name: &str, ctx: &Ctx, base: &str, data: &serde_json::Value) -> AppResult<String> {
        let template = self.env.get_template(name).map_err(AppError::internal)?;
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
        let html = template.render(context).map_err(|err| AppError::internal(format!("template `{name}`: {err:#}")))?;
        if html.len() > MAX_HTML_BYTES {
            return Err(AppError::internal(format!("template `{name}`: more than {MAX_HTML_BYTES} bytes")));
        }
        Ok(html)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literals_are_found() {
        let found = Templates::literals(&["{{ t(\"Seats\") }} {{ t('Free') }} {{ data.t }}"]);
        assert_eq!(found, ["Seats", "Free"]);
    }

    #[test]
    fn includes_and_syntax_errors_stop_the_boot() {
        for bad in ["{% include 'x.html' %}", "{% extends 'x' %}", "{{ unclosed"] {
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
        for bypass in
            ["{{ data.v|safe }}", "{{ data.v|tojson }}", "{% autoescape false %}{{ data.v }}{% endautoescape %}"]
        {
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
}
