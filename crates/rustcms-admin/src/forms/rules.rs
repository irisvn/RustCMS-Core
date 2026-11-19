//! Rules of an input computed on the server (ADR 0044, part E): whether it is
//! shown, whether it is required, which options it offers. Application code,
//! run when the form is read, when an input it depends on changes, and again on
//! every save.

use std::sync::Arc;
use std::time::Duration;

use rustcms_core::AppResult;

use super::SelectOption;
use super::reactions::{FormState, REACTION_TIMEOUT};
use crate::action::BoxFuture;
use crate::context::Ctx;

/// A rule's function: from the form's state, its answer.
pub(crate) type RuleFn<T> = Arc<dyn for<'a> Fn(&'a Ctx, &'a FormState) -> BoxFuture<'a, AppResult<T>> + Send + Sync>;

/// An input's rules, as declared (checked at boot).
#[derive(Clone)]
pub(crate) struct InputRules {
    pub(crate) shown: Option<RuleFn<bool>>,
    pub(crate) required: Option<RuleFn<bool>>,
    pub(crate) options: Option<RuleFn<Vec<SelectOption>>>,
    /// The inputs whose change asks the server again (`None`: not declared).
    pub(crate) depends_on: Option<Vec<String>>,
    pub(crate) timeout: Duration,
}

impl Default for InputRules {
    fn default() -> Self {
        Self { shown: None, required: None, options: None, depends_on: None, timeout: REACTION_TIMEOUT }
    }
}

impl InputRules {
    /// Whether any rule is declared.
    pub(crate) fn any(&self) -> bool {
        self.shown.is_some() || self.required.is_some() || self.options.is_some()
    }
}

impl std::fmt::Debug for InputRules {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InputRules")
            .field("shown", &self.shown.is_some())
            .field("required", &self.required.is_some())
            .field("options", &self.options.is_some())
            .field("depends_on", &self.depends_on)
            .field("timeout", &self.timeout)
            .finish()
    }
}
