//! The admin panel (like `app/Filament`): `resources` (one per model) and
//! `pages` (settings pages).

pub mod pages;
pub mod resources;

/// The integer `field` of `data` (keys travel as strings, ADR 0003).
pub(crate) fn int(data: &rustcms::admin::Data, field: &str) -> Option<i64> {
    match data.get(field)? {
        serde_json::Value::String(text) => text.parse().ok(),
        value => value.as_i64(),
    }
}
