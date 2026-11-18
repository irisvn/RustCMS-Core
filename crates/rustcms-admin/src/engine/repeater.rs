//! JSON repeaters (ADR 0040): the rows of a JSON value, each checked like a
//! form with the rules its inputs declare; shared by resources and settings.

use std::collections::HashSet;

use rustcms_core::ValidationErrors;
use serde_json::Value;

use super::conditions::FormConditions;
use super::validate::{Bounds, InputRule, Options, check_input};
use super::wire::FieldKind;
use crate::Data;
use crate::ui::{Component, Input, InputType, LayoutType, RepeaterSpec};

/// Deepest nesting of repeaters (a repeater in a row of a repeater in…).
pub(crate) const MAX_DEPTH: u8 = 3;
/// Most pairs of a key/value input in a row, and its longest value.
const KEY_VALUE: (usize, usize) = (50, 1000);

/// The rows of a JSON repeater, checked at boot.
#[derive(Debug, Clone)]
pub(crate) struct Rows {
    inputs: Vec<RowInput>,
    conditions: FormConditions,
    min_items: Option<usize>,
    max_items: Option<usize>,
}

#[derive(Debug, Clone)]
struct RowInput {
    name: String,
    rule: RowRule,
}

#[derive(Debug, Clone)]
enum RowRule {
    Value {
        kind: FieldKind,
        required: bool,
        max_length: Option<usize>,
        options: Vec<Value>,
        many: bool,
        bounds: Option<Bounds>,
        integer: bool,
        key_value: bool,
    },
    /// A custom widget or a hidden input: any JSON value.
    Any {
        required: bool,
    },
    Nested(Box<Rows>),
}

/// The wire kind of a JSON row input (what its value holds).
pub(crate) fn row_kind(input: &Input) -> FieldKind {
    match input.kind {
        InputType::TextInput | InputType::Textarea => FieldKind::String,
        InputType::NumberInput => FieldKind::F64,
        InputType::Toggle | InputType::Checkbox => FieldKind::Bool,
        InputType::DatePicker => FieldKind::Date,
        InputType::DateTimePicker => FieldKind::Instant,
        // A select holds what its options hold (text, numbers or booleans).
        InputType::Select if !input.options.is_empty() => {
            let all = |test: fn(&Value) -> bool| input.options.iter().all(|o| test(&o.value));
            if all(Value::is_string) {
                FieldKind::String
            } else if all(Value::is_number) {
                FieldKind::F64
            } else if all(Value::is_boolean) {
                FieldKind::Bool
            } else {
                FieldKind::Json
            }
        }
        _ => FieldKind::Json,
    }
}

impl Rows {
    /// The rows of `spec`, the JSON repeater `name` of `owner`; panics on
    /// what cannot hold (ADR 0040, boot checks).
    pub(crate) fn compile(owner: &str, name: &str, spec: &RepeaterSpec, depth: u8) -> Self {
        let here = format!("{owner}: repeater `{name}`");
        assert!(depth <= MAX_DEPTH, "{here}: repeaters nest {MAX_DEPTH} deep at most");
        check_spec(&here, spec);
        let mut inputs = Vec::new();
        let mut names = HashSet::new();
        for input in row_inputs(&spec.components) {
            let input_name = input.name.as_str();
            assert!(names.insert(input_name), "{here}: input `{input_name}` appears twice in a row");
            assert!(!input.clear_when_hidden, "{here}: `clear_when_hidden` does not apply inside rows");
            assert!(input.preview, "{here}: input `{input_name}`: only file uploads take `preview`");
            let rule = match input.kind {
                InputType::FileUpload | InputType::RichEditor => {
                    panic!("{here}: `{input_name}`: media and rich text inputs are not supported in JSON rows")
                }
                InputType::Secret | InputType::SecretMap | InputType::BelongsToMany => {
                    panic!("{here}: `{input_name}`: secrets and belongs-to-many inputs cannot be in a row")
                }
                InputType::Repeater => {
                    let nested = input.repeater.as_deref().expect("repeaters have a spec");
                    assert!(!nested.relationship, "{here}: `{input_name}`: a JSON row holds JSON repeaters only");
                    RowRule::Nested(Box::new(Rows::compile(&here, input_name, nested, depth + 1)))
                }
                InputType::Custom | InputType::Hidden => RowRule::Any { required: input.required },
                _ => {
                    let selects = matches!(input.kind, InputType::Select | InputType::CheckboxList);
                    assert!(
                        !selects || !input.options.is_empty(),
                        "{here}: `{input_name}`: a select in a row needs options (rows have no relationships)"
                    );
                    assert!(
                        selects || input.options.is_empty(),
                        "{here}: `{input_name}`: only selects and checkbox lists take options"
                    );
                    let number = input.kind == InputType::NumberInput;
                    assert!(
                        number || (input.number.is_none() && !input.integer),
                        "{here}: `{input_name}`: `min`, `max`, `step` and `integer` are for number inputs"
                    );
                    if let Some(range) = input.number.as_deref() {
                        let kind = if input.integer { FieldKind::I64 } else { FieldKind::F64 };
                        super::validate::check_bounds(&here, input_name, range, kind);
                    }
                    let text = matches!(input.kind, InputType::TextInput | InputType::Textarea);
                    assert!(text || input.max_length.is_none(), "{here}: `{input_name}`: `max_length` is for text");
                    RowRule::Value {
                        kind: row_kind(input),
                        required: input.required,
                        max_length: input.max_length,
                        options: input.options.iter().map(|o| o.value.clone()).collect(),
                        many: input.kind == InputType::CheckboxList,
                        bounds: input.number.as_deref().map(|range| Bounds::new(*range, None)),
                        integer: input.integer,
                        key_value: input.kind == InputType::KeyValue,
                    }
                }
            };
            inputs.push(RowInput { name: input_name.to_owned(), rule });
        }
        assert!(!inputs.is_empty(), "{here}: the row has no input");
        if let Some(template) = &spec.item_label {
            let parsed = crate::engine::title::TitleTemplate::parse(template)
                .unwrap_or_else(|e| panic!("{here}: the item label `{template}` {e}"));
            for field in parsed.fields() {
                assert!(
                    names.contains(field),
                    "{here}: the item label names `{field}`, which is not an input of the row"
                );
            }
        }
        // Conditions name inputs of the same row (ADR 0037 rules).
        let kinds: Vec<(String, FieldKind)> =
            row_inputs(&spec.components).into_iter().map(|i| (i.name.clone(), row_kind(i))).collect();
        let kind_of = |name: &str| kinds.iter().find(|(n, _)| n == name).map(|(_, k)| (*k, None));
        let conditions = FormConditions::new(&here, &spec.components, &kind_of);
        Rows {
            inputs,
            conditions,
            min_items: spec.min_items.map(|n| n as usize),
            max_items: spec.max_items.map(|n| n as usize),
        }
    }

    /// Fewest rows, when declared above 0.
    pub(crate) fn min_items(&self) -> Option<usize> {
        self.min_items.filter(|n| *n > 0)
    }

    /// Most rows, when declared.
    pub(crate) fn max_items(&self) -> Option<usize> {
        self.max_items
    }

    /// The smallest row the server may store: each required input no
    /// condition can hide, with a sample value; every other input left out
    /// (as empty or hidden inputs are). Boot checks that a settings struct
    /// reads such rows.
    pub(crate) fn probe(&self) -> Value {
        let mut row = Data::new();
        for input in &self.inputs {
            if self.conditions.is_conditional(&input.name) {
                continue;
            }
            let sample = match &input.rule {
                RowRule::Value { kind, required: true, options, many, bounds, integer, key_value, .. } => {
                    Some(if !options.is_empty() {
                        if *many { Value::Array(vec![options[0].clone()]) } else { options[0].clone() }
                    } else if *key_value {
                        Value::Object(Data::new())
                    } else {
                        match kind {
                            FieldKind::String => Value::String("x".into()),
                            FieldKind::F64 => {
                                let n = bounds.and_then(|b| b.range.min).unwrap_or(1.0);
                                if *integer { Value::from(n.ceil() as i64) } else { Value::from(n) }
                            }
                            FieldKind::Bool => Value::Bool(true),
                            FieldKind::Date => Value::String("2001-02-03".into()),
                            FieldKind::Instant => Value::String("2001-02-03T04:05:06Z".into()),
                            _ => Value::Array(Vec::new()),
                        }
                    })
                }
                RowRule::Any { required: true } => Some(Value::String("x".into())),
                RowRule::Nested(rows) => rows.min_items().map(|n| Value::Array(vec![rows.probe(); n])),
                _ => None,
            };
            if let Some(sample) = sample {
                row.insert(input.name.clone(), sample);
            }
        }
        Value::Object(row)
    }

    /// Checks `value`, the list at `path`; adds problems to `errors` and returns
    /// the list as stored (inputs hidden by their conditions removed), or
    /// `None` when it is refused. `null` is left to the field's rules, but
    /// holds no rows: refused under `min_items`.
    pub(crate) fn check(&self, path: &str, value: &Value, errors: &mut ValidationErrors) -> Option<Value> {
        let before = count(errors);
        let Some(rows) = value.as_array() else {
            match self.min_items.filter(|m| *m > 0) {
                _ if !value.is_null() => errors.add(path, "is invalid"),
                Some(min) => errors.add(path, format!("must have at least {min} items")),
                None => {}
            }
            return None;
        };
        if let Some(min) = self.min_items.filter(|min| rows.len() < *min) {
            errors.add(path, format!("must have at least {min} items"));
        }
        if let Some(max) = self.max_items.filter(|max| rows.len() > *max) {
            errors.add(path, format!("may not have more than {max} items"));
        }
        let out = self.check_each(path, 0, rows, errors);
        (count(errors) == before).then_some(Value::Array(out))
    }

    /// Whether a list of `len` rows has the number of rows declared.
    pub(crate) fn fits(&self, len: usize) -> bool {
        self.min_items.is_none_or(|min| len >= min) && self.max_items.is_none_or(|max| len <= max)
    }

    /// Checks each of `rows`, numbered from `first` in `path`; returns them
    /// as stored. The list's own bounds are the caller's.
    pub(crate) fn check_each(
        &self,
        path: &str,
        first: usize,
        rows: &[Value],
        errors: &mut ValidationErrors,
    ) -> Vec<Value> {
        let mut out = Vec::with_capacity(rows.len());
        for (index, row) in rows.iter().enumerate() {
            let index = first + index;
            let at = format!("{path}.{index}");
            let Some(row) = row.as_object() else {
                errors.add(at.as_str(), "is invalid");
                continue;
            };
            for key in row.keys().filter(|k| !self.inputs.iter().any(|i| &i.name == *k)) {
                errors.add(format!("{at}.{key}").as_str(), "is not allowed");
            }
            let hidden = self.conditions.hidden(row);
            let mut kept = Data::new();
            for input in &self.inputs {
                if hidden.contains(&input.name) {
                    continue;
                }
                let name = format!("{at}.{}", input.name);
                let value = row.get(&input.name);
                match &input.rule {
                    RowRule::Nested(rows) => {
                        if let Some(list) = rows.check(&name, value.unwrap_or(&Value::Null), errors) {
                            kept.insert(input.name.clone(), list);
                        }
                    }
                    RowRule::Any { required } => match value {
                        Some(v) if !v.is_null() => {
                            kept.insert(input.name.clone(), v.clone());
                        }
                        _ if *required => errors.add(name.as_str(), "is required"),
                        _ => {}
                    },
                    RowRule::Value { kind, required, max_length, options, many, bounds, integer, key_value } => {
                        let Some(value) = value else {
                            if *required {
                                errors.add(name.as_str(), "is required");
                            }
                            continue;
                        };
                        let rule = InputRule {
                            kind: *kind,
                            nullable: !required,
                            required: *required,
                            max_length: *max_length,
                            key_value: key_value.then_some(KEY_VALUE),
                            rich_text: false,
                            rich_nodes: None,
                            options: (!options.is_empty()).then(|| {
                                if *many {
                                    Options::Many(options)
                                } else {
                                    Options::One { values: options, scale: None }
                                }
                            }),
                            bounds: *bounds,
                        };
                        if !check_input(&name, &rule, value, errors) {
                            continue;
                        }
                        if *integer
                            && value.as_f64().is_some_and(|n| n.fract() != 0.0 || n.abs() > 9_007_199_254_740_992.0)
                        {
                            errors.add(name.as_str(), "must be an integer");
                            continue;
                        }
                        // `3.0` is stored as `3`: typed readers (`i32`) refuse a float.
                        let value = match value.as_f64() {
                            Some(n) if *integer && !value.is_i64() && !value.is_u64() => Value::from(n as i64),
                            _ => value.clone(),
                        };
                        kept.insert(input.name.clone(), value);
                    }
                }
            }
            out.push(Value::Object(kept));
        }
        out
    }
}

/// Problems reported so far (to tell whether a list added any).
fn count(errors: &ValidationErrors) -> usize {
    errors.issues().values().map(Vec::len).sum::<usize>() + errors.form_issues().len()
}

/// The inputs of a row, through its layouts (not into nested repeaters).
pub(crate) fn row_inputs(components: &[Component]) -> Vec<&Input> {
    let mut out = Vec::new();
    for component in components {
        match component {
            Component::Input(input) => out.push(input),
            Component::Layout(layout) => out.extend(row_inputs(&layout.children)),
            Component::Entry(_) => {}
        }
    }
    out
}

/// Options and layouts any repeater must respect (JSON or relationship).
pub(crate) fn check_spec(here: &str, spec: &RepeaterSpec) {
    assert!(!spec.components.is_empty(), "{here}: the row has no input (`.schema(components![…])`)");
    if let (Some(min), Some(max)) = (spec.min_items, spec.max_items) {
        assert!(min <= max, "{here}: `min_items` is above `max_items`");
    }
    if let Some(max) = spec.max_items {
        assert!(spec.default_items <= max, "{here}: `default_items` is above `max_items`");
    }
    fn layouts(here: &str, components: &[Component]) {
        for component in components {
            match component {
                Component::Layout(layout) => {
                    assert!(
                        matches!(layout.kind, LayoutType::Section | LayoutType::Grid),
                        "{here}: a row holds sections and grids, not tabs or splits"
                    );
                    layouts(here, &layout.children);
                }
                Component::Entry(_) => panic!("{here}: a row holds inputs, not entries"),
                // Reactions belong to the form's own inputs (ADR 0041).
                Component::Input(input) => assert!(
                    input.reaction.is_none(),
                    "{here}: row input `{}` cannot react (`.reacts()` is for the form's own inputs)",
                    input.name
                ),
            }
        }
    }
    layouts(here, &spec.components);
}

/// The schema of a JSON repeater's rows: the inputs as declared (rows have no
/// fields), texts translated by `t`, every input disabled when `disabled`.
pub(crate) fn rows_schema(
    spec: &RepeaterSpec,
    t: &dyn Fn(&str) -> String,
    disabled: bool,
) -> crate::schema::RepeaterSchema {
    use crate::schema::{ComponentSchema, InputSchema, LayoutSchema, Rule};
    fn components(list: &[Component], t: &dyn Fn(&str) -> String, disabled: bool) -> Vec<ComponentSchema> {
        list.iter()
            .filter_map(|component| match component {
                Component::Layout(layout) => Some(ComponentSchema::Layout(LayoutSchema {
                    kind: layout.kind,
                    id: layout.id.clone(),
                    label: layout.label.as_deref().map(t),
                    columns: layout.columns,
                    components: components(&layout.children, t, disabled),
                    collapsed: layout.collapsed,
                    compact: layout.compact,
                    aside_first: layout.aside_first,
                    shown_when: layout.shown_when.as_deref().cloned(),
                })),
                Component::Entry(_) => None,
                Component::Input(input) => {
                    let mut rules = Vec::new();
                    if input.required {
                        rules.push(Rule::Required);
                    }
                    if input.kind == InputType::KeyValue {
                        rules.push(Rule::MaxItems { value: KEY_VALUE.0 });
                        rules.push(Rule::MaxLength { value: KEY_VALUE.1 });
                    } else if let Some(max) = input.max_length {
                        rules.push(Rule::MaxLength { value: max });
                    }
                    Some(ComponentSchema::Input(InputSchema {
                        kind: input.kind,
                        name: input.name.clone(),
                        label: t(&input.label.clone().unwrap_or_else(|| crate::ui::humanize(&input.name))),
                        value_type: row_kind(input).name(),
                        // Switches have two states in a row: off is a value.
                        nullable: !input.required && !matches!(input.kind, InputType::Toggle | InputType::Checkbox),
                        default: None,
                        disabled,
                        write_only: false,
                        rules,
                        column_span: input.column_span,
                        placeholder: input.placeholder.as_deref().map(t),
                        help: input.help.as_deref().map(t),
                        mode: input.mode,
                        options: input
                            .options
                            .iter()
                            .map(|o| crate::ui::SelectOption { value: o.value.clone(), label: t(&o.label) })
                            .collect(),
                        relationship: None,
                        widget: input.widget.clone(),
                        props: input.props.clone(),
                        media: None,
                        collection: None,
                        links: None,
                        prefix: input.prefix.clone(),
                        suffix: input.suffix.clone(),
                        number: input.number.as_deref().copied(),
                        shown_when: input.shown_when.as_deref().cloned(),
                        clear_when_hidden: false,
                        integer: input.integer,
                        repeater: input.repeater.as_deref().map(|nested| Box::new(rows_schema(nested, t, disabled))),
                        rich_nodes: Vec::new(),
                        reacts: false,
                    }))
                }
            })
            .collect()
    }
    options_schema(spec, components(&spec.components, t, disabled), t, None)
}

/// A repeater's options around its row `components`.
pub(crate) fn options_schema(
    spec: &RepeaterSpec,
    components: Vec<crate::schema::ComponentSchema>,
    t: &dyn Fn(&str) -> String,
    relationship: Option<crate::schema::RepeaterRelationship>,
) -> crate::schema::RepeaterSchema {
    crate::schema::RepeaterSchema {
        create_components: None,
        components,
        min_items: spec.min_items,
        max_items: spec.max_items,
        reorderable: spec.reorderable,
        collapsible: spec.collapsible,
        collapsed: spec.collapsed,
        item_label: spec.item_label.clone(),
        add_action_label: spec.add_action_label.as_deref().map(t),
        addable: spec.addable,
        deletable: spec.deletable,
        default_items: (spec.default_items > 0).then_some(spec.default_items),
        cloneable: spec.cloneable,
        relationship,
    }
}

/// The texts of a repeater's rows to translate (labels, help, placeholders,
/// option labels, the add button), recursively.
pub(crate) fn display_strings(spec: &RepeaterSpec, out: &mut Vec<String>) {
    fn walk(list: &[Component], out: &mut Vec<String>) {
        for component in list {
            match component {
                Component::Layout(layout) => {
                    out.extend(layout.label.clone());
                    walk(&layout.children, out);
                }
                Component::Input(input) => {
                    out.push(input.label.clone().unwrap_or_else(|| crate::ui::humanize(&input.name)));
                    out.extend(input.placeholder.clone());
                    out.extend(input.help.clone());
                    out.extend(input.options.iter().map(|o| o.label.clone()));
                    if let Some(nested) = input.repeater.as_deref() {
                        display_strings(nested, out);
                    }
                }
                Component::Entry(_) => {}
            }
        }
    }
    out.extend(spec.add_action_label.clone());
    walk(&spec.components, out);
}
