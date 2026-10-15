//! Server templates of custom pages and slots (minijinja): HTML autoescape
//! always on, no way to mark a value safe, no includes, bounded work. The
//! output is then cleaned by [`super::html::Sanitizer`].

use std::collections::HashMap;
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
    /// Template files read again at each render (local development).
    files: HashMap<String, &'static str>,
    reload: bool,
}

/// An environment with the panel's rules: escaping always on, nothing marks
/// a value safe, bounded work.
fn environment() -> Environment<'static> {
    let mut env = Environment::new();
    env.set_auto_escape_callback(|_| AutoEscape::Html);
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    env.set_fuel(Some(FUEL));
    env.set_recursion_limit(RECURSION);
    // Nothing may bypass escaping.
    for filter in ["safe", "tojson", "escape", "e"] {
        env.remove_filter(filter);
    }
    env
}

/// One template per page or slot, escaping always on.
fn check(name: &str, source: &str) -> Result<(), String> {
    match Lexer::new(source).tags().into_iter().find(|tag| BANNED_TAGS.contains(tag)) {
        Some(tag) => Err(format!("template `{name}`: `{{% {tag} … %}}` is not supported")),
        None => Ok(()),
    }
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
        sources.iter().flat_map(|source| Lexer::new(source).literal_calls("t")).collect()
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
        let mut out = Bounded(Vec::new());
        if let Err(err) = template.render_captured_to(context, &mut out) {
            if out.0.len() >= MAX_HTML_BYTES {
                return Err(AppError::internal(format!("template `{name}`: more than {MAX_HTML_BYTES} bytes")));
            }
            return Err(AppError::internal(format!("template `{name}`: {err:#}")));
        }
        String::from_utf8(out.0).map_err(AppError::internal)
    }
}

/// Statements a template may not use.
const BANNED_TAGS: [&str; 5] = ["include", "import", "from", "extends", "autoescape"];

/// Output buffer refusing to grow past [`MAX_HTML_BYTES`].
struct Bounded(Vec<u8>);

impl std::io::Write for Bounded {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.0.len() + buf.len() > MAX_HTML_BYTES {
            self.0.resize(MAX_HTML_BYTES, 0);
            return Err(std::io::Error::other("output limit"));
        }
        self.0.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Reads the code of a template (inside `{{ }}` and `{% %}`, past comments
/// and string literals) as minijinja does with its default syntax: enough to
/// name statements and literal calls whatever the whitespace.
struct Lexer<'a> {
    source: &'a str,
}

/// One block of code: statement (`{% %}`) or expression (`{{ }}`).
struct Block<'a> {
    statement: bool,
    code: &'a str,
}

impl<'a> Lexer<'a> {
    fn new(source: &'a str) -> Self {
        Self { source }
    }

    /// Every block of code, in order.
    fn blocks(&self) -> Vec<Block<'a>> {
        let bytes = self.source.as_bytes();
        let mut blocks = Vec::new();
        let mut i = 0;
        while i + 1 < bytes.len() {
            if bytes[i] != b'{' || !matches!(bytes[i + 1], b'{' | b'%' | b'#') {
                i += 1;
                continue;
            }
            let kind = bytes[i + 1];
            let start = i + 2;
            if kind == b'#' {
                i = self.source[start..].find("#}").map_or(bytes.len(), |end| start + end + 2);
                continue;
            }
            let close = if kind == b'{' { b'}' } else { b'%' };
            // To the closing delimiter outside strings.
            let mut j = start;
            let mut quote = None;
            while j < bytes.len() {
                match (quote, bytes[j]) {
                    (Some(_), b'\\') => j += 1,
                    (Some(q), b) if b == q => quote = None,
                    (None, b'"' | b'\'') => quote = Some(bytes[j]),
                    (None, b) if b == close && bytes.get(j + 1) == Some(&b'}') => break,
                    _ => {}
                }
                j += 1;
            }
            let end = j.min(bytes.len());
            blocks.push(Block { statement: kind == b'%', code: &self.source[start..end] });
            i = end + 2;
        }
        blocks
    }

    /// The name of every statement (`{%- autoescape … %}` → `autoescape`).
    fn tags(&self) -> Vec<&'a str> {
        self.blocks()
            .into_iter()
            .filter(|b| b.statement)
            .map(|b| {
                let code = b.code.trim_start_matches(['-', '+']).trim_start();
                let end = code.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(code.len());
                &code[..end]
            })
            .collect()
    }

    /// The string of every call `function("literal")`.
    fn literal_calls(&self, function: &str) -> Vec<String> {
        let mut out = Vec::new();
        for block in self.blocks() {
            let code = block.code;
            let bytes = code.as_bytes();
            let mut i = 0;
            let mut quote = None;
            while i < bytes.len() {
                let b = bytes[i];
                if let Some(q) = quote {
                    if b == b'\\' {
                        i += 1;
                    } else if b == q {
                        quote = None;
                    }
                    i += 1;
                    continue;
                }
                if matches!(b, b'"' | b'\'') {
                    quote = Some(b);
                    i += 1;
                    continue;
                }
                let ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
                let preceded = i > 0 && (ident(bytes[i - 1]) || bytes[i - 1] == b'.');
                if !preceded
                    && code[i..].starts_with(function)
                    && !bytes.get(i + function.len()).is_some_and(|&b| ident(b))
                {
                    let rest = code[i + function.len()..].trim_start();
                    if let Some(rest) = rest.strip_prefix('(')
                        && let Some((text, after)) = string_literal(rest.trim_start())
                        && after.trim_start().starts_with(')')
                    {
                        out.push(text);
                    }
                }
                i += 1;
            }
        }
        out
    }
}

/// A string literal at the start of `code` (its value and what follows), with
/// minijinja's escapes.
fn string_literal(code: &str) -> Option<(String, &str)> {
    let mut chars = code.char_indices();
    let (_, quote) = chars.next().filter(|(_, c)| matches!(c, '"' | '\''))?;
    let mut text = String::new();
    while let Some((i, c)) = chars.next() {
        match c {
            '\\' => {
                let (_, escaped) = chars.next()?;
                text.push(match escaped {
                    'n' => '\n',
                    't' => '\t',
                    'r' => '\r',
                    '"' | '\'' | '\\' | '/' => escaped,
                    // Unicode and other escapes: not a plain literal.
                    _ => return None,
                });
            }
            c if c == quote => return Some((text, &code[i + 1..])),
            c => text.push(c),
        }
    }
    None
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
        let mut out = Bounded(Vec::new());
        let result = templates
            .env
            .get_template("big")
            .unwrap()
            .render_captured_to(minijinja::context! { data => Value::from(minijinja::value::Serde(data)) }, &mut out);
        assert!(result.is_err());
        assert!(out.0.len() <= MAX_HTML_BYTES, "{}", out.0.len());
    }
}
