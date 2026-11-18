//! Collections of the media library ([`crate::AdminProvider::media_collection`]):
//! named groups that sort and filter files. A file is in one collection at
//! most, set by the upload (the input's collection) and changed on its
//! record. Collections are never a boundary of access: who sees and may
//! reference a file is decided by the media scope and policy alone, and
//! records may reference files of any collection.

/// Longest collection name (ASCII only: as many bytes as characters).
pub const MAX_COLLECTION: usize = 64;

/// A declared collection: its name (stored on the files) and its label
/// (translated where it is shown).
#[derive(Debug, Clone)]
pub struct MediaCollection {
    pub(crate) name: String,
    pub(crate) label: String,
}

impl MediaCollection {
    /// Name, as stored on the files.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Label, before translation.
    pub fn label(&self) -> &str {
        &self.label
    }
}

/// The declared collections as select options, labels translated, in
/// declaration order.
pub(crate) fn options(ctx: &crate::context::Ctx) -> Vec<crate::forms::SelectOption> {
    ctx.registry
        .media_collections()
        .iter()
        .map(|c| crate::forms::SelectOption { value: c.name.clone().into(), label: ctx.t(&c.label) })
        .collect()
}

/// Whether `name` is a valid collection name: 1 to 64 of `a-z`, `0-9`, `.`,
/// `_`, `-`.
pub(crate) fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_COLLECTION
        && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_checked() {
        for good in ["a", "post-covers", "questions.image", "x_1", &"a".repeat(64)] {
            assert!(valid_name(good), "{good}");
        }
        for bad in ["", "Post", "a b", "~none", "a/b", "ảnh", &"a".repeat(65)] {
            assert!(!valid_name(bad), "{bad}");
        }
    }
}
