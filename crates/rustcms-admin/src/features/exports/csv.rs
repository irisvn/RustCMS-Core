//! CSV output (RFC 4180, CRLF) of wire values, safe to open in a spreadsheet:
//! text that a spreadsheet would run as a formula gets a leading `'`.

use serde_json::Value;

/// Characters that make a spreadsheet read a cell as a formula (also after
/// leading spaces), or start a new cell or line when first.
const FORMULA: [char; 4] = ['=', '+', '-', '@'];
const CONTROL: [char; 3] = ['\t', '\r', '\n'];

/// Whether `text` would be read as a formula.
fn dangerous(text: &str) -> bool {
    text.starts_with(CONTROL) || text.trim_start_matches(' ').starts_with(FORMULA)
}

/// `text` quoted as needed (quotes doubled).
fn quoted(text: &str, force: bool) -> String {
    if force || text.contains([',', '"', '\r', '\n']) || text.starts_with(' ') || text.ends_with(' ') {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text.to_owned()
    }
}

/// One cell: `null` is an empty unquoted cell, the empty string `""`;
/// text (`text` true: a text or JSON column) is protected against formulas;
/// numbers and booleans are written as they are.
pub(crate) fn cell(value: &Value, text: bool) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(s) if s.is_empty() => "\"\"".to_owned(),
        Value::String(s) if text && dangerous(s) => quoted(&format!("'{s}"), false),
        Value::String(s) => quoted(s, false),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        // JSON documents as their text, protected like text.
        other => {
            let s = other.to_string();
            if dangerous(&s) { quoted(&format!("'{s}"), false) } else { quoted(&s, false) }
        }
    }
}

/// A header cell (protected like text).
pub(crate) fn header(name: &str) -> String {
    cell(&Value::String(name.to_owned()), true)
}

/// A line of cells.
pub(crate) fn line(cells: impl IntoIterator<Item = String>) -> String {
    let mut out = cells.into_iter().collect::<Vec<_>>().join(",");
    out.push_str("\r\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn cells_are_quoted_and_protected() {
        let text = |v: Value| cell(&v, true);
        assert_eq!(text(json!(null)), "");
        assert_eq!(text(json!("")), "\"\"");
        assert_eq!(text(json!("plain")), "plain");
        assert_eq!(text(json!("a,b")), "\"a,b\"");
        assert_eq!(text(json!("say \"hi\"")), "\"say \"\"hi\"\"\"");
        assert_eq!(text(json!("two\nlines")), "\"two\nlines\"");
        for formula in ["=1+1", "+1", "-2", "@SUM(A1)", "  =x", "\tx", "\rx", "\nx"] {
            let out = text(json!(formula));
            assert!(out.trim_start_matches('"').starts_with('\''), "{formula:?} → {out:?}");
        }
        // Numbers (also as text of numeric columns) and booleans stay as they are.
        assert_eq!(cell(&json!(-2), false), "-2");
        assert_eq!(cell(&json!("-2"), false), "-2");
        assert_eq!(cell(&json!(true), false), "true");
        assert_eq!(text(json!({"k": "=x"})), "\"{\"\"k\"\":\"\"=x\"\"}\"");
        assert_eq!(header("=evil"), "'=evil");
        assert_eq!(line(["a".to_owned(), "".to_owned()]), "a,\r\n");
    }
}
