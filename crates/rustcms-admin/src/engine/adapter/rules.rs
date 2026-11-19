//! Rules of inputs computed on the server (ADR 0044, part E): boot checks,
//! their evaluation on a form's state, the `form/rules` route, the schema's
//! first answer, and the save's check (every path).

use std::collections::BTreeMap;
use std::time::Duration;

use serde_json::Value;

use super::write::key_text;
use super::*;
use crate::forms::RuleFn;
use crate::schema::{EvaluatedRules, FormRulesRequest, FormRulesResponse, ServerRulesSchema};
use crate::ui::{FormState, REACTION_MAX_TIMEOUT, SelectOption};

/// The rules of an input, checked at boot.
pub(super) struct InputRuleSet {
    pub(super) name: String,
    shown: Option<RuleFn<bool>>,
    required: Option<RuleFn<bool>>,
    options: Option<RuleFn<Vec<SelectOption>>>,
    pub(super) depends_on: Vec<String>,
    timeout: Duration,
}

impl InputRuleSet {
    /// Whether the input's visibility is computed.
    pub(super) fn shows(&self) -> bool {
        self.shown.is_some()
    }
}

/// The inputs of `form` with rules computed on the server, in form order;
/// panics on what cannot hold (ADR 0044 part E, boot checks). Rows refuse
/// them themselves (`check_spec`).
pub(super) fn input_rules(slug: &str, form: &Form, fields: &[Field]) -> Vec<InputRuleSet> {
    let inputs = form.inputs();
    let mut out = Vec::new();
    for input in &inputs {
        let Some(rules) = input.rules.as_deref() else { continue };
        let name = input.name.as_str();
        let here = format!("resource `{slug}`: input `{name}`");
        assert!(
            rules.any(),
            "{here}: `depends_on` and `rules_timeout` need `shown_when_fn`, `required_fn` or `options_fn`"
        );
        let depends_on = rules
            .depends_on
            .clone()
            .unwrap_or_else(|| panic!("{here}: rules computed on the server need `depends_on([...])`"));
        for dependency in &depends_on {
            assert!(
                inputs.iter().any(|i| i.name == *dependency),
                "{here}: `depends_on` names `{dependency}`, which is not an input of the form"
            );
        }
        assert!(
            !rules.timeout.is_zero() && rules.timeout <= REACTION_MAX_TIMEOUT,
            "{here}: the rules' timeout must be above zero and at most {REACTION_MAX_TIMEOUT:?}"
        );
        let relationship = input.repeater.as_deref().is_some_and(|s| s.relationship);
        let field = fields.iter().find(|f| f.name == name);
        assert!(
            !relationship
                && field.is_some_and(|f| f.secret.is_none())
                && !matches!(
                    input.kind,
                    InputType::Hidden | InputType::Secret | InputType::SecretMap | InputType::BelongsToMany
                ),
            "{here}: rules computed on the server are for inputs of fields (not hidden, encrypted, link or \
             relationship repeater inputs)"
        );
        assert!(
            rules.options.is_none()
                || (matches!(input.kind, InputType::Select | InputType::CheckboxList)
                    && field.is_some_and(|f| f.belongs_to.is_none())),
            "{here}: `options_fn` is for selects and checkbox lists that are not `belongs_to`"
        );
        out.push(InputRuleSet {
            name: name.to_owned(),
            shown: rules.shown.clone(),
            required: rules.required.clone(),
            options: rules.options.clone(),
            depends_on,
            timeout: rules.timeout,
        });
    }
    out
}

/// A form error: a rule could not be checked (`form_rules_failed`).
fn rules_failed() -> AppError {
    let mut errors = ValidationErrors::new();
    errors.add_form("could not check the form; try again");
    errors.into()
}

impl<R: Resource> ResourceAdapter<R> {
    /// Whether input `name` has a computed visibility (a condition of its own,
    /// for the save's input checks).
    pub(super) fn shown_by_rule(&self, name: &str) -> bool {
        self.input_rules.iter().any(|r| r.name == name && r.shows())
    }

    /// Panics when a declarative condition reads an input whose visibility is
    /// computed on the server (ADR 0044, part E): the panel evaluates
    /// conditions without waiting for the server.
    pub(super) fn check_rules_unread(&self) {
        for rule in self.input_rules.iter().filter(|r| r.shows()) {
            let name = rule.name.as_str();
            let read = self.conditions.reads(crate::ui::Scope::Row, name)
                || self.repeaters.values().any(|rows| rows.reads_root(name));
            assert!(
                !read,
                "resource `{}`: a condition reads `{name}`, whose visibility is computed on the server \
                 (`shown_when_fn`): conditions cannot read it",
                R::SLUG
            );
        }
    }

    /// The state rules read: what the user's form holds (the values conditions
    /// read, then `typed` over them, then the key a relation binds), as the
    /// declarative conditions leave it (hidden inputs `null`), fields the user
    /// may read only.
    pub(super) fn rules_state(&self, ctx: &Ctx, op: Operation, key: Option<String>, mut values: Data) -> FormState {
        if let Some((_, field, parent)) = &ctx.bound {
            values.insert(field.clone(), self.fk_value(field, parent));
        }
        let values = self.conditions.shown_values(&values, Default::default());
        FormState {
            operation: op,
            key,
            changed: String::new(),
            previous: None,
            values: values.into_iter().filter(|(n, _)| self.state_field(ctx, n)).collect(),
            relation_rows: Default::default(),
        }
    }

    /// Every input's rules on `state`, in form order, each input's closures
    /// within its time limit; why one failed otherwise (logged by the caller).
    pub(super) async fn eval_rules(
        &self,
        ctx: &Ctx,
        state: &FormState,
    ) -> Result<BTreeMap<String, EvaluatedRules>, String> {
        let mut out = BTreeMap::new();
        // One deadline for the whole form too: rules may run under a record's lock.
        let deadline = tokio::time::Instant::now() + REACTION_MAX_TIMEOUT;
        for rule in &self.input_rules {
            let run = async {
                let shown = match &rule.shown {
                    Some(f) => f(ctx, state).await?,
                    None => true,
                };
                let required = match &rule.required {
                    Some(f) => f(ctx, state).await?,
                    None => false,
                };
                let options = match &rule.options {
                    Some(f) => Some(f(ctx, state).await?),
                    None => None,
                };
                Ok::<_, AppError>((shown, required, options))
            };
            let until = (tokio::time::Instant::now() + rule.timeout).min(deadline);
            let (shown, required, options) = match tokio::time::timeout_at(until, run).await {
                Ok(Ok(answer)) => answer,
                Ok(Err(err)) => return Err(format!("`{}`: {err}", rule.name)),
                Err(_) => return Err(format!("`{}`: took longer than {:?}", rule.name, rule.timeout)),
            };
            let options = options.map(|list| {
                list.into_iter()
                    .map(|o| SelectOption {
                        value: if o.value.is_null() { o.value } else { self.canonical_value(&rule.name, &o.value) },
                        label: ctx.t_in(R::SLUG, &o.label),
                    })
                    .collect()
            });
            out.insert(rule.name.clone(), EvaluatedRules { shown, required, options });
        }
        Ok(out)
    }

    /// The schema's first answer (ADR 0044, part E): each input with rules
    /// carries them, computed for the form's values (create: the defaults;
    /// edit: the record as the user's form holds it). A failure counts as
    /// shown, not required, with the declared options (logged).
    pub(super) async fn fill_rules(
        &self,
        ctx: &Ctx,
        op: SchemaOperation,
        stored: Option<&Data>,
        components: &mut [ComponentSchema],
    ) {
        if self.input_rules.is_empty() || op == SchemaOperation::View {
            return;
        }
        let (operation, stored) = match (op, stored) {
            (SchemaOperation::Edit, Some(record)) => {
                let viewable = self.policy.allows(ctx, ability::VIEW, Some(record)).await;
                (Operation::Update, Some((record, viewable)))
            }
            _ => (Operation::Create, None),
        };
        let key = stored.and_then(|(record, _)| record.get(&self.pk_name)).and_then(key_text);
        let values = self.condition_values(ctx, operation, stored);
        let state = self.rules_state(ctx, operation, key, values);
        // The reactions' turns: a busy user or panel gets the declared rules, not a wait.
        let (Some(_user), Some(_turn)) = (super::react::UserTurn::take(&ctx.principal.id), super::react::try_turn())
        else {
            self.fill_defaults(components);
            return;
        };
        let answers = match self.eval_rules(ctx, &state).await {
            Ok(answers) => answers,
            Err(failure) => {
                tracing::warn!(resource = R::SLUG, failure, "form rules failed");
                BTreeMap::new()
            }
        };
        fn walk(list: &mut [ComponentSchema], fill: &dyn Fn(&mut InputSchema)) {
            for component in list {
                match component {
                    ComponentSchema::Input(input) => fill(input),
                    ComponentSchema::Layout(layout) => walk(&mut layout.components, fill),
                    _ => {}
                }
            }
        }
        let fill = |input: &mut InputSchema| {
            let Some(rule) = self.input_rules.iter().find(|r| r.name == input.name) else { return };
            let answer = answers.get(&rule.name);
            input.server_rules = Some(ServerRulesSchema {
                depends_on: rule.depends_on.clone(),
                shown: rule.shown.as_ref().map(|_| answer.is_none_or(|a| a.shown)),
                required: rule.required.as_ref().map(|_| answer.is_some_and(|a| a.required)),
            });
            if let Some(options) = answer.and_then(|a| a.options.clone()) {
                input.options = options;
            }
        };
        walk(components, &fill);
    }

    /// `POST form/rules`: the inputs' rules for the form's state, authorized as
    /// `form/react` (`create`, or `edit` of a record the user may update).
    pub(super) async fn form_rules(&self, ctx: &Ctx, request: FormRulesRequest) -> AppResult<FormRulesResponse> {
        let (op, stored, viewable) = match (request.operation, request.key.as_deref()) {
            (SchemaOperation::Create, None) => {
                if !self.policy.allows(ctx, ability::CREATE, None).await {
                    return Err(AppError::Forbidden);
                }
                (Operation::Create, None, false)
            }
            (SchemaOperation::Edit, Some(key)) if ctx.bound.is_none() => {
                self.admit(ctx, ability::UPDATE).await?;
                let model = self.find_model(ctx, &ctx.db, key).await?;
                let record = self.record(&model);
                if !self.policy.allows(ctx, ability::UPDATE, Some(&record)).await {
                    return Err(AppError::Forbidden);
                }
                let viewable = self.policy.allows(ctx, ability::VIEW, Some(&record)).await;
                (Operation::Update, Some(record), viewable)
            }
            _ => return Err(ValidationErrors::field("operation", "is invalid").into()),
        };
        let mut values = self.condition_values(ctx, op, stored.as_ref().map(|r| (r, viewable)));
        let bound = ctx.bound.as_ref().map(|(_, field, _)| field.clone());
        for (name, value) in request.state {
            if bound.as_deref() == Some(name.as_str()) || !self.typed(ctx, op, &name, &value)? {
                continue;
            }
            values.insert(name, value);
        }
        let state = self.rules_state(ctx, op, request.key.clone(), values);
        let _user =
            super::react::UserTurn::take(&ctx.principal.id).ok_or(AppError::TooManyRequests { retry_after: 1 })?;
        let _turn = super::react::turn().await?;
        match self.eval_rules(ctx, &state).await {
            Ok(rules) => Ok(FormRulesResponse { seq: request.seq, rules, failed: false }),
            Err(failure) => {
                tracing::warn!(resource = R::SLUG, failure, "form rules failed");
                Ok(FormRulesResponse { seq: request.seq, rules: BTreeMap::new(), failed: true })
            }
        }
    }

    /// The rules' schemas without an answer: shown, not required, declared options.
    fn fill_defaults(&self, components: &mut [ComponentSchema]) {
        for component in components {
            match component {
                ComponentSchema::Input(input) => {
                    if let Some(rule) = self.input_rules.iter().find(|r| r.name == input.name) {
                        input.server_rules = Some(ServerRulesSchema {
                            depends_on: rule.depends_on.clone(),
                            shown: rule.shown.as_ref().map(|_| true),
                            required: rule.required.as_ref().map(|_| false),
                        });
                    }
                }
                ComponentSchema::Layout(layout) => self.fill_defaults(&mut layout.components),
                _ => {}
            }
        }
    }

    /// The save's check (every path, ADR 0044 part E), after the declarative
    /// conditions (`hidden`: the inputs they hide, already dropped): the rules
    /// run on the state those conditions read (`stored`, the record as the
    /// user's form holds it on update, with `input` over it). A hidden input's
    /// value is dropped (but the key a relation binds). A shown one that is
    /// required (`required_fn`, or, under `shown_when_fn`, the field's own rule
    /// or a column that is not nullable) and holds no value is `is required`,
    /// for a user who may read and write it, when the save could have changed
    /// that (on create, or when the input or an input it depends on is sent: an
    /// old record is not refused for an input the save leaves alone). A value
    /// outside the computed options is `is not one of the options`. A rule
    /// that fails refuses the save (`form_rules_failed`).
    pub(super) async fn apply_rules(
        &self,
        ctx: &Ctx,
        op: Operation,
        input: &mut Data,
        stored: Option<(&Data, bool)>,
        hidden: &BTreeSet<String>,
    ) -> AppResult<()> {
        if self.input_rules.is_empty() {
            return Ok(());
        }
        let mut values = self.condition_values(ctx, op, stored);
        values.extend(input.iter().map(|(k, v)| (k.clone(), v.clone())));
        let key = stored.and_then(|(record, _)| record.get(&self.pk_name)).and_then(key_text);
        let state = self.rules_state(ctx, op, key, values.clone());
        let answers = self.eval_rules(ctx, &state).await.map_err(|failure| {
            tracing::warn!(resource = R::SLUG, failure, "form rules failed on save");
            rules_failed()
        })?;
        let bound = ctx.bound.as_ref().map(|(_, field, _)| field.as_str());
        let sent = input.keys().cloned().collect::<BTreeSet<String>>();
        let mut errors = ValidationErrors::new();
        for rule in &self.input_rules {
            let name = rule.name.as_str();
            // Hidden by a `when(..)` condition: not shown, whatever the rule says.
            let Some(answer) = answers.get(name).filter(|_| !hidden.contains(name)) else { continue };
            if !answer.shown {
                if bound != Some(name) {
                    input.remove(name);
                }
                continue;
            }
            let Some(field) = self.field(name) else { continue };
            let needed = answer.required || (rule.shows() && (field.required || !self.column(name).nullable));
            let changed =
                op == Operation::Create || sent.contains(name) || rule.depends_on.iter().any(|d| sent.contains(d));
            let editable = field.can_read(ctx) && field.can_write(op, ctx);
            let empty = input.get(name).or_else(|| values.get(name)).is_none_or(|v| crate::forms::is_empty(Some(v)));
            if needed && changed && editable && empty {
                errors.add(name, "is required");
                continue;
            }
            if let (Some(options), Some(sent)) = (&answer.options, input.get(name).filter(|v| !v.is_null())) {
                let allowed: Vec<&Value> = options.iter().map(|o| &o.value).collect();
                let canonical = |v: &Value| self.canonical_value(name, v);
                let fits = match sent {
                    Value::Array(items) => items.iter().all(|v| allowed.contains(&&canonical(v))),
                    one => allowed.contains(&&canonical(one)),
                };
                if !fits {
                    errors.add(name, "is not one of the options");
                }
            }
        }
        errors.into_result()?;
        Ok(())
    }
}
