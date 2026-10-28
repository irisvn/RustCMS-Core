//! What a client may send (≈ Laravel form requests).

use rustcms::prelude::*;
use serde::Deserialize;

// #region new-post
/// A new draft: a title, and an optional body.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewPost {
    pub title: String,
    #[serde(default)]
    pub body: Option<String>,
}
// #endregion new-post

// #region validate
impl Validate for NewPost {
    /// Trims both; an empty body is no body.
    fn validate(&mut self) -> Result<(), ValidationErrors> {
        self.title = self.title.trim().to_owned();
        self.body = self.body.take().map(|b| b.trim().to_owned()).filter(|b| !b.is_empty());
        let mut errors = ValidationErrors::new();
        if self.title.is_empty() || self.title.chars().count() > 200 || self.title.chars().any(char::is_control) {
            errors.add("title", "must be 1 to 200 characters, without control characters");
        }
        if self.body.as_ref().is_some_and(|b| b.chars().count() > 100_000) {
            errors.add("body", "may not be longer than 100000 characters");
        }
        if errors.is_empty() { Ok(()) } else { Err(errors) }
    }
}
// #endregion validate
