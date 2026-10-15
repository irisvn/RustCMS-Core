//! Name conversions for generated code.

use anyhow::{bail, ensure};

/// `BlogPost` → `blog_post`.
pub fn snake_case(name: &str) -> String {
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// `blog_post` → `BlogPost`.
pub fn pascal_case(name: &str) -> String {
    name.split('_')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let mut chars = p.chars();
            chars.next().map(|c| c.to_ascii_uppercase().to_string() + chars.as_str()).unwrap_or_default()
        })
        .collect()
}

/// English plural of the last word: `category` → `categories`, `post` → `posts`.
pub fn plural(word: &str) -> String {
    let vowel_before_y = word.chars().rev().nth(1).is_some_and(|c| "aeiou".contains(c));
    let consonant_y = word.ends_with('y') && !vowel_before_y;
    if consonant_y {
        format!("{}ies", &word[..word.len() - 1])
    } else if ["s", "x", "z", "ch", "sh"].iter().any(|s| word.ends_with(s)) {
        format!("{word}es")
    } else {
        format!("{word}s")
    }
}

/// Rust keywords, including reserved ones (edition 2024): cannot be module or crate names.
const KEYWORDS: &[&str] = &[
    "abstract", "as", "async", "await", "become", "box", "break", "const", "continue", "crate", "do", "dyn", "else",
    "enum", "extern", "false", "final", "fn", "for", "gen", "if", "impl", "in", "let", "loop", "macro", "match", "mod",
    "move", "mut", "override", "priv", "pub", "ref", "return", "self", "static", "struct", "super", "trait", "true",
    "try", "type", "typeof", "unsafe", "unsized", "use", "virtual", "where", "while", "yield",
];

/// PascalCase model name whose module name is not a keyword.
/// `BlogPost` → `Blog post`.
pub fn humanize(model: &str) -> String {
    let words = snake_case(model).replace('_', " ");
    let mut chars = words.chars();
    chars.next().map(|c| c.to_ascii_uppercase().to_string() + chars.as_str()).unwrap_or_default()
}

/// Whether `word` is a Rust keyword (not usable as a module name).
pub fn is_keyword(word: &str) -> bool {
    KEYWORDS.contains(&word)
}

pub fn validate_model(name: &str) -> anyhow::Result<()> {
    ensure!(
        name.chars().next().is_some_and(|c| c.is_ascii_uppercase()) && name.chars().all(|c| c.is_ascii_alphanumeric()),
        "model name must be PascalCase, e.g. `BlogPost`"
    );
    ensure!(
        !KEYWORDS.contains(&snake_case(name).as_str()),
        "`{name}` would be the module `{}`, a Rust keyword",
        snake_case(name)
    );
    Ok(())
}

/// snake_case identifier.
pub fn validate_snake(name: &str) -> anyhow::Result<()> {
    if name.is_empty()
        || !name.starts_with(|c: char| c.is_ascii_lowercase())
        || !name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    {
        bail!("`{name}` must be snake_case, e.g. `create_posts_table`");
    }
    Ok(())
}

/// Cargo package name accepted by `rustcms new`.
pub fn validate_package(name: &str) -> anyhow::Result<()> {
    ensure!(
        name.starts_with(|c: char| c.is_ascii_lowercase())
            && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_'),
        "`{name}` is not a valid package name (lowercase letters, digits, `-`, `_`)"
    );
    // The crate name appears as a path in the generated code (`use blog::…`).
    let crate_name = name.replace('-', "_");
    ensure!(!KEYWORDS.contains(&crate_name.as_str()), "`{name}` is a Rust keyword: choose another package name");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversions() {
        assert_eq!(snake_case("BlogPost"), "blog_post");
        assert_eq!(snake_case("Post"), "post");
        assert_eq!(pascal_case("blog_posts"), "BlogPosts");
        assert_eq!(plural("blog_post"), "blog_posts");
        assert_eq!(plural("category"), "categories");
        assert_eq!(plural("day"), "days");
        assert_eq!(plural("box"), "boxes");
        assert_eq!(plural("match"), "matches");
    }

    #[test]
    fn validation() {
        assert!(validate_model("BlogPost").is_ok());
        assert!(validate_model("blog_post").is_err());
        assert!(validate_model("Blog-Post").is_err());
        assert!(validate_snake("create_posts_table").is_ok());
        assert!(validate_snake("../evil").is_err());
        assert!(validate_package("my-blog").is_ok());
        assert!(validate_package("My Blog").is_err());
        assert!(validate_package("async").is_err(), "keywords break `use <crate>::…`");
        assert!(validate_package("my-type").is_ok());
    }
}
