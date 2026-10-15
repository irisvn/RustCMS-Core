//! Where generated files go (the application layout `rustcms new` creates)
//! and the module plumbing that makes a new file part of the crate.

use std::path::PathBuf;

use anyhow::bail;

use crate::generate::register;
use crate::project::Project;

/// `src/lib.rs` anchor for top-level modules created on demand.
pub const LIB_MODS: &str = "// rustcms:mods";

/// A module directory under `src/` (`services`, `database/factories`, …).
pub struct ModuleDir {
    /// Path below `src/`, `/`-separated.
    pub path: &'static str,
    /// Doc comment of a new `mod.rs`.
    pub doc: &'static str,
    /// Anchor where generators add `pub mod …;`.
    pub anchor: &'static str,
}

pub const MODELS: ModuleDir = ModuleDir {
    path: "models",
    doc: "SeaORM entities and their query scopes. `rustcms make:model <Name>` adds modules here.",
    anchor: "// rustcms:models",
};
pub const POLICIES: ModuleDir = ModuleDir {
    path: "policies",
    doc: "Policies: who may do what. `rustcms make:policy <Name>` adds modules here.",
    anchor: "// rustcms:policies",
};
pub const SERVICES: ModuleDir = ModuleDir {
    path: "services",
    doc: "Business rules shared by the admin panel, the API, jobs and seeders. `rustcms make:service <Name>` adds modules here.",
    anchor: "// rustcms:services",
};
pub const JOBS: ModuleDir = ModuleDir {
    path: "jobs",
    doc: "Background jobs. `rustcms make:job <Name>` adds modules here.",
    anchor: "// rustcms:jobs",
};
pub const SETTINGS: ModuleDir = ModuleDir {
    path: "settings",
    doc: "Settings stored in the database (their admin pages are in `crate::admin::pages`). `rustcms make:settings <Name>` adds modules here.",
    anchor: "// rustcms:settings",
};
pub const CONSOLE: ModuleDir = ModuleDir {
    path: "console",
    doc: "Console commands of the application. `rustcms make:command <name>` adds modules here.",
    anchor: "// rustcms:commands",
};
pub const RESOURCES: ModuleDir = ModuleDir {
    path: "admin/resources",
    doc: "Admin resources, one per model (`rustcms make:resource <Model>`).",
    anchor: "// rustcms:resource-mods",
};
pub const PAGES: ModuleDir = ModuleDir {
    path: "admin/pages",
    doc: "Admin pages that are not resources (`rustcms make:settings <Name>`).",
    anchor: "// rustcms:page-mods",
};
pub const CONTROLLERS: ModuleDir = ModuleDir {
    path: "http/controllers",
    doc: "Request handlers, one module per model (`rustcms make:api <Model>`).",
    anchor: "// rustcms:controller-mods",
};
pub const HTTP_RESOURCES: ModuleDir = ModuleDir {
    path: "http/resources",
    doc: "What the API returns for each model (like Laravel API resources).",
    anchor: "// rustcms:http-resource-mods",
};
pub const SEEDERS: ModuleDir = ModuleDir {
    path: "database/seeders",
    doc: "Seeders (`rustcms db:seed` runs `DatabaseSeeder`).",
    anchor: "// rustcms:seeder-mods",
};
pub const FACTORIES: ModuleDir = ModuleDir {
    path: "database/factories",
    doc: "Model factories for seeders and tests. `rustcms make:factory <Model>` adds modules here.",
    anchor: "// rustcms:factories",
};
pub const MIGRATIONS: &str = "database/migrations";

/// Anchor in a parent module where `pub mod <child>;` goes.
fn parent_anchor(parent: &str) -> &'static str {
    match parent {
        "" => LIB_MODS,
        "database" => "// rustcms:database-mods",
        "http" => "// rustcms:http-mods",
        _ => "// rustcms:mods",
    }
}

impl Project {
    /// `src/<path>`.
    pub fn src(&self, path: &str) -> PathBuf {
        self.root.join("src").join(path)
    }

    /// Refuses applications laid out before 0.3 (`src/app.rs`, `src/migrations`):
    /// generators write the current layout only.
    pub fn require_layout(&self) -> anyhow::Result<()> {
        let legacy = ["src/app.rs", "src/migrations", "src/admin.rs", "src/http/api"]
            .into_iter()
            .find(|path| self.root.join(path).exists());
        if let Some(path) = legacy {
            bail!(
                "{} still has `{path}` from the layout before 0.3: move it first, see the upgrade guide (\"Application layout\")",
                self.root.display()
            );
        }
        if !self.src("bootstrap/mod.rs").exists() {
            bail!(
                "{} predates the 0.3 layout (no src/bootstrap/mod.rs): move it first, see the upgrade guide (\"Application layout\")",
                self.root.display()
            );
        }
        Ok(())
    }

    /// Makes sure `dir` is a module of the crate: creates its `mod.rs` (doc and
    /// anchor) and declares it in its parent, creating parents as needed.
    /// Returns its `mod.rs`.
    pub fn ensure_module(&self, dir: &ModuleDir) -> anyhow::Result<PathBuf> {
        self.ensure_module_path(dir.path, dir.doc, dir.anchor)
    }

    /// [`Self::ensure_module`], then checks its anchor: the module is ready
    /// for a `pub mod` line.
    pub fn prepare_module(&self, dir: &ModuleDir) -> anyhow::Result<PathBuf> {
        let index = self.ensure_module(dir)?;
        crate::generate::preflight(&[], &[(&index, dir.anchor)])?;
        Ok(index)
    }

    fn ensure_module_path(&self, path: &str, doc: &str, anchor: &str) -> anyhow::Result<PathBuf> {
        let index = self.src(path).join("mod.rs");
        let (parent, name) = path.rsplit_once('/').unwrap_or(("", path));
        let parent_file = if parent.is_empty() { self.src("lib.rs") } else { self.src(parent).join("mod.rs") };
        // `src/services.rs` and `src/services/mod.rs` cannot both be the module.
        let flat = self.src(&format!("{path}.rs"));
        if flat.exists() {
            bail!("{} exists: generators use {} instead; move it there first", flat.display(), index.display());
        }
        if index.exists() {
            let declared = std::fs::read_to_string(&parent_file).is_ok_and(|parent| {
                parent
                    .lines()
                    .any(|l| matches!(l.trim(), d if d == format!("pub mod {name};") || d == format!("mod {name};")))
            });
            if !declared {
                bail!("{} exists but {} does not declare `pub mod {name};`", index.display(), parent_file.display());
            }
            return Ok(index);
        }
        if !parent.is_empty() {
            let doc = format!("The `{parent}` modules.");
            self.ensure_module_path(parent, &doc, parent_anchor(parent))?;
        }
        crate::generate::preflight(&[], &[(&parent_file, parent_anchor(parent))])?;
        std::fs::create_dir_all(self.src(path))?;
        std::fs::write(&index, format!("//! {doc}\n\n{anchor}\n"))?;
        println!("Created {}", index.display());
        register(&parent_file, parent_anchor(parent), &format!("pub mod {name};"))?;
        Ok(index)
    }
}
