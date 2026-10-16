//! Bounded minijinja templates shared by the admin panel's custom pages and
//! mail (ADR 0019, ADR 0022): escaping chosen per template and never turned
//! off from inside one (no `safe`, `tojson`, `escape`, `autoescape`), no
//! includes, imports or inheritance, bounded work, recursion and output.

use minijinja::{AutoEscape, Environment, UndefinedBehavior};

/// Work a single render may do (minijinja fuel).
pub const FUEL: u64 = 200_000;
/// Nesting of loops, conditions and calls.
pub const RECURSION: usize = 32;

/// Statements a template may not use.
const BANNED_TAGS: [&str; 5] = ["include", "import", "from", "extends", "autoescape"];

/// An environment with these rules. `html(name)` says which templates are
/// HTML (escaped); others (plain text) are not escaped.
pub fn environment(html: fn(&str) -> bool) -> Environment<'static> {
    let mut env = Environment::new();
    env.set_auto_escape_callback(move |name| if html(name) { AutoEscape::Html } else { AutoEscape::None });
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    env.set_fuel(Some(FUEL));
    env.set_recursion_limit(RECURSION);
    // Nothing may bypass escaping.
    for filter in ["safe", "tojson", "escape", "e"] {
        env.remove_filter(filter);
    }
    env
}

/// Refuses a template using a banned statement (one template each, escaping as chosen).
pub fn check(name: &str, source: &str) -> Result<(), String> {
    match Lexer::new(source).tags().into_iter().find(|tag| BANNED_TAGS.contains(tag)) {
        Some(tag) => Err(format!("template `{name}`: `{{% {tag} … %}}` is not supported")),
        None => Ok(()),
    }
}

/// The literal strings passed to `function("…")` in `sources` (for `lang:missing`).
pub fn literal_calls(sources: &[&str], function: &str) -> Vec<String> {
    sources.iter().flat_map(|source| Lexer::new(source).literal_calls(function)).collect()
}

/// Output buffer refusing to grow past `max` bytes (rendering then stops).
pub struct Bounded {
    /// What was written.
    pub out: Vec<u8>,
    max: usize,
}

impl Bounded {
    /// An empty buffer of at most `max` bytes.
    pub fn new(max: usize) -> Self {
        Self { out: Vec::new(), max }
    }

    /// Whether the limit was reached.
    pub fn full(&self) -> bool {
        self.out.len() >= self.max
    }
}

impl std::io::Write for Bounded {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.out.len() + buf.len() > self.max {
            self.out.resize(self.max, 0);
            return Err(std::io::Error::other("output limit"));
        }
        self.out.extend_from_slice(buf);
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
        let found = literal_calls(
            &[
                "{{ t(\"Seats\") }} {{ t('Free') }} {{ data.t }} {{ t( \"Spaced\" ) }}",
                r#"{%- if x %}{{ t ("Say \"hi\"") ~ t('It\'s') }}{% endif %} t("not code") {# t("comment") #}"#,
                r#"{{ data.t("x") }} {{ at("y") }} {{ t(name) }} {{ "t('in a string')" }}"#,
            ],
            "t",
        );
        assert_eq!(found, ["Seats", "Free", "Spaced", "Say \"hi\"", "It's"]);
    }

    #[test]
    fn banned_statements_are_found() {
        for bad in ["{% include 'x.html' %}", "{%extends 'x'%}", "{%-\nimport 'x' as y %}", "{%+\n autoescape false %}"]
        {
            assert!(check("bad", bad).is_err(), "{bad}");
        }
        assert!(check("ok", "{% if x %}{{ x }}{% endif %}").is_ok());
    }

    #[test]
    fn text_templates_are_not_escaped_html_ones_are() {
        let mut env = environment(|name| name.ends_with(".html"));
        env.add_template("a.html", "{{ v }}").unwrap();
        env.add_template("a.txt", "{{ v }}").unwrap();
        let ctx = minijinja::context! { v => "<b>&" };
        assert_eq!(env.get_template("a.html").unwrap().render(&ctx).unwrap(), "&lt;b&gt;&amp;");
        assert_eq!(env.get_template("a.txt").unwrap().render(&ctx).unwrap(), "<b>&");
    }
}
