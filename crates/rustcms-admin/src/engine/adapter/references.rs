//! Checks run on what is written, after hooks: `belongs_to` targets and
//! media (also inside rich text) must exist and be visible to the user.

use super::*;

impl<R: Resource> ResourceAdapter<R> {
    /// The final check of what a write stores (after hooks): related records
    /// and media must be visible to the user, media must match its field.
    pub(super) async fn check_written(&self, ctx: &Ctx, txn: &DatabaseTransaction, data: &Data) -> AppResult<()> {
        self.check_relations(ctx, txn, data).await?;
        self.check_media(ctx, txn, data, Vec::new()).await
    }

    /// Media references: well-formed, existing, not trashed, visible to the
    /// user, and of an accepted type and size (see [`crate::media::check_references`]).
    /// Those of `data` (its fields, its JSON repeaters' rows) and `rows` (the
    /// relationship rows the write wrote, ADR 0044) are checked once, last.
    pub(super) async fn check_media(
        &self,
        ctx: &Ctx,
        txn: &DatabaseTransaction,
        data: &Data,
        rows: Vec<crate::engine::repeater::MediaReference>,
    ) -> AppResult<()> {
        let mut wanted = self.media_references("", data)?;
        wanted.extend(rows);
        if !wanted.is_empty() && ctx.registry.get(crate::media::SLUG).is_none() {
            return Err(AppError::internal(format!(
                "`{}`: media fields need the media library (`AdminProvider::media`)",
                R::SLUG
            )));
        }
        crate::media::check_references(ctx, txn, wanted).await
    }

    /// The media `data` refers to (written after hooks): its media fields,
    /// the images of its rich text and the files and images of its JSON
    /// repeaters' rows, each named `prefix` + its path (`options.n1.` for a
    /// relationship row). A malformed value is a 422 at its path.
    pub(super) fn media_references(
        &self,
        prefix: &str,
        data: &Data,
    ) -> AppResult<Vec<crate::engine::repeater::MediaReference>> {
        let mut errors = ValidationErrors::new();
        let mut wanted = Vec::new();
        for field in self.fields.iter().filter(|f| f.rich_text) {
            let Some(value) = data.get(&field.name).filter(|v| !v.is_null()) else { continue };
            let at = format!("{prefix}{}", field.name);
            match crate::rich_text::parse_with(value, self.accepts(field)) {
                Ok(doc) => {
                    let ids = crate::rich_text::media_ids(&doc);
                    if !ids.is_empty() {
                        wanted.push((at, crate::field::MediaRule::images(), ids));
                    }
                }
                Err(e) => errors.add(at.as_str(), e.0),
            }
        }
        for field in &self.fields {
            let (Some(rule), Some(value)) = (&field.media, data.get(&field.name)) else { continue };
            let at = format!("{prefix}{}", field.name);
            if let Some(ids) = crate::media::reference_ids(&at, rule, value, &mut errors) {
                wanted.push((at, rule.clone(), ids));
            }
        }
        for (name, rows) in &self.repeaters {
            if let Some(value) = data.get(name) {
                rows.references(&format!("{prefix}{name}"), value, &mut wanted);
            }
        }
        errors.into_result()?;
        Ok(wanted)
    }

    /// Client-supplied foreign keys must point to records the user can see.
    pub(super) async fn check_relations(&self, ctx: &Ctx, txn: &DatabaseTransaction, input: &Data) -> AppResult<()> {
        let mut errors = ValidationErrors::new();
        for field in &self.fields {
            let (Some(target), Some(value)) = (field.belongs_to, input.get(&field.name)) else {
                continue;
            };
            if value.is_null() {
                continue;
            }
            let key = value.as_str().map_or_else(|| value.to_string(), str::to_owned);
            let target =
                ctx.registry.get(target).ok_or_else(|| AppError::internal(format!("unknown resource `{target}`")))?;
            if !target.keys_in_scope(ctx, txn, std::slice::from_ref(&key)).await?.contains(&key) {
                errors.add(field.name.as_str(), "is invalid");
            }
        }
        errors.into_result()
    }
}
