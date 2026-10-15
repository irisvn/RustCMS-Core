//! CSV output (RFC 4180, CRLF) of wire values, safe to open in a spreadsheet:
//! text that a spreadsheet would run as a formula gets a leading `'`.

use serde_json::Value;

use super::CellKind;

/// Characters that make a spreadsheet read a cell as a formula.
const FORMULA: [char; 4] = ['=', '+', '-', '@'];
/// Whitespace and controls a spreadsheet skips or acts on before the first character.
const LEADING: [char; 4] = [' ', '\t', '\r', '\n'];

/// Whether `text` would be read as a formula (or starts with a control).
fn dangerous(text: &str) -> bool {
    text.starts_with(['\t', '\r', '\n']) || text.trim_start_matches(LEADING).starts_with(FORMULA)
}

/// `text` quoted as needed (quotes doubled).
fn quoted(text: &str) -> String {
    if text.contains([',', '"', '\r', '\n']) || text.starts_with(' ') || text.ends_with(' ') {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text.to_owned()
    }
}

/// Text written as a cell, protected against formulas.
fn text_cell(text: &str) -> String {
    if dangerous(text) { quoted(&format!("'{text}")) } else { quoted(text) }
}

/// One cell: `null` is an empty unquoted cell, the empty text `""`; text is
/// protected against formulas; a JSON column's value is its JSON text (so
/// `"true"` and `true` stay apart), protected like text; numbers, booleans
/// and dates are written as they are.
pub(crate) fn cell(value: &Value, kind: CellKind) -> String {
    match (kind, value) {
        (_, Value::Null) => String::new(),
        (CellKind::Json, other) => text_cell(&other.to_string()),
        (_, Value::String(s)) if s.is_empty() => "\"\"".to_owned(),
        (CellKind::Text, Value::String(s)) => text_cell(s),
        (_, Value::String(s)) => quoted(s),
        (_, Value::Bool(b)) => b.to_string(),
        (_, Value::Number(n)) => n.to_string(),
        (_, other) => text_cell(&other.to_string()),
    }
}

/// A header cell (protected like text).
pub(crate) fn header(name: &str) -> String {
    text_cell(name)
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
        let text = |v: Value| cell(&v, CellKind::Text);
        assert_eq!(text(json!(null)), "");
        assert_eq!(text(json!("")), "\"\"");
        assert_eq!(text(json!("plain")), "plain");
        assert_eq!(text(json!("a,b")), "\"a,b\"");
        assert_eq!(text(json!("say \"hi\"")), "\"say \"\"hi\"\"\"");
        assert_eq!(text(json!("two\nlines")), "\"two\nlines\"");
        for formula in ["=1+1", "+1", "-2", "@SUM(A1)", "  =x", "\tx", "\rx", "\nx", " \t=1+1", "\n =x"] {
            let out = text(json!(formula));
            assert!(out.trim_start_matches('"').starts_with('\''), "{formula:?} → {out:?}");
        }
        // Numbers (also as text of numeric columns) and booleans stay as they are.
        assert_eq!(cell(&json!(-2), CellKind::Plain), "-2");
        assert_eq!(cell(&json!("-2"), CellKind::Plain), "-2");
        assert_eq!(cell(&json!(true), CellKind::Plain), "true");
        // JSON columns keep their JSON text.
        assert_eq!(cell(&json!("true"), CellKind::Json), "\"\"\"true\"\"\"");
        assert_eq!(cell(&json!(true), CellKind::Json), "true");
        assert_eq!(cell(&json!({"k": "=x"}), CellKind::Json), "\"{\"\"k\"\":\"\"=x\"\"}\"");
        assert_eq!(header("=evil"), "'=evil");
        assert_eq!(line(["a".to_owned(), "".to_owned()]), "a,\r\n");
    }
}
