//! Checks of one input value against its declaration, shared by resources,
//! settings and action forms: the same rules and messages everywhere.

use rustcms_core::ValidationErrors;
use serde_json::Value;

use super::wire::FieldKind;

/// Longest key of a key/value object, in characters.
pub(crate) const KEY_LENGTH: usize = 255;

/// What a value must satisfy.
#[derive(Debug, Clone, Copy)]
pub(crate) struct InputRule<'a> {
    /// The wire kind (decoding).
    pub kind: FieldKind,
    /// `null` is a value.
    pub nullable: bool,
    /// Never `null`.
    pub required: bool,
    /// Longest text, in characters.
    pub max_length: Option<usize>,
    /// A key/value object: `(max items, longest value)`.
    pub key_value: Option<(usize, usize)>,
    /// A rich text document.
    pub rich_text: bool,
    /// The values allowed (a select).
    pub options: Option<&'a [Value]>,
}

impl InputRule<'_> {
    /// A rule for `kind` with no other constraint.
    #[cfg(test)]
    pub(crate) fn of(kind: FieldKind) -> Self {
        Self {
            kind,
            nullable: true,
            required: false,
            max_length: None,
            key_value: None,
            rich_text: false,
            options: None,
        }
    }
}

/// Checks client input `value` of `name` against `rule` (its kind first);
/// adds the first problem to `errors` and returns whether it is acceptable.
pub(crate) fn check_input(name: &str, rule: &InputRule<'_>, value: &Value, errors: &mut ValidationErrors) -> bool {
    let problem = if value.is_null() {
        (rule.required || !rule.nullable).then(|| "is required".to_owned())
    } else if let Err(e) = rule.kind.decode(value) {
        Some(e.0)
    } else {
        shape_problem(rule, value).or_else(|| option_problem(rule, value))
    };
    report(name, problem, errors)
}

/// Checks a value written by trusted code (hooks) before it is stored: the
/// limits first, then its kind (the order resources always used).
pub(crate) fn check_written(name: &str, rule: &InputRule<'_>, value: &Value, errors: &mut ValidationErrors) -> bool {
    let problem = if value.is_null() {
        (!rule.nullable).then(|| "is required".to_owned())
    } else {
        shape_problem(rule, value).or_else(|| rule.kind.decode(value).err().map(|e| e.0))
    };
    report(name, problem, errors)
}

/// Key/value bounds, rich text, length.
fn shape_problem(rule: &InputRule<'_>, value: &Value) -> Option<String> {
    if let Some(message) = rule.key_value.and_then(|bounds| key_value_problem(value, bounds)) {
        Some(message)
    } else if let Some(Err(e)) = rule.rich_text.then(|| crate::rich_text::parse(value)) {
        Some(e.0)
    } else if let (Some(max), Some(text)) = (rule.max_length, value.as_str())
        && text.chars().count() > max
    {
        Some(format!("may not be longer than {max} characters"))
    } else {
        None
    }
}

fn option_problem(rule: &InputRule<'_>, value: &Value) -> Option<String> {
    rule.options.is_some_and(|options| !options.contains(value)).then(|| "is not one of the options".to_owned())
}

fn report(name: &str, problem: Option<String>, errors: &mut ValidationErrors) -> bool {
    match problem {
        Some(message) => {
            errors.add(name, message);
            false
        }
        None => true,
    }
}

/// What is wrong with a key/value object, if anything.
fn key_value_problem(value: &Value, (max_items, max_length): (usize, usize)) -> Option<String> {
    let Some(object) = value.as_object() else { return Some("must be an object of text values".to_owned()) };
    if object.len() > max_items {
        Some(format!("may not have more than {max_items} items"))
    } else if object.keys().any(|k| k.trim().is_empty()) {
        Some("may not have an empty key".to_owned())
    } else if object.keys().any(|k| k.chars().count() > KEY_LENGTH) {
        Some(format!("may not have keys longer than {KEY_LENGTH} characters"))
    } else if object.values().any(|v| !v.is_string()) {
        Some("must be an object of text values".to_owned())
    } else if object.values().any(|v| v.as_str().is_some_and(|t| t.chars().count() > max_length)) {
        Some(format!("may not have values longer than {max_length} characters"))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn first(rule: &InputRule<'_>, value: Value) -> Option<String> {
        let mut errors = ValidationErrors::new();
        check_input("f", rule, &value, &mut errors);
        errors.errors.get("f").map(|m| m[0].clone())
    }

    #[test]
    fn checks_each_rule() {
        let text = InputRule { max_length: Some(3), ..InputRule::of(FieldKind::String) };
        assert_eq!(first(&text, json!("abc")), None);
        assert_eq!(first(&text, json!("abcd")).as_deref(), Some("may not be longer than 3 characters"));
        assert_eq!(first(&InputRule { required: true, ..text }, Value::Null).as_deref(), Some("is required"));
        assert_eq!(first(&InputRule { nullable: false, ..text }, Value::Null).as_deref(), Some("is required"));
        assert!(first(&text, json!(5)).is_some(), "kind");

        let options = [json!("a"), json!("b")];
        let select = InputRule { options: Some(&options), ..InputRule::of(FieldKind::String) };
        assert_eq!(first(&select, json!("a")), None);
        assert_eq!(first(&select, json!("c")).as_deref(), Some("is not one of the options"));

        let kv = InputRule { key_value: Some((1, 2)), ..InputRule::of(FieldKind::Json) };
        assert_eq!(first(&kv, json!({"a": "xy"})), None);
        assert_eq!(first(&kv, json!({"a": "x", "b": "y"})).as_deref(), Some("may not have more than 1 items"));
        assert_eq!(first(&kv, json!({" ": "x"})).as_deref(), Some("may not have an empty key"));
        assert_eq!(first(&kv, json!({"a": 1})).as_deref(), Some("must be an object of text values"));
        assert_eq!(first(&kv, json!({"a": "xyz"})).as_deref(), Some("may not have values longer than 2 characters"));
    }

    #[test]
    fn written_values_keep_their_order() {
        // Both too long and of the wrong kind: input reports the kind, written values the length.
        let rule = InputRule { max_length: Some(2), ..InputRule::of(FieldKind::I32) };
        let mut input = ValidationErrors::new();
        check_input("f", &rule, &json!("abc"), &mut input);
        let mut written = ValidationErrors::new();
        check_written("f", &rule, &json!("abc"), &mut written);
        assert_eq!(written.errors["f"], ["may not be longer than 2 characters"]);
        assert_ne!(input.errors["f"], written.errors["f"]);
        // Hooks may leave a required (nullable) column empty.
        let mut errors = ValidationErrors::new();
        assert!(check_written("f", &InputRule { required: true, ..rule }, &Value::Null, &mut errors));
    }
}
