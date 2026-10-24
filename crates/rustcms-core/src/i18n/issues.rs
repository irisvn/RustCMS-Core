//! Machine-readable validation messages: a stable code and its parameters.
//!
//! Every message the framework writes into [`ValidationErrors`](crate::ValidationErrors)
//! follows one of the English [`TEMPLATES`]; the issue is derived from the
//! message, so call sites keep writing plain English and the admin panel
//! translates by code. A message that follows no template (an application's own
//! text) becomes the `custom` issue, shown as written.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::Serialize;

/// One validation problem: a stable code (see [`TEMPLATES`]) and the values of
/// its placeholders, as text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Issue {
    /// Stable code, e.g. `max_length`; `custom` for application messages.
    pub code: &'static str,
    /// Placeholder values (`{"max": "200"}`); for `custom`, `{"message": …}`.
    pub params: BTreeMap<String, String>,
}

impl Issue {
    /// The issue for an English message: the template it follows, or `custom`.
    pub fn of(message: &str) -> Self {
        for template in compiled() {
            if let Some(params) = template.capture(message) {
                return Self { code: template.code, params };
            }
        }
        Self { code: CUSTOM, params: BTreeMap::from([("message".to_owned(), message.to_owned())]) }
    }
}

/// Code of messages that follow no template.
pub const CUSTOM: &str = "custom";

/// `(code, English template)`. Placeholders are `{name}`; a code is never
/// reused for another meaning. Documented in the admin protocol.
pub const TEMPLATES: &[(&str, &str)] = &[
    // Presence and shape.
    ("required", "is required"),
    ("invalid", "is invalid"),
    ("not_allowed", "is not allowed"),
    ("not_found", "was not found"),
    ("several_matches", "matches several records"),
    ("not_imported", "could not be imported"),
    ("set_by_relation", "is set by the relation"),
    ("taken", "has already been taken"),
    ("confirmation", "does not match"),
    ("empty", "is empty"),
    ("not_option", "is not one of the options"),
    ("unreadable_secret", "cannot be decrypted; replace or clear it"),
    ("given_twice", "may only be given once"),
    ("not_instant", "must be a date and time (RFC 3339)"),
    ("not_after_since", "must be after since"),
    ("token_invalid", "is invalid or has expired"),
    // Lengths and counts.
    ("max_length", "may not be longer than {max} characters"),
    ("max_bytes", "may not be longer than {max} bytes"),
    ("min_length", "must be at least {min} characters"),
    ("too_long", "is too long"),
    ("too_large", "is too large"),
    ("max_items", "may not have more than {max} items"),
    ("list_size", "must be a list of 1 to {max} items"),
    ("max_kib", "may not be larger than {max} KiB"),
    ("max_elements", "may not have more than {max} elements"),
    ("max_images", "may not have more than {max} images"),
    ("max_embeds", "may not have more than {max} embeds"),
    ("too_many_roles", "has too many roles"),
    ("too_many_permissions", "has too many permissions"),
    ("unknown_roles", "has unknown roles"),
    // Types and formats (wire format).
    ("integer", "must be an integer"),
    ("integer_string", "must be an integer string"),
    ("number", "must be a number"),
    ("number_range", "must be a number from 1 to {max}"),
    ("positive_integer", "must be a positive integer"),
    ("boolean", "must be true or false"),
    ("string", "must be a string"),
    ("decimal_string", "must be a decimal string"),
    ("uuid", "must be a UUID"),
    ("date", "must be a date (YYYY-MM-DD)"),
    ("local_datetime", "must be a local date-time (YYYY-MM-DDTHH:MM:SS)"),
    ("timestamp", "must be an RFC 3339 timestamp"),
    ("out_of_range", "is out of range"),
    ("between", "must be between 1 and {max}"),
    ("email", "must be a valid email address"),
    ("control_characters", "may not contain control characters"),
    ("name_format", "must be at most {max} characters, without control characters"),
    ("text_object", "must be an object of text values"),
    ("empty_key", "may not have an empty key"),
    ("key_length", "may not have keys longer than {max} characters"),
    ("value_length", "may not have values longer than {max} characters"),
    ("token_name", "must be 1 to 255 characters"),
    ("abilities", "must be at most {max} non-empty names"),
    ("jwt_abilities", "must be 1 to {max} non-empty names, {bytes} bytes in all"),
    ("jwt_too_long", "too long for an access token"),
    ("idempotency_key", "must be 1 to 128 letters, digits, `_` or `-`"),
    ("idempotency_reused", "was already used for a different request; send a new key for a new change"),
    // Lists, filters, sorting.
    ("cannot_filter", "cannot be filtered"),
    ("cannot_sort", "cannot sort by `{name}`"),
    ("trashed_value", "must be `with` or `only`"),
    ("trashed_unsupported", "is not supported by this resource"),
    ("malformed_filter", "malformed `{key}`"),
    ("unknown_operator", "unknown operator `{op}`"),
    ("search_too_broad", "matches too many records to check; refine the search"),
    ("too_many_matches", "Too many matching records to check individually; narrow the search or filters."),
    ("too_many_to_export", "More than {max} records to export; narrow the search or filters."),
    ("too_large_to_export", "Too large to export at once; export in the background."),
    ("invalid_parent", "invalid parent key"),
    ("invalid_key", "invalid key"),
    ("unsupported_key", "unsupported key type"),
    // Records and references.
    ("conflicts", "conflicts with existing data"),
    ("missing_reference", "references a missing or protected record"),
    ("stale", "This was changed by someone else meanwhile. Reload and try again."),
    ("last_super_admin", "At least one active super admin is required for the `{guard}` guard."),
    ("no_super_admin_role", "no super_admin role for guard `{guard}`"),
    ("unknown_permissions", "unknown permissions: {list}"),
    // Files and media.
    ("duplicate_file", "may not list the same file twice"),
    ("file_types", "does not accept {types} files"),
    ("multipart", "must be sent as multipart/form-data"),
    ("file_part", "must be the only part, named `file`"),
    ("file_name_length", "has a name longer than 255 characters"),
    ("unreadable", "could not be read"),
    ("file_type", "is not an accepted file type"),
    ("incomplete_file", "is not a complete file of its type"),
    ("image", "is not a valid image"),
    ("too_many_pixels", "has too many pixels"),
    // Rich text documents.
    ("document", "is not a document"),
    ("document_invalid", "is not a valid document"),
    ("document_version", "is not a valid document (version 1 or 2)"),
    ("document_needs_version", "has a table, which needs document version 2"),
    ("document_needs_version", "has an embed, which needs document version 2"),
    ("document_needs_version", "has a callout, which needs document version 2"),
    ("document_needs_version", "has an image caption, which needs document version 2"),
    ("document_needs_version", "has an image width, which needs document version 2"),
    ("document_depth", "is nested too deeply"),
    ("document_structure", "has a list item outside a list"),
    ("document_structure", "has a list holding something else than list items"),
    ("document_structure", "has a code block holding something else than plain text"),
    ("document_structure", "has a heading without a level from 2 to 4"),
    ("document_structure", "has unexpected properties on `{node}`"),
    ("document_structure", "has an unknown block `{node}`"),
    ("document_structure", "has an unknown inline `{node}`"),
    ("document_structure", "has an unknown mark `{node}`"),
    ("document_structure", "has a mark with attributes"),
    ("document_structure", "has a text with the same mark twice"),
    ("document_structure", "has a text with too many marks"),
    ("document_structure", "has a text without text"),
    ("document_structure", "has an empty text"),
    ("document_link", "has a link that is not http, https or mailto"),
    ("document_link", "has a link with unexpected attributes"),
    ("document_image", "has an image that is not a media item"),
    ("document_image", "has an image with unexpected attributes"),
    ("document_alt", "has an image description longer than {max} characters"),
    ("document_caption", "has an image caption longer than {max} characters"),
    ("document_image", "has an image width other than `normal`, `wide` or `full`"),
    ("document_table_rows", "has a table with no rows or more than {max} rows"),
    ("document_table_columns", "has a table row with no cells or more than {max} cells"),
    ("document_structure", "has a table holding something else than rows"),
    ("document_structure", "has a table whose rows have different lengths"),
    ("document_structure", "has a table cell out of place (header cells make up the first row only)"),
    ("document_structure", "has a table cell holding something else than paragraphs"),
    ("document_structure", "has a callout holding something else than paragraphs"),
    ("document_structure", "has a callout with unexpected attributes"),
    ("document_structure", "has a callout whose tone is not `info` or `warn`"),
    ("document_embed", "has an embed whose provider is not youtube, vimeo or x"),
    ("document_embed", "has an embed with an invalid id"),
    ("document_embed", "has an embed with unexpected attributes"),
    // Accounts.
    ("bad_credentials", "These credentials do not match our records."),
    ("wrong_password", "The password is incorrect."),
    ("password_for_email", "is required to change your email"),
    // Request bodies (`detail` is the parser's English explanation).
    ("invalid_body", "Failed to deserialize the JSON body into the target type: {detail}"),
];

/// A template split into literal text and placeholders.
struct Compiled {
    code: &'static str,
    /// Literal before each placeholder, then the trailing literal.
    literals: Vec<&'static str>,
    names: Vec<&'static str>,
}

impl Compiled {
    fn new(code: &'static str, template: &'static str) -> Self {
        let (mut literals, mut names, mut rest) = (Vec::new(), Vec::new(), template);
        while let Some(start) = rest.find('{') {
            let end = rest[start..].find('}').map(|e| start + e).expect("closed placeholder");
            literals.push(&rest[..start]);
            names.push(&rest[start + 1..end]);
            rest = &rest[end + 1..];
        }
        literals.push(rest);
        Self { code, literals, names }
    }

    /// The placeholder values when `message` follows this template.
    fn capture(&self, message: &str) -> Option<BTreeMap<String, String>> {
        let mut rest = message.strip_prefix(self.literals[0])?;
        let mut params = BTreeMap::new();
        for (i, name) in self.names.iter().enumerate() {
            let next = self.literals[i + 1];
            let value = if i + 1 == self.names.len() {
                let value = rest.strip_suffix(next)?;
                rest = "";
                value
            } else {
                let at = rest.find(next)?;
                let value = &rest[..at];
                rest = &rest[at + next.len()..];
                value
            };
            // Empty values are values (`unknown operator ``` for `filter[x][]`).
            if value.contains(['\n', '\r']) {
                return None;
            }
            params.insert((*name).to_owned(), value.to_owned());
        }
        rest.is_empty().then_some(params)
    }
}

fn compiled() -> &'static [Compiled] {
    static COMPILED: OnceLock<Vec<Compiled>> = OnceLock::new();
    COMPILED.get_or_init(|| TEMPLATES.iter().map(|(code, t)| Compiled::new(code, t)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect()
    }

    #[test]
    fn messages_map_to_codes_and_params() {
        assert_eq!(Issue::of("is required"), Issue { code: "required", params: params(&[]) });
        assert_eq!(
            Issue::of("may not be longer than 200 characters"),
            Issue { code: "max_length", params: params(&[("max", "200")]) }
        );
        assert_eq!(
            Issue::of("cannot sort by `secret`"),
            Issue { code: "cannot_sort", params: params(&[("name", "secret")]) }
        );
        assert_eq!(
            Issue::of("must be 1 to 100 non-empty names, 2048 bytes in all"),
            Issue { code: "jwt_abilities", params: params(&[("max", "100"), ("bytes", "2048")]) }
        );
        assert_eq!(Issue::of("does not accept image/svg+xml files").code, "file_types");
        assert_eq!(Issue::of("unknown operator ``"), Issue { code: "unknown_operator", params: params(&[("op", "")]) });
    }

    #[test]
    fn other_text_is_custom() {
        for message in ["is required!", "Title is required", "may not be longer than 5 bytes or so", "whatever"] {
            let issue = Issue::of(message);
            assert_eq!(issue.code, CUSTOM, "{message}");
            assert_eq!(issue.params["message"], message);
        }
    }

    #[test]
    fn templates_are_well_formed() {
        for (code, template) in TEMPLATES {
            assert!(code.chars().all(|c| c.is_ascii_lowercase() || c == '_'), "{code}");
            assert_eq!(template.matches('{').count(), template.matches('}').count(), "{template}");
        }
    }
}
