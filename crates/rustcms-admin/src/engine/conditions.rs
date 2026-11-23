//! Which fields of a form are shown for given values (ADR 0037): shared by
//! resources, settings pages and action forms. The SPA evaluates the same
//! conditions (`admin-ui/src/schema/conditions.ts`); both run the cases of
//! `fixtures/conditions`.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde_json::Value;

use super::wire::{self, FieldKind};
use crate::Data;
use crate::ui::{Component, Condition, InputType, Scope};

/// How a condition reads a field's value: its kind and decimal scale.
pub(crate) type KindOf<'a> = &'a dyn Fn(&str) -> Option<(FieldKind, Option<u32>)>;

/// What a repeater row's conditions may read outside the row (ADR 0044):
/// the kinds of the enclosing row's inputs and of the record's form's; `None`
/// where there is no such level. `kind_of` answers `None` for a name that is
/// not an input there.
#[derive(Clone, Copy, Default)]
pub(crate) struct Outer<'a> {
    pub(crate) parent: Option<KindOf<'a>>,
    pub(crate) root: Option<KindOf<'a>>,
}

/// The values of the levels a row's conditions read: each as its own
/// conditions leave it (hidden inputs `null`, see [`FormConditions::shown_values`]).
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct OuterValues<'a> {
    pub(crate) parent: Option<&'a Data>,
    pub(crate) root: Option<&'a Data>,
}

/// The conditions of a form, checked at boot.
#[derive(Debug, Clone, Default)]
pub(crate) struct FormConditions {
    /// Fields whose every input is under a condition, dependencies first:
    /// the field is shown when one of the conditions holds.
    fields: Vec<ConditionalField>,
    /// The kind of each field a condition reads, by where it is.
    kinds: HashMap<(Scope, String), (FieldKind, Option<u32>)>,
}

#[derive(Debug, Clone)]
struct ConditionalField {
    name: String,
    /// One per input of the field (the input's own condition and its layouts').
    conditions: Vec<Condition>,
    clear_when_hidden: bool,
}

impl FormConditions {
    /// The conditions of `components`. Panics, naming `context`, on a
    /// condition that names no field input of the form, an operator that does
    /// not fit the field's kind (`is_true` on booleans; `eq`, `ne`, `is_in` on
    /// scalar kinds, with values of that kind), conditions depending on each
    /// other in a cycle, a condition on a `belongs_to_many` input, or
    /// `clear_when_hidden` on an input without a condition.
    pub(crate) fn new(context: &str, components: &[Component], kind_of: KindOf<'_>) -> Self {
        Self::new_in(context, components, kind_of, Outer::default())
    }

    /// The conditions of a repeater's row `components`, which may also read
    /// the levels of `outer` (`when_parent`, `when_root`); as [`Self::new`]
    /// otherwise. A condition reading a level that is not there panics.
    pub(crate) fn new_in(context: &str, components: &[Component], kind_of: KindOf<'_>, outer: Outer<'_>) -> Self {
        // Every input with its effective condition (its layouts' and its own).
        let mut inputs: Vec<(String, bool, Option<Condition>, bool)> = Vec::new();
        walk(components, None, &mut inputs);
        let names: BTreeSet<&str> = inputs.iter().map(|(name, ..)| name.as_str()).collect();

        let mut kinds = HashMap::new();
        let mut by_field: BTreeMap<String, (Vec<Condition>, bool, bool)> = BTreeMap::new();
        for (name, links, condition, clear) in &inputs {
            assert!(
                !(*clear && condition.is_none()),
                "{context}: input `{name}` has `clear_when_hidden` but no condition (`shown_when`)"
            );
            let entry = by_field.entry(name.clone()).or_insert((Vec::new(), false, false));
            entry.2 |= *clear;
            let Some(condition) = condition else {
                // Shown without condition through this input: always shown.
                entry.1 = true;
                continue;
            };
            assert!(!*links, "{context}: input `{name}`: conditions on belongs-to-many inputs are not supported");
            entry.0.push(check(context, name, condition, &names, kind_of, outer, &mut kinds));
        }
        let mut fields: Vec<ConditionalField> = by_field
            .into_iter()
            .filter(|(_, (_, always, _))| !always)
            .map(|(name, (conditions, _, clear_when_hidden))| ConditionalField { name, conditions, clear_when_hidden })
            .collect();
        order(context, &mut fields);
        Self { fields, kinds }
    }

    /// Whether field `name` is shown only under a condition.
    pub(crate) fn is_conditional(&self, name: &str) -> bool {
        self.fields.iter().any(|f| f.name == name)
    }

    /// The fields under a condition, with their `clear_when_hidden`.
    pub(crate) fn conditional(&self) -> impl Iterator<Item = (&str, bool)> {
        self.fields.iter().map(|f| (f.name.as_str(), f.clear_when_hidden))
    }

    /// Whether a condition reads outside its level (`when_parent`, `when_root`).
    pub(crate) fn reads_outside(&self) -> bool {
        self.kinds.keys().any(|(scope, _)| *scope != Scope::Row)
    }

    /// The fields hidden for `values` (missing values count as `null`; a
    /// hidden field's value counts as `null` for the conditions after it).
    pub(crate) fn hidden(&self, values: &Data) -> BTreeSet<String> {
        self.hidden_in(values, OuterValues::default())
    }

    /// As [`Self::hidden`], for a row whose conditions may read `outer`
    /// (missing levels and values count as `null`).
    pub(crate) fn hidden_in(&self, values: &Data, outer: OuterValues<'_>) -> BTreeSet<String> {
        let mut hidden = BTreeSet::new();
        for field in &self.fields {
            let value = |scope: Scope, name: &str| -> Value {
                let level = match scope {
                    Scope::Row if hidden.contains(name) => return Value::Null,
                    Scope::Row => Some(values),
                    Scope::Parent => outer.parent,
                    Scope::Root => outer.root,
                };
                level.and_then(|l| l.get(name)).cloned().unwrap_or(Value::Null)
            };
            if !field.conditions.iter().any(|c| self.holds(c, &value)) {
                hidden.insert(field.name.clone());
            }
        }
        hidden
    }

    /// `values` with the fields hidden by these conditions set to `null`: what
    /// the conditions of the rows below read from this level.
    pub(crate) fn shown_values(&self, values: &Data, outer: OuterValues<'_>) -> Data {
        let mut out = values.clone();
        for name in self.hidden_in(values, outer) {
            out.insert(name, Value::Null);
        }
        out
    }

    fn holds(&self, condition: &Condition, value: &dyn Fn(Scope, &str) -> Value) -> bool {
        let compared = |scope: Scope, name: &str| self.canonical(scope, name, &value(scope, name));
        match condition {
            Condition::Eq { name, scope, value: expected } => compared(*scope, name) == *expected,
            Condition::Ne { name, scope, value: expected } => compared(*scope, name) != *expected,
            Condition::IsIn { name, scope, values } => values.contains(&compared(*scope, name)),
            Condition::Filled { name, scope } => filled(&value(*scope, name)),
            Condition::IsTrue { name, scope } => value(*scope, name) == Value::Bool(true),
            Condition::All { conditions } => conditions.iter().all(|c| self.holds(c, value)),
            Condition::Any { conditions } => conditions.iter().any(|c| self.holds(c, value)),
        }
    }

    /// `value` as compared for field `name` of level `scope` (as is when it
    /// does not decode: it then equals no declared value).
    fn canonical(&self, scope: Scope, name: &str, value: &Value) -> Value {
        match self.kinds.get(&(scope, name.to_owned())) {
            Some((kind, _)) if !value.is_null() => comparable(*kind, value).unwrap_or(value.clone()),
            _ => value.clone(),
        }
    }
}

/// `value` as conditions compare it: its canonical wire form, decimals as
/// written without trailing zeros and never rounded to the column's scale
/// (`"10.50"` is `"10.5"`, `"10.504"` stays itself), as the SPA compares them.
fn comparable(kind: FieldKind, value: &Value) -> Result<Value, wire::DecodeError> {
    let canonical = wire::canonical(kind, None, value)?;
    Ok(match (kind, &canonical) {
        (FieldKind::Decimal, Value::String(text)) => match text.parse::<rust_decimal::Decimal>() {
            Ok(d) => Value::String(d.normalize().to_string()),
            Err(_) => canonical,
        },
        _ => canonical,
    })
}

/// Set: not `null`, `""` (or blank), `[]` or `{}`.
fn filled(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::String(s) => !s.trim().is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
        _ => true,
    }
}

/// Rewrites every condition of `components` with its values in canonical
/// form (UUIDs hyphenated in lower case, decimals without trailing zeros),
/// so that schemas send the panel what the server compares. Run after
/// [`FormConditions::new`] checked them: a value that does not read as its
/// field's kind is left as written.
/// The conditions of a JSON repeater's rows (at any depth) that read the
/// record's form are rewritten too (`when_root`, with `kind_of`); the others
/// are left as written.
pub(crate) fn canonicalize(components: &mut [Component], kind_of: KindOf<'_>) {
    canonicalize_with(components, &|scope, name| if scope == Scope::Row { kind_of(name) } else { None }, kind_of);
}

/// How a level's conditions read the kinds of the inputs they name, by where
/// they are (`None`: left as written).
pub(crate) type ScopedKindOf<'a> = &'a dyn Fn(Scope, &str) -> Option<(FieldKind, Option<u32>)>;

/// Rewrites the conditions of `components` (one level, read through
/// `kind_of`) with their values in canonical form, and in the rows of the
/// JSON repeaters among them those reading the record's form (`form`).
pub(crate) fn canonicalize_with(components: &mut [Component], kind_of: ScopedKindOf<'_>, form: KindOf<'_>) {
    for component in components {
        match component {
            Component::Input(input) => {
                if let Some(condition) = input.shown_when.as_deref_mut() {
                    *condition = canonical_condition(condition, kind_of);
                }
                if let Some(spec) = input.repeater.as_deref_mut().filter(|s| !s.relationship) {
                    let rows = |scope: Scope, name: &str| if scope == Scope::Root { form(name) } else { None };
                    canonicalize_with(&mut spec.components, &rows, form);
                }
            }
            Component::Layout(layout) => {
                if let Some(condition) = layout.shown_when.as_deref_mut() {
                    *condition = canonical_condition(condition, kind_of);
                }
                canonicalize_with(&mut layout.children, kind_of, form);
            }
            Component::Entry(entry) => {
                if let Some(condition) = entry.shown_when.as_deref_mut() {
                    *condition = canonical_condition(condition, kind_of);
                }
            }
        }
    }
}

fn canonical_condition(condition: &Condition, kind_of: ScopedKindOf<'_>) -> Condition {
    let value_of = |name: &str, scope: Scope, value: &Value| -> Value {
        match kind_of(scope, name) {
            Some((kind, _)) if !value.is_null() => comparable(kind, value).unwrap_or_else(|_| value.clone()),
            _ => value.clone(),
        }
    };
    match condition {
        Condition::Eq { name, scope, value } => {
            Condition::Eq { name: name.clone(), scope: *scope, value: value_of(name, *scope, value) }
        }
        Condition::Ne { name, scope, value } => {
            Condition::Ne { name: name.clone(), scope: *scope, value: value_of(name, *scope, value) }
        }
        Condition::IsIn { name, scope, values } => Condition::IsIn {
            name: name.clone(),
            scope: *scope,
            values: values.iter().map(|v| value_of(name, *scope, v)).collect(),
        },
        Condition::All { conditions } => {
            Condition::All { conditions: conditions.iter().map(|c| canonical_condition(c, kind_of)).collect() }
        }
        Condition::Any { conditions } => {
            Condition::Any { conditions: conditions.iter().map(|c| canonical_condition(c, kind_of)).collect() }
        }
        other => other.clone(),
    }
}

/// Whether `components` declare a condition (an input's, an entry's or a
/// layout's `shown_when`), at any depth.
pub(crate) fn declares_conditions(components: &[Component]) -> bool {
    let mut inputs = Vec::new();
    walk(components, None, &mut inputs);
    inputs.iter().any(|(_, _, condition, _)| condition.is_some())
}

fn walk(
    components: &[Component],
    inherited: Option<&Condition>,
    out: &mut Vec<(String, bool, Option<Condition>, bool)>,
) {
    let combine = |own: Option<&Condition>| match (inherited, own) {
        (Some(a), Some(b)) => Some(a.clone().and(b.clone())),
        (a, b) => a.or(b).cloned(),
    };
    for component in components {
        match component {
            Component::Input(input) => {
                out.push((
                    input.name.clone(),
                    input.kind == InputType::BelongsToMany,
                    combine(input.shown_when.as_deref()),
                    input.clear_when_hidden,
                ));
            }
            // An infolist's entries (checked as a form's inputs, ADR 0039).
            Component::Entry(entry) => {
                out.push((entry.name.clone(), false, combine(entry.shown_when.as_deref()), false))
            }
            Component::Layout(layout) => walk(&layout.children, combine(layout.shown_when.as_deref()).as_ref(), out),
        }
    }
}

/// Checks `condition` of input `input` and returns it with its values in
/// canonical form.
fn check(
    context: &str,
    input: &str,
    condition: &Condition,
    names: &BTreeSet<&str>,
    kind_of: KindOf<'_>,
    outer: Outer<'_>,
    kinds: &mut HashMap<(Scope, String), (FieldKind, Option<u32>)>,
) -> Condition {
    let mut kind = |scope: Scope, name: &str| -> (FieldKind, Option<u32>) {
        let found = match scope {
            Scope::Row => {
                assert!(
                    names.contains(name),
                    "{context}: the condition of `{input}` names `{name}`, which is not an input (or entry) of the \
                     form"
                );
                kind_of(name).unwrap_or_else(|| {
                    panic!("{context}: the condition of `{input}` names `{name}`, whose value conditions cannot read")
                })
            }
            Scope::Parent | Scope::Root => {
                let (level, starter) = match scope {
                    Scope::Parent => (outer.parent, "when_parent"),
                    _ => (outer.root, "when_root"),
                };
                let level = level.unwrap_or_else(|| {
                    panic!(
                        "{context}: the condition of `{input}` uses `{starter}(\"{name}\")`, which reads {} (only in \
                         {})",
                        if scope == Scope::Parent { "the enclosing row" } else { "the record's form" },
                        if scope == Scope::Parent { "rows of a repeater nested in a row" } else { "repeater rows" },
                    )
                });
                level(name).unwrap_or_else(|| {
                    panic!(
                        "{context}: the condition of `{input}` uses `{starter}(\"{name}\")`, which is not an input \
                         of that level whose value conditions can read"
                    )
                })
            }
        };
        kinds.insert((scope, name.to_owned()), found);
        found
    };
    let mut value_of = |scope: Scope, name: &str, value: &Value| -> Value {
        let (field_kind, _) = kind(scope, name);
        assert!(
            matches!(
                field_kind,
                FieldKind::String
                    | FieldKind::I32
                    | FieldKind::I64
                    | FieldKind::F64
                    | FieldKind::Decimal
                    | FieldKind::Bool
                    | FieldKind::Uuid
                    | FieldKind::Date
            ),
            "{context}: the condition of `{input}` compares `{name}`, whose kind ({}) has no comparable values",
            field_kind.name()
        );
        if value.is_null() {
            return Value::Null;
        }
        comparable(field_kind, value).unwrap_or_else(|_| {
            panic!("{context}: the condition of `{input}` compares `{name}` with {value}, not a value of its kind")
        })
    };
    match condition {
        Condition::Eq { name, scope, value } => {
            Condition::Eq { name: name.clone(), scope: *scope, value: value_of(*scope, name, value) }
        }
        Condition::Ne { name, scope, value } => {
            Condition::Ne { name: name.clone(), scope: *scope, value: value_of(*scope, name, value) }
        }
        Condition::IsIn { name, scope, values } => {
            assert!(!values.is_empty(), "{context}: the condition of `{input}` lists no values for `{name}`");
            Condition::IsIn {
                name: name.clone(),
                scope: *scope,
                values: values.iter().map(|v| value_of(*scope, name, v)).collect(),
            }
        }
        Condition::Filled { name, scope } => {
            kind(*scope, name);
            condition.clone()
        }
        Condition::IsTrue { name, scope } => {
            let (field_kind, _) = kind(*scope, name);
            assert!(
                field_kind == FieldKind::Bool,
                "{context}: the condition of `{input}` uses `is_true` on `{name}`, which is not a boolean"
            );
            condition.clone()
        }
        Condition::All { conditions } | Condition::Any { conditions } => {
            assert!(!conditions.is_empty(), "{context}: the condition of `{input}` combines no conditions");
            let checked = conditions.iter().map(|c| check(context, input, c, names, kind_of, outer, kinds)).collect();
            match condition {
                Condition::All { .. } => Condition::All { conditions: checked },
                _ => Condition::Any { conditions: checked },
            }
        }
    }
}

/// Sorts `fields` so that a field comes after the conditional fields its
/// conditions read; panics on a cycle.
fn order(context: &str, fields: &mut Vec<ConditionalField>) {
    let index: HashMap<String, usize> = fields.iter().enumerate().map(|(i, f)| (f.name.clone(), i)).collect();
    let depends: Vec<Vec<usize>> = fields
        .iter()
        .map(|f| {
            let mut deps: Vec<usize> =
            // Only the same level orders: the levels around a row never read it.
                f.conditions
                    .iter()
                    .flat_map(|c| c.names())
                    .filter(|(scope, _)| *scope == Scope::Row)
                    .filter_map(|(_, n)| index.get(n).copied())
                    .collect();
            deps.sort_unstable();
            deps.dedup();
            deps
        })
        .collect();
    // 0: not visited, 1: in progress, 2: done.
    let mut state = vec![0u8; fields.len()];
    let mut sorted = Vec::with_capacity(fields.len());
    fn visit(
        i: usize,
        context: &str,
        fields: &[ConditionalField],
        depends: &[Vec<usize>],
        state: &mut [u8],
        sorted: &mut Vec<usize>,
    ) {
        match state[i] {
            2 => return,
            1 => panic!("{context}: the conditions of `{}` depend on each other in a cycle", fields[i].name),
            _ => {}
        }
        state[i] = 1;
        for &d in &depends[i] {
            visit(d, context, fields, depends, state, sorted);
        }
        state[i] = 2;
        sorted.push(i);
    }
    for i in 0..fields.len() {
        visit(i, context, fields, &depends, &mut state, &mut sorted);
    }
    let mut taken: Vec<Option<ConditionalField>> = std::mem::take(fields).into_iter().map(Some).collect();
    *fields = sorted.into_iter().map(|i| taken[i].take().expect("each field once")).collect();
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::ui::{Section, TextInput, Toggle, when};

    #[test]
    fn schemas_get_canonical_condition_values() {
        let kind_of = |name: &str| match name {
            "owner" => Some((FieldKind::Uuid, None)),
            "price" => Some((FieldKind::Decimal, Some(2))),
            _ => None,
        };
        let upper = "{0C7E1D2A-5B3F-4E6A-9D8C-1A2B3C4D5E6F}";
        let mut components = vec![
            Component::Input(TextInput::make("a").shown_when(when("owner").eq(upper))),
            Component::Layout(Section::make("S").shown_when(Condition::any([
                when("owner").is_in(["0C7E1D2A5B3F4E6A9D8C1A2B3C4D5E6F"]),
                when("price").ne("10.50"),
            ]))),
        ];
        canonicalize(&mut components, &kind_of);
        let canonical = json!("0c7e1d2a-5b3f-4e6a-9d8c-1a2b3c4d5e6f");
        let Component::Input(input) = &components[0] else { panic!() };
        assert_eq!(serde_json::to_value(input.shown_when.as_deref().unwrap()).unwrap()["value"], canonical);
        let Component::Layout(layout) = &components[1] else { panic!() };
        let section = serde_json::to_value(layout.shown_when.as_deref().unwrap()).unwrap();
        assert_eq!(section["conditions"][0]["values"], json!([canonical]));
        assert_eq!(section["conditions"][1]["value"], json!("10.5"));

        // Infolist entries too.
        let mut entries: Vec<Component> =
            crate::components![crate::ui::TextEntry::make("a").shown_when(when("owner").eq(upper))];
        canonicalize(&mut entries, &kind_of);
        let Component::Entry(entry) = &entries[0] else { panic!() };
        assert_eq!(serde_json::to_value(entry.shown_when.as_deref().unwrap()).unwrap()["value"], canonical);
    }

    fn kinds(name: &str) -> Option<(FieldKind, Option<u32>)> {
        match name {
            "on_sale" | "featured" => Some((FieldKind::Bool, None)),
            "price" => Some((FieldKind::Decimal, Some(2))),
            "count" => Some((FieldKind::I64, None)),
            _ => Some((FieldKind::String, None)),
        }
    }

    fn data(value: Value) -> Data {
        value.as_object().unwrap().clone()
    }

    #[test]
    fn hides_inputs_and_layouts_whose_conditions_fail() {
        let components = crate::components![
            TextInput::make("type"),
            TextInput::make("url").shown_when(when("type").eq("external")),
            Toggle::make("on_sale"),
            Section::make("Sale")
                .shown_when(when("on_sale").is_true())
                .schema(crate::components![TextInput::make("code").shown_when(when("type").ne("internal"))]),
        ];
        let conditions = FormConditions::new("t", &components, &kinds);
        let hidden = |v: Value| conditions.hidden(&data(v)).into_iter().collect::<Vec<_>>();
        assert_eq!(hidden(json!({"type": "external", "on_sale": true})), Vec::<String>::new());
        assert_eq!(hidden(json!({"type": "internal", "on_sale": true})), ["code", "url"]);
        assert_eq!(hidden(json!({"type": "external", "on_sale": false})), ["code"]);
        assert_eq!(hidden(json!({})), ["code", "url"], "missing values are null");
        assert!(conditions.is_conditional("url") && !conditions.is_conditional("type"));
    }

    #[test]
    fn compares_canonical_values_and_chains_hidden_fields() {
        let components = crate::components![
            TextInput::make("price"),
            TextInput::make("count"),
            Toggle::make("featured").shown_when(when("price").is_in(["10.5", "20"])),
            TextInput::make("badge").shown_when(when("featured").is_true()),
            TextInput::make("note").shown_when(when("count").eq("7")),
        ];
        let conditions = FormConditions::new("t", &components, &kinds);
        let hidden = |v: Value| conditions.hidden(&data(v)).into_iter().collect::<Vec<_>>();
        // Decimals compare by value: "10.50" is "10.5", "20.00" is "20".
        assert_eq!(hidden(json!({"price": "10.50", "featured": true, "count": "007"})), Vec::<String>::new());
        // `featured` hidden: its stored `true` counts as null, so `badge` is hidden too.
        assert_eq!(hidden(json!({"price": "1", "featured": true, "count": "7"})), ["badge", "featured"]);
    }

    #[test]
    fn a_field_with_an_unconditional_input_is_always_shown() {
        let components = crate::components![
            Toggle::make("on_sale"),
            TextInput::make("code").shown_when(when("on_sale").is_true()),
            Section::make("Other").schema(crate::components![TextInput::make("code")]),
        ];
        assert!(!FormConditions::new("t", &components, &kinds).is_conditional("code"));
    }

    /// The cases shared with the SPA (`admin-ui/tests/conditions.test.ts`).
    #[test]
    fn shared_cases() {
        let cases: Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/conditions/cases.json"
        )))
        .unwrap();
        fn components(list: &Value, kinds: &mut HashMap<String, String>) -> Vec<Component> {
            list.as_array()
                .unwrap()
                .iter()
                .map(|c| {
                    let condition = c.get("shownWhen").map(|w| serde_json::from_value::<Condition>(w.clone()).unwrap());
                    if let Some(name) = c["name"].as_str() {
                        kinds.insert(name.to_owned(), c["valueType"].as_str().unwrap().to_owned());
                        let input = TextInput::make(name);
                        Component::Input(match condition {
                            Some(condition) => input.shown_when(condition),
                            None => input,
                        })
                    } else {
                        let section = Section::make("s").schema(components(&c["components"], kinds));
                        Component::Layout(match condition {
                            Some(condition) => section.shown_when(condition),
                            None => section,
                        })
                    }
                })
                .collect()
        }
        for form in cases["forms"].as_array().unwrap() {
            let mut kinds = HashMap::new();
            let components = components(&form["components"], &mut kinds);
            let kind = |name: &str| -> (FieldKind, Option<u32>) {
                match name {
                    "bool" => (FieldKind::Bool, None),
                    "i32" => (FieldKind::I32, None),
                    "i64" => (FieldKind::I64, None),
                    "decimal" => (FieldKind::Decimal, Some(2)),
                    "uuid" => (FieldKind::Uuid, None),
                    _ => (FieldKind::String, None),
                }
            };
            let kind_of = |name: &str| kinds.get(name).map(|k| kind(k));
            // A repeater's row reading the record's form (ADR 0044).
            let root_kinds = form["root"]["kinds"].as_object().cloned();
            let root_of = |name: &str| root_kinds.as_ref()?.get(name)?.as_str().map(kind);
            let outer = Outer { parent: None, root: root_kinds.is_some().then_some(&root_of as KindOf<'_>) };
            let conditions = FormConditions::new_in("t", &components, &kind_of, outer);
            for case in form["cases"].as_array().unwrap() {
                let root = case.get("root").map(|r| data(r.clone()));
                let outer = OuterValues { parent: None, root: root.as_ref() };
                let hidden: Vec<String> =
                    conditions.hidden_in(&data(case["values"].clone()), outer).into_iter().collect();
                let expected: Vec<String> = serde_json::from_value(case["hidden"].clone()).unwrap();
                assert_eq!(hidden, expected, "{}: {}", form["name"], case["values"]);
            }
        }
    }

    #[test]
    #[should_panic(expected = "names `nope`, which is not an input (or entry) of the form")]
    fn names_must_be_inputs() {
        let components = crate::components![TextInput::make("a").shown_when(when("nope").filled())];
        FormConditions::new("t", &components, &kinds);
    }

    #[test]
    #[should_panic(expected = "uses `is_true` on `type`, which is not a boolean")]
    fn is_true_needs_a_boolean() {
        let components =
            crate::components![TextInput::make("type"), TextInput::make("a").shown_when(when("type").is_true())];
        FormConditions::new("t", &components, &kinds);
    }

    #[test]
    #[should_panic(expected = "not a value of its kind")]
    fn values_must_decode() {
        let components =
            crate::components![TextInput::make("count"), TextInput::make("a").shown_when(when("count").eq("x"))];
        FormConditions::new("t", &components, &kinds);
    }

    #[test]
    #[should_panic(expected = "depend on each other in a cycle")]
    fn cycles_are_refused() {
        let components = crate::components![
            TextInput::make("a").shown_when(when("b").filled()),
            TextInput::make("b").shown_when(when("a").filled()),
        ];
        FormConditions::new("t", &components, &kinds);
    }

    #[test]
    #[should_panic(expected = "has `clear_when_hidden` but no condition")]
    fn clearing_needs_a_condition() {
        let components = crate::components![TextInput::make("a").clear_when_hidden()];
        FormConditions::new("t", &components, &kinds);
    }
}
