//! Forms of actions (ADR 0020): checked and put in canonical form before the
//! idempotency claim (so equivalent requests share it), references checked
//! in the action's transaction (only when it runs, never on a replay).

use super::*;
use crate::action::ActionField;

impl<R: Resource> ResourceAdapter<R> {
    /// The action `name`, if the user is admitted to run it at all (403
    /// before anything else is read; 404 for no such action).
    pub(super) async fn admitted_action(&self, ctx: &Ctx, name: &str) -> AppResult<&Action<Entity<R>>> {
        let action = self.actions.iter().find(|a| a.name == name).ok_or(AppError::NotFound)?;
        // Per-record policy checks are authoritative; an explicit admission
        // permission, when declared, refuses outright before them.
        if action.required_permission.is_some_and(|p| !ctx.can(p)) {
            return Err(AppError::Forbidden);
        }
        self.admit(ctx, action.ability).await?;
        Ok(action)
    }

    /// `data` checked against the action's form and in canonical form: every
    /// field present (default, else `null`), values re-encoded from their
    /// type (`"07"` → `7`…), keys of `belongs_to` fields canonical. Without
    /// a form, any data is refused.
    pub(super) fn action_data(&self, ctx: &Ctx, action: &Action<Entity<R>>, data: Data) -> AppResult<Data> {
        let mut errors = ValidationErrors::new();
        for name in data.keys() {
            if !action.form.iter().any(|f| f.name == *name) {
                errors.add(name.as_str(), "is not allowed");
            }
        }
        let value_of = |field: &ActionField| {
            data.get(&field.name).cloned().or_else(|| field.default.clone()).unwrap_or(serde_json::Value::Null)
        };
        // Conditions (ADR 0037): a hidden field reaches the handler as `null`,
        // whatever was sent, and is not required.
        let hidden = match self.action_conditions.get(action.name) {
            Some(conditions) => conditions.hidden(&action.form.iter().map(|f| (f.name.clone(), value_of(f))).collect()),
            None => Default::default(),
        };
        let mut out = Data::new();
        for field in &action.form {
            if hidden.contains(&field.name) {
                out.insert(field.name.clone(), serde_json::Value::Null);
                continue;
            }
            let Some(mut value) = canonical(field, &value_of(field), &mut errors) else { continue };
            if let (Some(target), serde_json::Value::String(key)) = (field.belongs_to, &value) {
                match ctx.registry.get(target).and_then(|t| t.canonical_key(key)) {
                    Some(key) => value = serde_json::Value::String(key),
                    None => {
                        errors.add(field.name.as_str(), "is not one of the options");
                        continue;
                    }
                }
            }
            out.insert(field.name.clone(), value);
        }
        errors.into_result()?;
        Ok(out)
    }

    /// Checks the `belongs_to` values of canonical `data` in the action's
    /// transaction: each a record the user may view.
    pub(super) async fn check_action_references(
        &self,
        ctx: &Ctx,
        txn: &DatabaseTransaction,
        action: &Action<Entity<R>>,
        data: &Data,
    ) -> AppResult<()> {
        let mut errors = ValidationErrors::new();
        for field in &action.form {
            let (Some(target), Some(serde_json::Value::String(key))) = (field.belongs_to, data.get(&field.name)) else {
                continue;
            };
            let resource =
                ctx.registry.get(target).ok_or_else(|| AppError::internal(format!("unknown resource `{target}`")))?;
            if !resource.keys_in_scope(ctx, txn, std::slice::from_ref(key)).await?.contains(key) {
                errors.add(field.name.as_str(), "is not one of the options");
            }
        }
        errors.into_result()?;
        Ok(())
    }

    /// Options of the `belongs_to` field `field` of action `name`.
    pub(super) async fn action_field_options(
        &self,
        ctx: &Ctx,
        name: &str,
        field: &str,
        search: Option<&str>,
        keys: &[String],
    ) -> AppResult<Vec<SelectOption>> {
        let action = self.admitted_action(ctx, name).await?;
        let field = action.form.iter().find(|f| f.name == field).ok_or(AppError::NotFound)?;
        let target = field.belongs_to.ok_or(AppError::NotFound)?;
        let resource =
            ctx.registry.get(target).ok_or_else(|| AppError::internal(format!("unknown resource `{target}`")))?;
        let found = resource.lookup(ctx, search, keys).await?;
        Ok(found
            .into_iter()
            .map(|(value, label)| SelectOption { value: serde_json::Value::String(value), label })
            .collect())
    }

    /// The form of an action for the schema.
    pub(super) fn action_form(&self, ctx: &Ctx, action: &Action<Entity<R>>) -> Option<FormSchema> {
        if action.form.is_empty() {
            return None;
        }
        let components = action
            .form
            .iter()
            .map(|field| {
                let mut rules = Vec::new();
                if field.required {
                    rules.push(Rule::Required);
                }
                if let Some(max) = field.max_length {
                    rules.push(Rule::MaxLength { value: max });
                }
                ComponentSchema::Input(InputSchema {
                    kind: field.input.kind,
                    name: field.name.clone(),
                    label: ctx.t_in(R::SLUG, &field.display_label()),
                    value_type: field.kind.name(),
                    nullable: !field.required,
                    default: field.default.clone(),
                    disabled: false,
                    write_only: false,
                    rules,
                    column_span: field.input.column_span,
                    placeholder: field.input.placeholder.as_deref().map(|p| ctx.t_in(R::SLUG, p)),
                    help: field.input.help.as_deref().map(|h| ctx.t_in(R::SLUG, h)),
                    mode: field.input.mode,
                    options: field
                        .input
                        .options
                        .iter()
                        .map(|o| SelectOption { value: o.value.clone(), label: ctx.t_in(R::SLUG, &o.label) })
                        .collect(),
                    relationship: field.belongs_to.map(|resource| Relationship { resource }),
                    widget: None,
                    props: None,
                    media: None,
                    collection: None,
                    links: None,
                    prefix: None,
                    suffix: None,
                    number: None,
                    shown_when: field.input.shown_when.as_deref().cloned(),
                    clear_when_hidden: false,
                    integer: false,
                    repeater: None,
                    rich_nodes: Vec::new(),
                    reacts: false,
                    toolbar: None,
                })
            })
            .collect();
        Some(FormSchema { columns: 1, components, width: Default::default() })
    }
}

/// The conditions of an action's form (ADR 0037), checked at boot.
pub(super) fn form_conditions(
    slug: &str,
    action: &str,
    form: &[ActionField],
) -> crate::engine::conditions::FormConditions {
    let components: Vec<crate::ui::Component> =
        form.iter().map(|f| crate::ui::Component::Input(f.input.clone())).collect();
    let kind_of = |name: &str| form.iter().find(|f| f.name == name).map(|f| (f.kind, None));
    crate::engine::conditions::FormConditions::new(
        &format!("resource `{slug}`, action `{action}`"),
        &components,
        &kind_of,
    )
}

/// Rewrites the conditions of an action's form with canonical values (sent
/// by schemas as the server compares them).
pub(super) fn canonicalize_conditions(form: &mut [ActionField]) {
    let kinds: Vec<(String, crate::wire::FieldKind)> = form.iter().map(|f| (f.name.clone(), f.kind)).collect();
    let kind_of = |name: &str| kinds.iter().find(|(n, _)| n == name).map(|(_, kind)| (*kind, None));
    for field in form {
        let mut components = [crate::ui::Component::Input(field.input.clone())];
        crate::engine::conditions::canonicalize(&mut components, &kind_of);
        let [crate::ui::Component::Input(input)] = components else { unreachable!() };
        field.input = input;
    }
}

/// `value` of `field` in canonical form, or `None` after adding its problem.
fn canonical(
    field: &ActionField,
    value: &serde_json::Value,
    errors: &mut ValidationErrors,
) -> Option<serde_json::Value> {
    let options = field.options();
    let rule = InputRule {
        kind: field.kind,
        nullable: !field.required,
        required: field.required,
        max_length: field.max_length,
        key_value: None,
        rich_text: false,
        rich_nodes: None,
        options: (!options.is_empty()).then_some(Options::One { values: &options, scale: None }),
        bounds: None,
    };
    // A key may be written as a number (`12`) or text (`"12"`).
    let value = match (field.belongs_to, value) {
        (Some(_), serde_json::Value::Number(n)) => serde_json::Value::String(n.to_string()),
        _ => value.clone(),
    };
    if !check_input(&field.name, &rule, &value, errors) {
        return None;
    }
    if value.is_null() {
        return Some(value);
    }
    if field.belongs_to.is_some() {
        // Made canonical by the target when checked; here, text without spaces.
        return Some(value);
    }
    let decoded = field.kind.decode(&value).ok()?;
    Some(decoded.first().map(encode).unwrap_or(value))
}

/// Boot checks of an action's form.
///
/// # Panics
/// On an invalid or repeated name, or a default that is not a valid value.
pub(super) fn check_action_form(slug: &str, action: &str, form: &[ActionField]) {
    let mut names = HashSet::new();
    for field in form {
        assert!(
            !field.name.is_empty()
                && field.name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
                && names.insert(field.name.as_str()),
            "resource `{slug}`: action `{action}`: form field `{}` is not a unique snake_case name",
            field.name
        );
        if let Some(default) = &field.default {
            let mut errors = ValidationErrors::new();
            assert!(
                field.belongs_to.is_none() && canonical(field, default, &mut errors).is_some(),
                "resource `{slug}`: action `{action}`: invalid default of `{}`",
                field.name
            );
        }
    }
}
