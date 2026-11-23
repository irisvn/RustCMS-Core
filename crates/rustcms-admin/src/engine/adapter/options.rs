//! Options computed on the server (`options_fn`, ADR 0044 part E): boot
//! checks, the `form/options` route, and the save's check.
//!
//! The function reads the form's state as reactions do (ADR 0043's rule:
//! never a value the user may not read, nor a record they may not view), the
//! declarative conditions applied. It never runs inside the write's
//! transaction: a save runs it before `begin` (the preflight), then compares,
//! under the record's lock, what it read (the dependencies the user may read,
//! and which inputs were checked) and checks the value against its answer.

use std::time::Duration;

use serde_json::Value;

use super::react::{REACTING, REACTING_WAIT, UserTurn};
use super::*;
use crate::forms::{OPTIONS_MAX_TIMEOUT, OptionsFn, is_empty};
use crate::schema::{FormOptionsRequest, FormOptionsResponse};
use crate::ui::FormState;

/// Options one answer may hold.
pub(crate) const MAX_OPTIONS: usize = 500;
/// Bytes of one answer, as JSON.
pub(crate) const MAX_OPTIONS_BYTES: usize = 64 * 1024;
/// The form error of a save whose options could not be computed (`options_failed`).
pub(crate) const OPTIONS_FAILED: &str = "could not check the options; try again";

/// An input whose options are computed, checked at boot.
pub(super) struct OptionsInput {
    pub(super) name: String,
    run: OptionsFn,
    pub(super) depends_on: Vec<String>,
    timeout: Duration,
    /// A checkbox list (else a select).
    many: bool,
}

/// The inputs of `form` with `options_fn`, in form order; panics on what
/// cannot hold (ADR 0044 part E, boot checks). Rows, settings pages and
/// action forms refuse `options_fn` themselves.
pub(super) fn options_inputs<C: ColumnTrait>(
    slug: &str,
    form: &Form,
    fields: &[Field],
    columns: &HashMap<String, ColumnInfo<C>>,
) -> Vec<OptionsInput> {
    let inputs = form.inputs();
    let mut out = Vec::new();
    for input in &inputs {
        let Some(source) = input.options_fn.as_deref() else { continue };
        let name = input.name.as_str();
        let here = format!("resource `{slug}`: input `{name}`");
        let run = source
            .run
            .clone()
            .unwrap_or_else(|| panic!("{here}: `depends_on` and `options_timeout` need `options_fn`"));
        let depends_on =
            source.depends_on.clone().unwrap_or_else(|| panic!("{here}: `options_fn` needs `depends_on([...])`"));
        assert!(
            !source.timeout.is_zero() && source.timeout <= OPTIONS_MAX_TIMEOUT,
            "{here}: the options timeout must be above zero and at most {OPTIONS_MAX_TIMEOUT:?}"
        );
        let field = fields.iter().find(|f| f.name == name);
        assert!(
            matches!(input.kind, InputType::Select | InputType::CheckboxList)
                && field.is_some_and(|f| f.belongs_to.is_none() && f.secret.is_none()),
            "{here}: `options_fn` is for selects and checkbox lists of fields that are not `belongs_to` or encrypted"
        );
        assert!(
            inputs.iter().filter(|i| i.name == name).count() == 1,
            "{here}: a field with `options_fn` has one input (field `{name}` appears in several)"
        );
        for dependency in &depends_on {
            let found = inputs.iter().find(|i| i.name == *dependency);
            let scalar_or_list = found.is_some_and(|i| {
                let field = fields.iter().find(|f| f.name == *dependency);
                let column = columns.get(dependency.as_str());
                field.is_some_and(|f| f.secret.is_none())
                    && column.is_some_and(|c| c.kind != FieldKind::Json || i.kind == InputType::CheckboxList)
                    && !matches!(
                        i.kind,
                        InputType::Hidden
                            | InputType::Secret
                            | InputType::SecretMap
                            | InputType::BelongsToMany
                            | InputType::Repeater
                    )
            });
            assert!(
                scalar_or_list,
                "{here}: `depends_on` names `{dependency}`, which is not an input of the form with a scalar or \
                 list value (not hidden, encrypted, a link, a repeater or a JSON value)"
            );
        }
        out.push(OptionsInput {
            name: name.to_owned(),
            run,
            depends_on,
            timeout: source.timeout,
            many: input.kind == InputType::CheckboxList,
        });
    }
    out
}

/// The form error of a save whose options could not be computed.
fn options_failed() -> AppError {
    let mut errors = ValidationErrors::new();
    errors.add_form(OPTIONS_FAILED);
    errors.into()
}

/// What an `options_fn` read changed between the preflight and the lock.
fn stale() -> AppError {
    AppError::Conflict {
        code: "stale",
        reason: "options",
        message: "This was changed by someone else meanwhile. Reload and try again.",
    }
}

/// The form's state the options read: authorized as a reaction's (stored
/// values of readable fields of a record the user may view, typed values of
/// fields they may write, the key a relation binds and its fixed values over
/// what was sent), then normalized by the declarative conditions (hidden
/// inputs `null`); fields the user may read only.
pub(super) struct OptionsState {
    state: FormState,
    /// Inputs the conditions hide.
    hidden: BTreeSet<String>,
    /// Fields the relation sets (its key, its filters).
    set_by_relation: Vec<String>,
}

/// What a save's preflight read and answered (ADR 0044, part E).
pub(super) struct OptionsCheck {
    /// The request as sent (what both states are built from).
    sent: Data,
    /// The inputs the save could change (create, the input or a dependency sent).
    triggered: Vec<usize>,
    /// The triggered inputs the user may access, each with the dependencies
    /// it read and its answer's values.
    evaluated: Vec<Evaluated>,
}

struct Evaluated {
    index: usize,
    projection: Vec<Option<Value>>,
    options: Vec<Value>,
}

impl<R: Resource> ResourceAdapter<R> {
    /// Whether field `name` has `options_fn` (its declared options are then
    /// not checked: the answer is).
    pub(super) fn computes_options(&self, name: &str) -> bool {
        self.options_inputs.iter().any(|o| o.name == name)
    }

    /// Whether a checkbox list with computed options holds a list without
    /// repeats (`None`: it does, or it is not one). Checked whatever the
    /// answer, before membership.
    pub(super) fn computed_list_problem(&self, name: &str, value: &Value) -> Option<String> {
        if value.is_null() || !self.options_inputs.iter().any(|o| o.name == name && o.many) {
            return None;
        }
        crate::engine::validate::list_shape_problem(value)
    }

    /// The state options read (see [`OptionsState`]). `strict`: a JSON
    /// repeater sent with more rows than it may hold is a 422 (the route);
    /// otherwise its value is left out (a save refuses it anyway).
    fn options_state(
        &self,
        ctx: &Ctx,
        op: Operation,
        key: Option<String>,
        stored: Option<(&Data, bool)>,
        sent: &Data,
        strict: bool,
    ) -> AppResult<OptionsState> {
        let mut values = self.condition_values(ctx, op, stored);
        let bound = ctx.bound.as_ref().map(|(_, field, parent)| (field.clone(), self.fk_value(field, parent)));
        let fixed = self.filter_values(&super::operations::bound_filters(ctx))?;
        let mut set_by_relation: Vec<String> = fixed.iter().map(|(f, _)| (*f).to_owned()).collect();
        set_by_relation.extend(bound.iter().map(|(f, _)| f.clone()));
        for (name, value) in sent {
            if set_by_relation.contains(name) {
                continue;
            }
            let typed = match self.typed(ctx, op, name, value) {
                Ok(typed) => typed,
                Err(err) if strict => return Err(err),
                Err(_) => false,
            };
            if typed {
                values.insert(name.clone(), value.clone());
            }
        }
        if let Some((field, value)) = bound {
            values.insert(field, value);
        }
        values.extend(fixed.into_iter().map(|(f, v)| (f.to_owned(), v)));
        let hidden = self.conditions.hidden(&values);
        let shown = self.conditions.shown_values(&values, Default::default());
        let state = FormState {
            operation: op,
            key,
            changed: String::new(),
            previous: None,
            values: shown.into_iter().filter(|(n, _)| self.state_field(ctx, n)).collect(),
        };
        Ok(OptionsState { state, hidden, set_by_relation })
    }

    /// Whether the user's form holds input `name` with options to compute:
    /// writable for `op`, shown on the state, not set by the relation.
    fn options_shown(&self, ctx: &Ctx, op: Operation, built: &OptionsState, name: &str) -> bool {
        self.writable(ctx, op, name) && !built.hidden.contains(name) && !built.set_by_relation.iter().any(|f| f == name)
    }

    /// `values` read by `input` (its dependencies, canonical; `None`: not in
    /// the state, i.e. not readable).
    fn projection(&self, built: &OptionsState, input: &OptionsInput) -> Vec<Option<Value>> {
        input
            .depends_on
            .iter()
            .map(|d| {
                built.state.values.get(d).map(|v| if v.is_null() { Value::Null } else { self.canonical_value(d, v) })
            })
            .collect()
    }

    /// Runs `input`'s function on `state` within its timeout and `deadline`,
    /// its answer checked; why it failed otherwise (logged by the caller).
    async fn run_options(
        &self,
        ctx: &Ctx,
        input: &OptionsInput,
        state: &FormState,
        deadline: tokio::time::Instant,
    ) -> Result<Vec<SelectOption>, String> {
        let mut state = state.clone();
        state.changed = input.name.clone();
        let until = (tokio::time::Instant::now() + input.timeout).min(deadline);
        let late = || format!("took longer than {:?}", input.timeout);
        let list = match tokio::time::timeout_at(until, (input.run)(ctx, &state)).await {
            Ok(Ok(list)) => list,
            Ok(Err(err)) => return Err(err.to_string()),
            Err(_) => return Err(late()),
        };
        let checked = self.checked_options(ctx, input, list)?;
        // The checks count too: an answer is never late and successful.
        if tokio::time::Instant::now() >= until {
            return Err(late());
        }
        Ok(checked)
    }

    /// An answer as the panel and the save use it: at most [`MAX_OPTIONS`]
    /// options and [`MAX_OPTIONS_BYTES`]; a select's values decoded to the
    /// column's kind and canonical (stored as they are), a checkbox list's
    /// not `null`; no value twice; labels translated.
    fn checked_options(
        &self,
        ctx: &Ctx,
        input: &OptionsInput,
        list: Vec<SelectOption>,
    ) -> Result<Vec<SelectOption>, String> {
        if list.len() > MAX_OPTIONS {
            return Err(format!("more than {MAX_OPTIONS} options"));
        }
        let column = self.column(&input.name);
        let column_type = column.column.def().get_column_type().clone();
        let scale = crate::wire::decimal_scale(&column_type);
        let mut seen = HashSet::new();
        let mut out = Vec::with_capacity(list.len());
        for option in list {
            if option.value.is_null() {
                return Err("an option without a value".to_owned());
            }
            let value = if input.many {
                option.value
            } else {
                // An `i64` or decimal written as a JSON integer reads as the wire's string.
                let value = match (column.kind, &option.value) {
                    (FieldKind::I64 | FieldKind::Decimal, Value::Number(n)) if n.is_i64() || n.is_u64() => {
                        Value::String(n.to_string())
                    }
                    _ => option.value,
                };
                let canonical = crate::wire::canonical(column.kind, scale, &value)
                    .map_err(|e| format!("option {value} is not a value of the column: {}", e.0))?;
                super::input::storable(&column_type, column.kind, &canonical)
                    .map_err(|problem| format!("option {value} {problem}"))?;
                canonical
            };
            if !seen.insert(value.to_string()) {
                return Err(format!("option {value} is given twice"));
            }
            out.push(SelectOption { value, label: ctx.t_in(R::SLUG, &option.label) });
        }
        let bytes = serde_json::to_vec(&out).map_or(usize::MAX, |b| b.len());
        if bytes > MAX_OPTIONS_BYTES {
            return Err(format!("more than {} KiB of options", MAX_OPTIONS_BYTES / 1024));
        }
        Ok(out)
    }

    /// `POST form/options`: the options of `request.input` for the form's
    /// state, authorized as `form/react` (`create`, or `edit` of a record in
    /// scope the user may update; `ctx.bound` through a relation). An input
    /// the user's form does not hold with computed options (not readable or
    /// writable, hidden by its conditions, unknown) is a 404.
    pub(super) async fn form_options(&self, ctx: &Ctx, request: FormOptionsRequest) -> AppResult<FormOptionsResponse> {
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
        let stored = stored.as_ref().map(|r| (r, viewable));
        let built = self.options_state(ctx, op, request.key.clone(), stored, &request.state, true)?;
        let input = self
            .options_inputs
            .iter()
            .find(|o| {
                o.name == request.input
                    && self.state_field(ctx, &o.name)
                    && self.options_shown(ctx, op, &built, &o.name)
            })
            .ok_or(AppError::NotFound)?;
        // Bounded turns, after authorization and before running anything (the
        // route is also limited per user, with reactions: see `http::options`).
        let _user = UserTurn::take(&ctx.principal.id).ok_or(AppError::TooManyRequests { retry_after: 1 })?;
        let _turn = tokio::time::timeout(REACTING_WAIT, REACTING.acquire())
            .await
            .map_err(|_| AppError::TooManyRequests { retry_after: 5 })?
            .map_err(AppError::internal)?;
        let deadline = tokio::time::Instant::now() + input.timeout;
        match self.run_options(ctx, input, &built.state, deadline).await {
            Ok(options) => Ok(FormOptionsResponse { options: Some(options), failed: false }),
            Err(failure) => {
                tracing::warn!(resource = R::SLUG, input = input.name, failure, "form options failed");
                Ok(FormOptionsResponse { options: None, failed: true })
            }
        }
    }

    /// The inputs of `built` a save checks among `triggered`: shown and
    /// writable, with a value to check (sent; on update, the stored one of a
    /// field the user may read in a record they may view). Never decided by
    /// a value the user cannot observe.
    fn options_checked(
        &self,
        ctx: &Ctx,
        op: Operation,
        built: &OptionsState,
        triggered: &[usize],
        sent: &Data,
        viewable: bool,
    ) -> Vec<usize> {
        triggered
            .iter()
            .copied()
            .filter(|&index| {
                let name = self.options_inputs[index].name.as_str();
                let readable_stored =
                    op == Operation::Update && viewable && self.field(name).is_some_and(|f| f.can_read(ctx));
                self.options_shown(ctx, op, built, name) && (sent.contains_key(name) || readable_stored)
            })
            .collect()
    }

    /// The save's preflight (ADR 0044, part E), before `begin`: for the
    /// inputs the save could change (on create, or when the input or one of
    /// its dependencies is sent), the state built as the route builds it
    /// (on update, from the record read without a lock), and each checked
    /// input's function run. One deadline, the longest timeout of those
    /// inputs, covers waiting for a turn, every function and the answers'
    /// checks; a failure is a form error (`options_failed`), nothing saved.
    /// `None`: nothing to check.
    pub(super) async fn options_preflight(
        &self,
        ctx: &Ctx,
        op: Operation,
        key: Option<&str>,
        sent: &Data,
    ) -> AppResult<Option<OptionsCheck>> {
        if self.options_inputs.is_empty() {
            return Ok(None);
        }
        let triggered: Vec<usize> = self
            .options_inputs
            .iter()
            .enumerate()
            .filter(|(_, o)| {
                op == Operation::Create
                    || sent.contains_key(&o.name)
                    || o.depends_on.iter().any(|d| sent.contains_key(d))
            })
            .map(|(index, _)| index)
            .collect();
        if triggered.is_empty() {
            return Ok(None);
        }
        let (stored, viewable) = match key {
            Some(key) => {
                let model = self.find_model(ctx, &ctx.db, key).await?;
                let record = self.record(&model);
                if !self.policy.allows(ctx, ability::UPDATE, Some(&record)).await {
                    return Err(AppError::Forbidden);
                }
                let viewable = self.policy.allows(ctx, ability::VIEW, Some(&record)).await;
                (Some(record), viewable)
            }
            None => (None, false),
        };
        let built =
            self.options_state(ctx, op, key.map(str::to_owned), stored.as_ref().map(|r| (r, viewable)), sent, false)?;
        let checked = self.options_checked(ctx, op, &built, &triggered, sent, viewable);
        let mut evaluated = Vec::with_capacity(checked.len());
        if !checked.is_empty() {
            let longest = checked.iter().map(|&i| self.options_inputs[i].timeout).max().unwrap_or_default();
            let deadline = tokio::time::Instant::now() + longest.min(OPTIONS_MAX_TIMEOUT);
            // Saves wait for a turn up to the deadline (no per-user refusal).
            let _turn = match tokio::time::timeout_at(deadline, REACTING.acquire()).await {
                Ok(Ok(turn)) => turn,
                _ => {
                    tracing::warn!(resource = R::SLUG, "form options: no turn before the deadline on save");
                    return Err(options_failed());
                }
            };
            for index in checked {
                let input = &self.options_inputs[index];
                let options = self.run_options(ctx, input, &built.state, deadline).await.map_err(|failure| {
                    tracing::warn!(resource = R::SLUG, input = input.name, failure, "form options failed on save");
                    options_failed()
                })?;
                evaluated.push(Evaluated {
                    index,
                    projection: self.projection(&built, input),
                    options: options.into_iter().map(|o| o.value).collect(),
                });
            }
        }
        Ok(Some(OptionsCheck { sent: sent.clone(), triggered, evaluated }))
    }

    /// The save's check under the lock (ADR 0044, part E): the state rebuilt
    /// with the write's context (`wctx`, the principal `begin` refreshed) and
    /// the locked record (`stored`); the inputs checked and the dependencies
    /// they read (values the user may read only) must be those of the
    /// preflight, else 409 `stale`. Then each checked input's effective value
    /// (`input`: accepted, conditions applied; else the stored one) must be
    /// among its answer (`is not one of the options`); an empty one is not
    /// checked.
    pub(super) fn options_locked(
        &self,
        wctx: &Ctx,
        op: Operation,
        check: Option<OptionsCheck>,
        key: Option<&str>,
        stored: Option<(&Data, bool)>,
        input: &Data,
    ) -> AppResult<()> {
        let Some(check) = check else { return Ok(()) };
        let viewable = stored.is_some_and(|(_, viewable)| viewable);
        let built = self.options_state(wctx, op, key.map(str::to_owned), stored, &check.sent, false)?;
        let checked = self.options_checked(wctx, op, &built, &check.triggered, &check.sent, viewable);
        let same = checked.len() == check.evaluated.len()
            && checked.iter().zip(&check.evaluated).all(|(&index, evaluated)| {
                index == evaluated.index && self.projection(&built, &self.options_inputs[index]) == evaluated.projection
            });
        if !same {
            return Err(stale());
        }
        let mut errors = ValidationErrors::new();
        for evaluated in &check.evaluated {
            let options = &self.options_inputs[evaluated.index];
            let name = options.name.as_str();
            let value = if check.sent.contains_key(name) {
                input.get(name)
            } else {
                stored.filter(|(_, viewable)| *viewable).and_then(|(record, _)| record.get(name))
            };
            let Some(value) = value.filter(|v| !is_empty(Some(v))) else { continue };
            let scale = crate::wire::decimal_scale(self.column(name).column.def().get_column_type());
            let rule = InputRule {
                options: Some(if options.many {
                    Options::Many(&evaluated.options)
                } else {
                    Options::One { values: &evaluated.options, scale }
                }),
                ..self.rule(name, false)
            };
            if let Some(problem) = crate::engine::validate::option_problem(&rule, value) {
                errors.add(name, problem);
            }
        }
        errors.into_result()
    }
}
