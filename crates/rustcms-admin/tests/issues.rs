//! Every validation message the framework writes has a stable code: the panel
//! translates by code, so a message without one would stay English. Scans the
//! sources of core, auth and admin for the calls that add messages.

use std::path::{Path, PathBuf};

use rustcms_core::i18n::{CUSTOM, Issue, TEMPLATES};

/// Calls whose last string literal is a user-visible validation message.
const MARKERS: &[&str] =
    &["ValidationErrors::field(", ".add(", ".add_form(", "invalid(", "DecodeError(", "err(", "bad("];

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// The string literals inside the parentheses that open at `start`.
fn literals(text: &str, start: usize) -> Vec<String> {
    let (mut depth, mut out, mut chars) = (0usize, Vec::new(), text[start..].char_indices().peekable());
    while let Some((_, c)) = chars.next() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            '"' => {
                let mut literal = String::new();
                while let Some((_, c)) = chars.next() {
                    match c {
                        '\\' => {
                            if let Some((_, escaped)) = chars.next() {
                                literal.push(escaped);
                            }
                        }
                        '"' => break,
                        c => literal.push(c),
                    }
                }
                out.push(literal);
            }
            _ => {}
        }
    }
    out
}

/// `format!` arguments replaced by a sample value.
fn sample(message: &str) -> String {
    let mut out = String::new();
    let mut rest = message;
    while let Some(i) = rest.find('{') {
        out.push_str(&rest[..i]);
        let end = rest[i..].find('}').map_or(rest.len(), |e| i + e + 1);
        out.push('7');
        rest = &rest[end..];
    }
    out + rest
}

#[test]
fn framework_messages_have_codes() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut files = Vec::new();
    for krate in ["rustcms-core", "rustcms-auth", "rustcms-admin"] {
        sources(&root.join(krate).join("src"), &mut files);
    }
    let mut checked = 0;
    let mut missing = Vec::new();
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap();
        // Unit tests write messages of their own.
        let text = text.split("#[cfg(test)]").next().unwrap_or_default();
        for marker in MARKERS {
            for (at, _) in text.match_indices(marker) {
                // `map_err(`, `ok_or_else(`…: a word boundary before the call.
                let before = text[..at].chars().next_back().unwrap_or(' ');
                if marker.starts_with(char::is_alphabetic) && (before.is_alphanumeric() || before == '_') {
                    continue;
                }
                let Some(message) = literals(text, at + marker.len() - 1).pop() else { continue };
                // Field names, keys and SQL fragments are not messages.
                if !message.contains(' ') {
                    continue;
                }
                checked += 1;
                if Issue::of(&sample(&message)).code == CUSTOM {
                    missing.push(format!("{}: {message:?}", file.display()));
                }
            }
        }
    }
    assert!(checked > 80, "only {checked} messages found: the scan is broken");
    assert!(missing.is_empty(), "messages without a code in rustcms_core::i18n::TEMPLATES:\n{}", missing.join("\n"));
}

#[test]
fn codes_are_documented() {
    let docs = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/src/admin/i18n.md"))
        .unwrap_or_default();
    let undocumented: Vec<&str> =
        TEMPLATES.iter().map(|(code, _)| *code).filter(|code| !docs.contains(&format!("`{code}`"))).collect();
    assert!(undocumented.is_empty(), "add to docs/src/admin/i18n.md: {undocumented:?}");
}

/// `fixtures/contract/issues.json`: the codes and English templates the panel
/// translates (its catalogs are checked against this file). Regenerate with
/// `UPDATE_CONTRACT=1`.
#[test]
fn issue_codes_fixture() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/contract/issues.json");
    let mut codes = serde_json::Map::new();
    for (code, template) in TEMPLATES {
        let list = codes.entry(code.to_string()).or_insert_with(|| serde_json::json!([]));
        list.as_array_mut().unwrap().push(serde_json::json!(template));
    }
    let json = serde_json::to_string_pretty(&serde_json::Value::Object(codes)).unwrap() + "\n";
    if std::env::var("UPDATE_CONTRACT").is_ok() {
        std::fs::write(&path, &json).unwrap();
    }
    let stored = std::fs::read_to_string(&path).unwrap_or_default();
    assert_eq!(stored, json, "fixtures/contract/issues.json is stale: UPDATE_CONTRACT=1 cargo test -p rustcms-admin");
}
