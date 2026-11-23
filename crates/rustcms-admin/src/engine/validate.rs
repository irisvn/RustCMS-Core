//! Checks of one input value against its declaration, shared by resources,
//! settings and action forms: the same rules and messages everywhere.

use std::cmp::Ordering;

use rust_decimal::{Decimal, RoundingStrategy};
use rustcms_core::ValidationErrors;
use serde_json::Value;

use super::wire::FieldKind;
use crate::ui::NumberBounds;

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
    /// The application nodes it may hold (none: `None`).
    pub rich_nodes: Option<crate::rich_text::Accepts<'a>>,
    /// The values allowed (a select or a checkbox list).
    pub options: Option<Options<'a>>,
    /// The range of a number input (`min`, `max`; the step is not checked).
    pub bounds: Option<Bounds>,
}

/// The range a number must be in, and the scale its column rounds it to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Bounds {
    /// `min`, `max` (and the step, not checked).
    pub range: NumberBounds,
    /// The column's decimal scale, if any: a decimal is checked as sent and
    /// as the database may round it.
    pub scale: Option<u32>,
    /// `min` and `max` as decimals, read once from the bounds as written.
    exact: (Option<Decimal>, Option<Decimal>),
    /// The column stores an `f32`: a number is checked as that `f32` (what
    /// is read back) against the bounds as `f32`s, so a value read back is
    /// accepted again (`0.1` reads back as `0.10000000149…` from an `f32`).
    single: bool,
}

impl Bounds {
    pub(crate) fn new(range: NumberBounds, scale: Option<u32>) -> Self {
        Self { range, scale, exact: (range.min.and_then(exact), range.max.and_then(exact)), single: false }
    }

    /// The same bounds for an `f32` column.
    pub(crate) fn single(self, single: bool) -> Self {
        Self { single, ..self }
    }
}

/// `bound` as a decimal, when a decimal holds it as written (the shortest
/// text that reads back as the `f64`: `0.1` is 0.1); `None` beyond a
/// decimal's range or precision (`1e30`, `1e-30`).
fn exact(bound: f64) -> Option<Decimal> {
    let text = bound.to_string();
    // `-0` reads as 0.
    text.parse::<Decimal>().ok().filter(|d| d.is_zero() && bound == 0.0 || d.normalize().to_string() == text)
}

/// The static options a value is checked against.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Options<'a> {
    /// One of these values (a select). The value matches as sent or in its
    /// canonical wire form (decoded, then encoded as stored: decimals at
    /// `scale`), so declarations given in canonical form accept every
    /// spelling of the same value (`"0012"` and `"12"` on an `i64`).
    One {
        /// The values allowed.
        values: &'a [Value],
        /// The column's decimal scale, if any.
        scale: Option<u32>,
    },
    /// A list of these values (a checkbox list), each at most once; items are
    /// compared as JSON values; `[]` is a value.
    Many(&'a [Value]),
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
            rich_nodes: None,
            options: None,
            bounds: None,
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
        shape_problem(rule, value).or_else(|| option_problem(rule, value)).or_else(|| bounds_problem(rule, value))
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
    } else if let Some(Err(e)) = rule.rich_text.then(|| crate::rich_text::parse_with(value, rule.rich_nodes)) {
        Some(e.0)
    } else if let (Some(max), Some(text)) = (rule.max_length, value.as_str())
        && text.chars().count() > max
    {
        Some(format!("may not be longer than {max} characters"))
    } else {
        None
    }
}

/// Whether a checkbox list's value has its shape whatever its options: a
/// list, each item at most once (inputs with options computed on the
/// server, whose membership is checked against their answer, ADR 0044 part E).
pub(crate) fn list_shape_problem(value: &Value) -> Option<String> {
    let Some(items) = value.as_array() else { return Some("is invalid".to_owned()) };
    let mut seen = std::collections::HashSet::new();
    (!items.iter().all(|item| seen.insert(item.to_string()))).then(|| "may only be given once".to_owned())
}

/// Membership in `rule.options` (none: `None`).
pub(crate) fn option_problem(rule: &InputRule<'_>, value: &Value) -> Option<String> {
    const NOT_OPTION: &str = "is not one of the options";
    match rule.options? {
        Options::One { values, scale } => {
            let canonical = || super::wire::canonical(rule.kind, scale, value).ok();
            (!values.contains(value) && !canonical().is_some_and(|c| values.contains(&c)))
                .then(|| NOT_OPTION.to_owned())
        }
        Options::Many(values) => {
            let Some(items) = value.as_array() else { return Some("is invalid".to_owned()) };
            if items.iter().any(|item| !values.contains(item)) {
                Some(NOT_OPTION.to_owned())
            } else if items.len() > values.len() || items.iter().enumerate().any(|(i, item)| items[..i].contains(item))
            {
                // More items than options: one is repeated (and the search stays short).
                Some("may only be given once".to_owned())
            } else {
                None
            }
        }
    }
}

/// A number below `min` or above `max`. Integers and decimals are compared
/// exactly with the bound as written (`0.1` is 0.1, not the nearest `f64`);
/// a decimal is checked as sent and rounded to its column's scale both ways
/// databases round (PostgreSQL and MySQL away from zero, the wire to even),
/// so a value that passes is never stored out of the range.
fn bounds_problem(rule: &InputRule<'_>, value: &Value) -> Option<String> {
    let Bounds { range, scale, exact: (min_exact, max_exact), single } = rule.bounds?;
    let decoded = rule.kind.decode(value).ok()?.into_iter().next()?;
    let candidates: Vec<Number> = match decoded {
        sea_orm::Value::Int(Some(n)) => vec![Number::Exact(Decimal::from(n))],
        sea_orm::Value::BigInt(Some(n)) => vec![Number::Exact(Decimal::from(n))],
        sea_orm::Value::Double(Some(n)) if single => vec![Number::Single(n as f32)],
        sea_orm::Value::Double(Some(n)) => vec![Number::Float(n)],
        sea_orm::Value::Decimal(Some(d)) => {
            let mut out = vec![Number::Exact(d)];
            if let Some(scale) = scale {
                out.push(Number::Exact(d.round_dp_with_strategy(scale, RoundingStrategy::MidpointAwayFromZero)));
                out.push(Number::Exact(d.round_dp_with_strategy(scale, RoundingStrategy::MidpointNearestEven)));
            }
            out
        }
        _ => return None,
    };
    let outside = |bound: f64, exact: Option<Decimal>, side: Ordering| {
        candidates.iter().any(|n| n.cmp_bound(bound, exact) == side)
    };
    if let Some(min) = range.min.filter(|min| outside(*min, min_exact, Ordering::Less)) {
        Some(format!("must be at least {}", number_text(min)))
    } else {
        range
            .max
            .filter(|max| outside(*max, max_exact, Ordering::Greater))
            .map(|max| format!("may not be greater than {}", number_text(max)))
    }
}

/// A decoded number: exact (integers, decimals), a float, or a float as an
/// `f32` column stores it.
enum Number {
    Exact(Decimal),
    Float(f64),
    Single(f32),
}

impl Number {
    /// Compares with `bound`; an exact number with the bound's decimal
    /// (`exact`: always there on integer and decimal columns, the boot
    /// refuses bounds they cannot compare exactly).
    fn cmp_bound(&self, bound: f64, exact: Option<Decimal>) -> Ordering {
        match (self, exact) {
            (Self::Exact(n), Some(bound)) => n.cmp(&bound),
            (Self::Exact(n), None) => {
                f64::try_from(*n).ok().and_then(|n| n.partial_cmp(&bound)).unwrap_or(Ordering::Equal)
            }
            (Self::Float(n), _) => n.partial_cmp(&bound).unwrap_or(Ordering::Equal),
            (Self::Single(n), _) => n.partial_cmp(&(bound as f32)).unwrap_or(Ordering::Equal),
        }
    }
}

/// Checks the bounds of an `f32` column: compared as `f32`s, they must stay
/// finite and not round to 0 (`1e-50` would bound nothing).
pub(crate) fn check_single_bounds(context: &str, name: &str, bounds: &NumberBounds) {
    assert!(
        [bounds.min, bounds.max].into_iter().flatten().all(|n| {
            let single = n as f32;
            single.is_finite() && (n == 0.0 || single != 0.0)
        }),
        "{context}: number input `{name}` stores an `f32`: its bounds must be finite and not round to 0 as `f32`s"
    );
}

/// Checks a declaration of bounds for a value of `kind`; panics with
/// `context` on one that cannot hold: a bound or step that is not a finite
/// number, `min` above `max`, a step that is not positive, a bound or step
/// that is not whole on an integer, a bound a decimal cannot hold as written
/// on an integer or decimal (compared exactly).
pub(crate) fn check_bounds(context: &str, name: &str, bounds: &NumberBounds, kind: FieldKind) {
    let all = || [bounds.min, bounds.max, bounds.step].into_iter().flatten();
    assert!(
        all().all(f64::is_finite),
        "{context}: number input `{name}` has a bound or step that is not a finite number"
    );
    assert!(
        !matches!((bounds.min, bounds.max), (Some(min), Some(max)) if min > max),
        "{context}: number input `{name}` has `min` above `max`"
    );
    assert!(bounds.step.is_none_or(|step| step > 0.0), "{context}: number input `{name}` needs a positive step");
    if matches!(kind, FieldKind::I32 | FieldKind::I64) {
        assert!(
            all().all(|n| n.fract() == 0.0),
            "{context}: number input `{name}` holds integers: its bounds and step must be whole numbers"
        );
    }
    if matches!(kind, FieldKind::I32 | FieldKind::I64 | FieldKind::Decimal) {
        assert!(
            [bounds.min, bounds.max].into_iter().flatten().all(|n| exact(n).is_some()),
            "{context}: number input `{name}` has a bound a decimal cannot hold as written"
        );
    }
}

/// A bound as people write it: `5`, not `5.0`.
pub(crate) fn number_text(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < 1e15 { format!("{}", value as i64) } else { value.to_string() }
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
pub(crate) fn key_value_problem(value: &Value, (max_items, max_length): (usize, usize)) -> Option<String> {
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
        let select = one(FieldKind::String, &options, None);
        assert_eq!(first(&select, json!("a")), None);
        assert_eq!(first(&select, json!("c")).as_deref(), Some("is not one of the options"));
        assert_eq!(first(&select, Value::Null), None, "null is for nullable and required");
        assert_eq!(first(&InputRule { required: true, ..select }, Value::Null).as_deref(), Some("is required"));

        let kv = InputRule { key_value: Some((1, 2)), ..InputRule::of(FieldKind::Json) };
        assert_eq!(first(&kv, json!({"a": "xy"})), None);
        assert_eq!(first(&kv, json!({"a": "x", "b": "y"})).as_deref(), Some("may not have more than 1 items"));
        assert_eq!(first(&kv, json!({" ": "x"})).as_deref(), Some("may not have an empty key"));
        assert_eq!(first(&kv, json!({"a": 1})).as_deref(), Some("must be an object of text values"));
        assert_eq!(first(&kv, json!({"a": "xyz"})).as_deref(), Some("may not have values longer than 2 characters"));
    }

    #[test]
    fn many_options() {
        let options = [json!("red"), json!("green"), json!(3)];
        let list = InputRule { options: Some(Options::Many(&options)), ..InputRule::of(FieldKind::Json) };
        assert_eq!(first(&list, json!([])), None);
        assert_eq!(first(&list, json!(["red", "green", 3])), None);
        assert_eq!(first(&list, json!(["green"])), None);
        assert_eq!(first(&list, json!(["red", "pink"])).as_deref(), Some("is not one of the options"));
        assert_eq!(first(&list, json!(["3"])).as_deref(), Some("is not one of the options"), "JSON values");
        assert_eq!(first(&list, json!(["red", "red"])).as_deref(), Some("may only be given once"));
        assert_eq!(first(&list, json!(["red", "green", 3, "green"])).as_deref(), Some("may only be given once"));
        assert_eq!(first(&list, json!("red")).as_deref(), Some("is invalid"));
        assert_eq!(first(&list, json!({"red": true})).as_deref(), Some("is invalid"));
        assert_eq!(first(&list, Value::Null), None);
    }

    fn one(kind: FieldKind, values: &[Value], scale: Option<u32>) -> InputRule<'_> {
        InputRule { options: Some(Options::One { values, scale }), ..InputRule::of(kind) }
    }

    #[test]
    fn one_option_compares_canonical_values() {
        let values = [json!("12")];
        let rule = one(FieldKind::I64, &values, None);
        assert_eq!(first(&rule, json!("12")), None);
        assert_eq!(first(&rule, json!("0012")), None);
        assert_eq!(first(&rule, json!("13")).as_deref(), Some("is not one of the options"));
        // The wire types stay strict: a number is not an `i64` value.
        assert_eq!(first(&rule, json!(12)).as_deref(), Some("must be a string"));

        let values = [json!("10.50")];
        let rule = one(FieldKind::Decimal, &values, Some(2));
        assert_eq!(first(&rule, json!("10.5")), None);
        assert_eq!(first(&rule, json!("10.500")), None);
        assert_eq!(first(&rule, json!("10.51")).as_deref(), Some("is not one of the options"));

        let values = [json!(7)];
        let rule = one(FieldKind::I32, &values, None);
        assert_eq!(first(&rule, json!(7)), None);
        assert_eq!(first(&rule, json!("7")).as_deref(), Some("must be an integer"));

        let values = [json!("2026-01-01T00:00:00Z")];
        let rule = one(FieldKind::Instant, &values, None);
        assert_eq!(first(&rule, json!("2026-01-01T02:00:00+02:00")), None);

        let values = [json!("67e55044-10b1-426f-9247-bb680e5fe0c8")];
        let rule = one(FieldKind::Uuid, &values, None);
        assert_eq!(first(&rule, json!("67E55044-10B1-426F-9247-BB680E5FE0C8")), None);
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
    #[test]
    fn checks_number_bounds_of_client_input() {
        let bounds = NumberBounds { min: Some(1.0), max: Some(5.0), step: Some(1.0) };
        let within = |range: NumberBounds, scale: Option<u32>| Some(Bounds::new(range, scale));
        let int = InputRule { bounds: within(bounds, None), ..InputRule::of(FieldKind::I32) };
        assert_eq!(first(&int, json!(1)), None);
        assert_eq!(first(&int, json!(5)), None);
        assert_eq!(first(&int, json!(0)).as_deref(), Some("must be at least 1"));
        assert_eq!(first(&int, json!(6)).as_deref(), Some("may not be greater than 5"));
        assert_eq!(first(&int, Value::Null), None, "null is for nullable and required");
        assert!(first(&int, json!("3")).is_some(), "the kind first: an i32 is a JSON number");

        // i64 and decimals travel as strings.
        let big = InputRule {
            bounds: within(NumberBounds { max: Some(10.0), ..bounds }, None),
            ..InputRule::of(FieldKind::I64)
        };
        assert_eq!(first(&big, json!("10")), None);
        assert_eq!(first(&big, json!("11")).as_deref(), Some("may not be greater than 10"));
        let price = InputRule {
            bounds: within(NumberBounds { min: Some(0.5), max: Some(4.995), step: None }, Some(2)),
            ..InputRule::of(FieldKind::Decimal)
        };
        assert_eq!(first(&price, json!("0.50")), None);
        assert_eq!(first(&price, json!("0.49")).as_deref(), Some("must be at least 0.5"));
        // 4.995 is within, but a decimal(…, 2) column stores 5.00: refused.
        assert_eq!(first(&price, json!("4.995")).as_deref(), Some("may not be greater than 4.995"));
        assert_eq!(first(&price, json!("4.99")), None);

        // Integers are compared exactly, also beyond what an f64 holds.
        let huge = InputRule {
            bounds: within(NumberBounds { max: Some(9_007_199_254_740_992.0), ..Default::default() }, None),
            ..InputRule::of(FieldKind::I64)
        };
        assert_eq!(first(&huge, json!("9007199254740992")), None);
        assert!(first(&huge, json!("9007199254740993")).is_some());
        // A bound is compared as written: 0.1 is 0.1.
        let tenth = InputRule {
            bounds: within(NumberBounds { min: Some(0.1), ..Default::default() }, None),
            ..InputRule::of(FieldKind::Decimal)
        };
        assert_eq!(first(&tenth, json!("0.1")), None);

        // An `f32` column: a number is checked as the `f32` stored, against the
        // bounds as `f32`s; what is read back (`0.1` as 0.10000000149…) is
        // accepted again.
        let ratio = InputRule {
            bounds: Some(Bounds::new(NumberBounds { min: Some(1e-30), max: Some(0.1), step: None }, None).single(true)),
            ..InputRule::of(FieldKind::F64)
        };
        let read_back = f64::from(0.1_f32);
        for (sent, accepted) in [(0.1, true), (read_back, true), (0.100_000_1, false), (1e-31, false)] {
            assert_eq!(first(&ratio, json!(sent)).is_none(), accepted, "{sent}");
        }
        // An `f64` column: compared as sent.
        let double = InputRule {
            bounds: within(NumberBounds { max: Some(0.1), ..bounds }, None),
            ..InputRule::of(FieldKind::F64)
        };
        assert!(first(&double, json!(read_back)).is_some());

        // Trusted writes (hooks) are not bounded.
        let mut errors = ValidationErrors::new();
        assert!(check_written("f", &int, &json!(9), &mut errors));
    }

    #[test]
    fn bounds_are_read_as_decimals_as_written() {
        assert_eq!(exact(0.1), Some("0.1".parse().unwrap()));
        assert_eq!(exact(5.0), Some(Decimal::from(5)));
        assert_eq!(exact(-2.5), Some("-2.5".parse().unwrap()));
        assert_eq!(exact(1e-30), None, "beyond a decimal's precision: not rounded to 0");
        assert_eq!(exact(1e30), None, "beyond a decimal's range");
    }

    #[test]
    #[should_panic(expected = "not round to 0")]
    fn single_bounds_hold_as_f32() {
        check_single_bounds("t", "ratio", &NumberBounds { min: Some(1e-50), ..Default::default() });
    }

    #[test]
    #[should_panic(expected = "must be whole numbers")]
    fn integer_bounds_are_whole() {
        check_bounds("t", "level", &NumberBounds { step: Some(0.5), ..Default::default() }, FieldKind::I32);
    }

    #[test]
    #[should_panic(expected = "a decimal cannot hold as written")]
    fn decimal_bounds_are_exact() {
        check_bounds("t", "rate", &NumberBounds { min: Some(1e-30), ..Default::default() }, FieldKind::Decimal);
    }

    #[test]
    fn float_bounds_are_free() {
        check_bounds(
            "t",
            "ratio",
            &NumberBounds { min: Some(1e-30), step: Some(0.5), ..Default::default() },
            FieldKind::F64,
        );
    }

    #[test]
    fn bounds_read_as_written() {
        assert_eq!(number_text(5.0), "5");
        assert_eq!(number_text(-2.0), "-2");
        assert_eq!(number_text(0.25), "0.25");
    }
}
