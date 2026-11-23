//! Options computed on the server (`options_fn`, ADR 0044 part E): the
//! options of a select or checkbox list of a resource form, read from the
//! form's state. Asked by the panel when the form opens and when an input of
//! `depends_on` changes; evaluated again before every save that could change
//! the input, the value then checked against the answer.

use std::sync::Arc;
use std::time::Duration;

use rustcms_core::AppResult;

use super::SelectOption;
use super::reactions::{FormState, REACTION_TIMEOUT};
use crate::action::BoxFuture;
use crate::context::Ctx;

/// Time an `options_fn` may take by default.
pub const OPTIONS_TIMEOUT: Duration = REACTION_TIMEOUT;
/// The longest [`Input::options_timeout`](super::Input::options_timeout).
pub const OPTIONS_MAX_TIMEOUT: Duration = Duration::from_secs(30);

/// An `options_fn`: from the form's state, the options.
pub(crate) type OptionsFn =
    Arc<dyn for<'a> Fn(&'a Ctx, &'a FormState) -> BoxFuture<'a, AppResult<Vec<SelectOption>>> + Send + Sync>;

/// An input's computed options, as declared (checked at boot).
#[derive(Clone)]
pub(crate) struct OptionsSource {
    /// `None` when `depends_on` or `options_timeout` came without `options_fn`.
    pub(crate) run: Option<OptionsFn>,
    /// The inputs whose change asks again (`None`: not declared).
    pub(crate) depends_on: Option<Vec<String>>,
    pub(crate) timeout: Duration,
}

impl Default for OptionsSource {
    fn default() -> Self {
        Self { run: None, depends_on: None, timeout: OPTIONS_TIMEOUT }
    }
}

impl std::fmt::Debug for OptionsSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OptionsSource")
            .field("run", &self.run.is_some())
            .field("depends_on", &self.depends_on)
            .field("timeout", &self.timeout)
            .finish()
    }
}
