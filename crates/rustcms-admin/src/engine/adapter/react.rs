//! Form reactions (ADR 0043): boot checks and the `form/react` route.
//!
//! A reaction reads what the user's form holds (ADR 0037's rule: never a value
//! the user may not read, nor a record they may not view), and its patches are
//! kept only when the user could make them. Reactions never run on save.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use serde_json::Value;

use super::*;
use crate::forms::{ReactionFn, is_empty};
use crate::schema::{FormReactRequest, FormReactResponse};
use crate::ui::{FormPatch, FormState, REACTION_MAX_TIMEOUT};

/// Reactions running at once, across users.
pub(super) static REACTING: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(8);
/// Reactions running at once for one user (the panel needs only the latest).
const REACTING_PER_USER: usize = 2;
static PER_USER: Mutex<Option<HashMap<String, usize>>> = Mutex::new(None);
/// Longest wait for a turn before answering 429 (the user is typing).
pub(super) const REACTING_WAIT: Duration = Duration::from_secs(5);
/// Patches one answer may hold; the rest are dropped (and logged).
pub(crate) const MAX_PATCHES: usize = 200;
/// Characters of a notice or a row's warning.
pub(crate) const MAX_MESSAGE: usize = 500;
/// Rows of a JSON repeater a request may send when the repeater declares no
/// `max_items`.
pub(crate) const MAX_STATE_ROWS: usize = 1000;

/// The reaction of an input, checked at boot.
pub(super) struct InputReaction {
    pub(super) name: String,
    run: ReactionFn,
    timeout: Duration,
}

/// The reactions of `form`, in form order; panics on what cannot hold (ADR
/// 0043, boot checks). Rows refuse reactions themselves (`check_spec`).
pub(super) fn reactions(slug: &str, form: &Form, fields: &[Field]) -> Vec<InputReaction> {
    let mut out = Vec::new();
    for input in form.inputs() {
        let Some(reaction) = input.reaction.as_deref() else { continue };
        let name = input.name.as_str();
        let here = format!("resource `{slug}`: input `{name}`");
        let run = reaction.run.clone().unwrap_or_else(|| panic!("{here}: `.reaction_timeout()` needs `.reacts()`"));
        assert!(
            !reaction.timeout.is_zero() && reaction.timeout <= REACTION_MAX_TIMEOUT,
            "{here}: the reaction timeout must be above zero and at most {REACTION_MAX_TIMEOUT:?}"
        );
        let relationship = input.repeater.as_deref().is_some_and(|s| s.relationship);
        assert!(
            fields.iter().any(|f| f.name == name && f.secret.is_none())
                && !relationship
                && !matches!(
                    input.kind,
                    InputType::Hidden | InputType::Secret | InputType::SecretMap | InputType::BelongsToMany
                ),
            "{here}: only inputs of fields react (not hidden, encrypted, link or relationship repeater inputs)"
        );
        out.push(InputReaction { name: name.to_owned(), run, timeout: reaction.timeout });
    }
    out
}

/// A turn of one user, given back when dropped.
pub(super) struct UserTurn(String);

impl UserTurn {
    pub(super) fn take(user: &str) -> Option<Self> {
        let mut map = PER_USER.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let count = map.get_or_insert_with(HashMap::new).entry(user.to_owned()).or_default();
        if *count >= REACTING_PER_USER {
            return None;
        }
        *count += 1;
        Some(Self(user.to_owned()))
    }
}

impl Drop for UserTurn {
    fn drop(&mut self) {
        let mut map = PER_USER.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(map) = map.as_mut()
            && let Some(count) = map.get_mut(&self.0)
        {
            *count -= 1;
            if *count == 0 {
                map.remove(&self.0);
            }
        }
    }
}

impl<R: Resource> ResourceAdapter<R> {
    /// `POST form/react`: the changes the reaction of `request.changed` asks
    /// for, kept when the user could make them. Authorized as the form's
    /// write: `create` without a record (through a relation: `ctx.bound`),
    /// `edit` on a record in scope the user may update. Whether the user may
    /// also view that record decides whether its stored values are read.
    pub(super) async fn react(&self, ctx: &Ctx, request: FormReactRequest) -> AppResult<FormReactResponse> {
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
        let reaction = self
            .reactions
            .iter()
            .find(|r| r.name == request.changed && self.writable(ctx, op, &r.name))
            .ok_or_else(|| AppError::from(ValidationErrors::field("changed", "is invalid")))?;
        let empty = |failed| FormReactResponse { seq: request.seq, patches: Vec::new(), failed };
        let bound = ctx.bound.as_ref().map(|(_, field, parent)| (field.clone(), self.fk_value(field, parent)));
        // What the form holds: the values conditions read (ADR 0037: stored
        // values only for a record the user may view, never an unreadable
        // field's), then what the user typed in inputs they may write (the
        // form may be half typed: values that do not decode are left out),
        // then the key a relation binds.
        let mut values = self.condition_values(ctx, op, stored.as_ref().map(|r| (r, viewable)));
        // Through a filtered relation, its values as the save sets them (ADR 0044).
        let fixed = self.filter_values(&super::operations::bound_filters(ctx))?;
        let mut previous = None;
        for (name, value) in request.state {
            if bound.as_ref().is_some_and(|(field, _)| *field == name)
                || fixed.iter().any(|(field, _)| *field == name)
                || !self.typed(ctx, op, &name, &value)?
            {
                continue;
            }
            if name == reaction.name {
                previous = request.previous.clone().filter(|p| self.typed(ctx, op, &name, p).unwrap_or(false));
            }
            values.insert(name, value);
        }
        if let Some((field, value)) = bound {
            values.insert(field, value);
        }
        values.extend(fixed.into_iter().map(|(f, v)| (f.to_owned(), v)));
        if self.conditions.hidden(&values).contains(&reaction.name) {
            return Ok(empty(false));
        }
        // Bounded turns, after authorization and before running anything (the
        // route is also limited per user, see `http::react`).
        let _user = UserTurn::take(&ctx.principal.id).ok_or(AppError::TooManyRequests { retry_after: 1 })?;
        let _turn = tokio::time::timeout(REACTING_WAIT, REACTING.acquire())
            .await
            .map_err(|_| AppError::TooManyRequests { retry_after: 5 })?
            .map_err(AppError::internal)?;
        let state = FormState {
            operation: op,
            key: request.key.clone(),
            changed: reaction.name.clone(),
            previous,
            values: values
                .iter()
                .filter(|(n, _)| self.state_field(ctx, n))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        };
        // One time limit for the reaction and the checking of its patches.
        let deadline = tokio::time::Instant::now() + reaction.timeout;
        let outcome = match tokio::time::timeout_at(deadline, (reaction.run)(ctx, &state)).await {
            Ok(Ok(patches)) => self.keep_patches(ctx, op, &state, values, patches, deadline).ok_or(None),
            Ok(Err(err)) => Err(Some(err.to_string())),
            Err(_) => Err(None),
        };
        match outcome {
            Ok(patches) => Ok(FormReactResponse { seq: request.seq, patches, failed: false }),
            Err(failure) => {
                let failure = failure.unwrap_or_else(|| format!("took longer than {:?}", reaction.timeout));
                tracing::warn!(resource = R::SLUG, input = reaction.name, failure, "form reaction failed");
                Ok(empty(true))
            }
        }
    }

    /// Whether `value`, sent for `name`, is something the user typed in the
    /// form: an input of the form for a field the user may write, not
    /// encrypted, whose value decodes (`null` included). A JSON repeater
    /// holding more rows than it may is refused (422).
    pub(super) fn typed(&self, ctx: &Ctx, op: Operation, name: &str, value: &Value) -> AppResult<bool> {
        let input_of_form = self.form.inputs().iter().any(|i| i.name == name);
        let field = self.field(name).filter(|f| f.secret.is_none());
        if !input_of_form || field.is_none() || !self.writable(ctx, op, name) {
            return Ok(false);
        }
        if let Some(rows) = self.repeaters.get(name)
            && let Some(list) = value.as_array()
        {
            let max = rows.max_items().unwrap_or(MAX_STATE_ROWS).min(MAX_STATE_ROWS);
            if list.len() > max {
                return Err(ValidationErrors::field(name, format!("may not have more than {max} items")).into());
            }
        }
        Ok(value.is_null() || self.column(name).kind.decode(value).is_ok())
    }

    /// Whether a reaction may see field `name`: readable, not encrypted.
    pub(super) fn state_field(&self, ctx: &Ctx, name: &str) -> bool {
        self.field(name).is_some_and(|f| f.secret.is_none() && f.can_read(ctx))
    }

    /// Whether the user may write field `name` in `op` (as `accept_input`).
    pub(super) fn writable(&self, ctx: &Ctx, op: Operation, name: &str) -> bool {
        let natural_create = op == Operation::Create && matches!(self.meta.key, KeyStrategy::Natural);
        self.field(name).is_some_and(|f| f.can_write(op, ctx)) && (name != self.pk_name || natural_create)
    }

    /// `value` as the column stores it (ADR 0003: a decimal at the column's
    /// scale), for comparing and sending.
    pub(super) fn canonical_value(&self, name: &str, value: &Value) -> Value {
        let column = self.column(name);
        let scale = crate::wire::decimal_scale(column.column.def().get_column_type());
        crate::wire::canonical(column.kind, scale, value).unwrap_or_else(|_| value.clone())
    }

    /// The patches the user could make, in order, each applied to `values`
    /// (the form's) for the next, conditions evaluated again after each;
    /// values canonical, messages translated and bounded. The others are
    /// logged and dropped (the application's bug, not the user's), as are
    /// patches that change nothing and those beyond [`MAX_PATCHES`]. `None`
    /// when the reaction's time runs out before they are all checked.
    fn keep_patches(
        &self,
        ctx: &Ctx,
        op: Operation,
        state: &FormState,
        mut values: Data,
        patches: Vec<FormPatch>,
        deadline: tokio::time::Instant,
    ) -> Option<Vec<FormPatch>> {
        let dropped = |field: &str, reason: &str| {
            tracing::warn!(resource = R::SLUG, input = state.changed, field, reason, "form reaction patch dropped");
        };
        if patches.len() > MAX_PATCHES {
            dropped("", "more patches than an answer may hold");
        }
        let mut kept = Vec::new();
        for patch in patches.into_iter().take(MAX_PATCHES) {
            if tokio::time::Instant::now() >= deadline {
                return None;
            }
            match self.keep_patch(ctx, op, &mut values, patch) {
                Ok(Some(patch)) => kept.push(patch),
                Ok(None) => {}
                Err((field, reason)) => dropped(&field, reason),
            }
        }
        // Time ran out during the last check, or in the reaction's own
        // synchronous work (an answer is never late and successful).
        (tokio::time::Instant::now() < deadline).then_some(kept)
    }

    /// `patch` as the panel applies it (`None`: it changes nothing), applied
    /// to `values`; the field and why it is dropped otherwise.
    fn keep_patch(
        &self,
        ctx: &Ctx,
        op: Operation,
        values: &mut Data,
        patch: FormPatch,
    ) -> Result<Option<FormPatch>, (String, &'static str)> {
        let message = |text: &str| {
            let text = ctx.t_in(R::SLUG, text);
            (text.chars().count() <= MAX_MESSAGE).then_some(text)
        };
        let Some(field) = patch.field().map(str::to_owned) else {
            let FormPatch::Notice { message: text } = &patch else { unreachable!("only notices have no field") };
            let text = message(text).ok_or((String::new(), "a message longer than 500 characters"))?;
            return Ok(Some(FormPatch::notice(text)));
        };
        let fail = |reason: &'static str| (field.clone(), reason);
        let input =
            self.form.inputs().into_iter().find(|i| i.name == field).ok_or_else(|| fail("not an input of the form"))?;
        if matches!(input.kind, InputType::Hidden | InputType::Secret | InputType::SecretMap | InputType::BelongsToMany)
            || input.repeater.as_deref().is_some_and(|s| s.relationship)
            || self.field(&field).is_none_or(|f| f.secret.is_some())
        {
            return Err(fail("a hidden, encrypted, link or relationship repeater input"));
        }
        // The key a relation binds is the relation's (the save binds it again).
        if ctx.bound.as_ref().is_some_and(|(_, bound, _)| *bound == field) {
            return Err(fail("the key a relation binds"));
        }
        // So are a filtered relation's values (ADR 0044).
        if super::operations::bound_filters(ctx).iter().any(|(f, _)| *f == field) {
            return Err(fail("a value the relation sets"));
        }
        // Written into the user's form: a field the user may read and write.
        if !self.writable(ctx, op, &field) || self.field(&field).is_none_or(|f| !f.can_read(ctx)) {
            return Err(fail("not readable and writable by the user"));
        }
        // Conditions on the form as the previous patches left it.
        if self.conditions.hidden(values).contains(&field) {
            return Err(fail("hidden by its conditions"));
        }
        let mut errors = ValidationErrors::new();
        if let FormPatch::Set { value, .. } | FormPatch::SetIfEmpty { value, .. } = &patch {
            let current = values.get(&field);
            if matches!(patch, FormPatch::SetIfEmpty { .. }) && !is_empty(current) {
                return Ok(None);
            }
            let rule = InputRule {
                options: self.input_options(&field),
                bounds: self.number_bounds.get(field.as_str()).copied(),
                ..self.rule(&field, false)
            };
            if !check_input(&field, &rule, value, &mut errors) {
                return Err(fail("a value its input refuses"));
            }
            // Computed options (ADR 0044, part E): not the declared ones, but a
            // checkbox list's shape still holds.
            if self.computed_list_problem(&field, value).is_some() {
                return Err(fail("a value its input refuses"));
            }
            // As a save would store it: rows checked, values canonical.
            let value = match self.repeaters.get(field.as_str()) {
                // `null` passed the input's checks (a nullable column): cleared,
                // unless the repeater needs rows.
                Some(rows) if value.is_null() && rows.min_items().is_some() => {
                    return Err(fail("rows the repeater refuses"));
                }
                _ if value.is_null() => Value::Null,
                Some(rows) => {
                    rows.check(&field, value, &mut errors).ok_or_else(|| fail("rows the repeater refuses"))?
                }
                None => self.canonical_value(&field, value),
            };
            let same = current.is_some_and(|c| {
                *c == value
                    || (!c.is_null()
                        && !self.repeaters.contains_key(field.as_str())
                        && self.canonical_value(&field, c) == value)
            });
            if same {
                return Ok(None);
            }
            values.insert(field.clone(), value.clone());
            return Ok(Some(match patch {
                FormPatch::SetIfEmpty { .. } => FormPatch::SetIfEmpty { field, value },
                _ => FormPatch::Set { field, value },
            }));
        }
        // Row patches: JSON repeaters only, applied to the form's list in place.
        let rows = self
            .repeaters
            .get(field.as_str())
            .ok_or_else(|| fail("a row patch on an input that is not a JSON repeater"))?;
        let entry = values.entry(field.clone()).or_insert_with(|| Value::Array(Vec::new()));
        if !entry.is_array() {
            *entry = Value::Array(Vec::new());
        }
        let Value::Array(list) = entry else { unreachable!("made a list above") };
        let len = list.len();
        let at = |index: u32| {
            usize::try_from(index).ok().filter(|i| *i < len).ok_or_else(|| fail("a row index out of range"))
        };
        Ok(Some(match patch {
            FormPatch::AddRow { index, row, .. } => {
                let index = match index {
                    Some(index) => usize::try_from(index)
                        .ok()
                        .filter(|i| *i <= len)
                        .ok_or_else(|| fail("a row index out of range"))?,
                    None => len,
                };
                if !rows.fits(len + 1) {
                    return Err(fail("more rows than the repeater allows"));
                }
                let checked = rows.check_each(&field, index, std::slice::from_ref(&row), &mut errors);
                let Some(row) = checked.into_iter().next().filter(|_| errors.is_empty()) else {
                    return Err(fail("a row the repeater refuses"));
                };
                list.insert(index, row.clone());
                FormPatch::AddRow { field, index: u32::try_from(index).ok(), row }
            }
            FormPatch::RemoveRow { index, .. } => {
                let index = at(index)?;
                if !rows.fits(len - 1) {
                    return Err(fail("fewer rows than the repeater needs"));
                }
                list.remove(index);
                FormPatch::RemoveRow { field, index: index as u32 }
            }
            FormPatch::MoveRow { from, to, .. } => {
                let (from, to) = (at(from)?, at(to)?);
                if from == to {
                    return Ok(None);
                }
                let row = list.remove(from);
                list.insert(to, row);
                FormPatch::MoveRow { field, from: from as u32, to: to as u32 }
            }
            FormPatch::FlagRow { index, message: text, .. } => {
                at(index)?;
                let text = message(&text).ok_or_else(|| fail("a message longer than 500 characters"))?;
                FormPatch::FlagRow { field, index, message: text }
            }
            FormPatch::Set { .. } | FormPatch::SetIfEmpty { .. } | FormPatch::Notice { .. } => {
                unreachable!("handled above")
            }
        }))
    }
}
