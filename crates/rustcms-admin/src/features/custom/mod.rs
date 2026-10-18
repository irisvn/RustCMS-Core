//! Customizing the panel (ADR 0019): a theme, custom pages and slots drawn
//! from server templates (escaped, then sanitized) or extension scripts, and
//! custom table cells (`ColumnSchema::cell`, drawn by scripts).

mod html;
mod http;
mod page;
mod subpath;
mod template;
mod theme;

use std::sync::Arc;

pub use http::{CustomContent, CustomPageResponse, PageLink, SlotContent, SlotsResponse};
pub(crate) use http::{links, router, routes};
pub(crate) use page::{DynPage, DynSlot, PageAdapter, SlotAdapter};
pub use page::{PageView, PanelPage, PanelPageMeta, PanelSlot, Placement, SlotMeta, TemplateFile};
pub use theme::{ColorScheme, Palette, TOKENS, Theme};

/// Largest `data` a page or slot may send (bytes of JSON).
pub(crate) const MAX_DATA_BYTES: usize = 1024 * 1024;

/// Pages and slots of the panel, with their compiled templates.
pub struct CustomPanel {
    pub(crate) pages: Vec<Arc<dyn DynPage>>,
    pub(crate) slots: Vec<Arc<dyn DynSlot>>,
    pub(crate) templates: template::Templates,
    pub(crate) sanitizer: html::Sanitizer,
    pub(crate) base: String,
    sources: Vec<&'static str>,
}

fn valid_slug(slug: &str) -> bool {
    (1..=64).contains(&slug.len()) && slug.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// `vendor/name`, as widgets.
fn valid_script(name: &str) -> bool {
    let part = |p: &str| {
        (1..=32).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    };
    name.split_once('/').is_some_and(|(vendor, name)| part(vendor) && part(name))
}

impl CustomPanel {
    /// Checks and compiles `pages` and `slots` for the panel under `base`.
    ///
    /// # Panics
    /// On an invalid or repeated slug or name, a page or slot without
    /// permissions, an invalid script name, a template that does not compile,
    /// or a slot beside an unknown resource.
    ///
    /// With `local` (`APP_ENV=local`), template files of debug builds are
    /// read again from disk at each render.
    pub(crate) fn new(
        base: &str,
        pages: Vec<Arc<dyn DynPage>>,
        slots: Vec<Arc<dyn DynSlot>>,
        registry: &crate::registry::Registry,
        local: bool,
    ) -> Self {
        let mut templates = template::Templates::new();
        templates.reload_files(local);
        let mut sources = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for page in &pages {
            let slug = page.slug();
            assert!(valid_slug(slug), "page `{slug}`: slugs are 1-64 of a-z, 0-9 and -");
            assert!(seen.insert(slug), "page `{slug}` registered twice");
            assert!(!page.meta().permissions.is_empty(), "page `{slug}`: declare at least one permission");
            compile(&mut templates, &mut sources, &format!("page:{slug}"), page.view());
        }
        let mut names = std::collections::HashSet::new();
        for slot in &slots {
            let name = slot.name();
            assert!(valid_slug(name), "slot `{name}`: names are 1-64 of a-z, 0-9 and -");
            assert!(names.insert(name), "slot `{name}` registered twice");
            assert!(!slot.meta().permissions.is_empty(), "slot `{name}`: declare at least one permission");
            let Placement::RecordAside(resource) = slot.placement();
            assert!(registry.get(resource).is_some(), "slot `{name}`: no resource `{resource}`");
            compile(&mut templates, &mut sources, &format!("slot:{name}"), slot.view());
        }
        Self {
            pages,
            slots,
            templates,
            sanitizer: html::Sanitizer::new(base),
            base: base.trim_end_matches('/').to_owned(),
            sources,
        }
    }

    /// No pages and no slots (panels built by hand, tests).
    pub fn empty(base: &str) -> Self {
        Self::new(base, Vec::new(), Vec::new(), &crate::registry::Registry::new(), false)
    }

    /// Labels, groups and the `t("…")` literals of every template (for `lang:missing`).
    pub(crate) fn display_strings(&self) -> Vec<String> {
        let mut out = Vec::new();
        for page in &self.pages {
            out.push(page.meta().label.clone());
            out.extend(page.meta().group.clone());
        }
        for slot in &self.slots {
            out.extend(slot.meta().label.clone());
        }
        out.extend(template::Templates::literals(&self.sources));
        out
    }

    /// Permissions pages declare for `permission:sync`.
    pub(crate) fn declared_permissions(&self) -> Vec<String> {
        self.pages.iter().flat_map(|p| p.meta().declared.clone()).collect()
    }
}

fn compile(templates: &mut template::Templates, sources: &mut Vec<&'static str>, name: &str, view: PageView) {
    match view {
        PageView::Template(source) => {
            templates.add(name, source);
            sources.push(source);
        }
        PageView::File(file) => {
            templates.add(name, file.source);
            sources.push(file.source);
            if file.debug {
                templates.watch(name, file.path);
            }
        }
        PageView::Script(script) => {
            assert!(valid_script(script), "{name}: scripts are named `vendor/name` (a-z, 0-9, -), not `{script}`")
        }
    }
}
