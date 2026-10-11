//! `rustcms new`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, bail};
use include_dir::{Dir, DirEntry, include_dir};
use minijinja::Environment;
use minijinja::syntax::SyntaxConfig;
use serde::Serialize;

use crate::naming::validate_package;

pub static APP_TEMPLATE: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/templates/app");

/// Keep in sync with the workspace `sea-orm` pin.
pub const SEA_ORM_VERSION: &str = "2.0.4";
const RUSTCMS_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Serialize)]
struct TemplateVars {
    name: String,
    crate_name: String,
    app_key: String,
    rustcms_version: &'static str,
    rustcms_dep: String,
    rustcms_testing_dep: String,
    sea_orm_version: &'static str,
    in_workspace: bool,
}

pub fn new_app(name: &str, parent: &Path, rustcms_path: Option<&str>, in_workspace: bool) -> anyhow::Result<ExitCode> {
    validate_package(name)?;
    let root = parent.join(name);
    if root.exists() {
        bail!("{} already exists", root.display());
    }
    let (rustcms_dep, rustcms_testing_dep) = match rustcms_path.map(str::to_owned).or_else(local_checkout) {
        Some(path) => (
            format!("{{ path = \"{path}/crates/rustcms\" }}"),
            format!("{{ path = \"{path}/crates/rustcms-testing\" }}"),
        ),
        None => (format!("\"{RUSTCMS_VERSION}\""), format!("\"{RUSTCMS_VERSION}\"")),
    };
    let context = TemplateVars {
        name: name.to_owned(),
        crate_name: name.replace('-', "_"),
        app_key: rustcms_core::AppKey::generate().encode(),
        rustcms_version: RUSTCMS_VERSION,
        rustcms_dep,
        rustcms_testing_dep,
        sea_orm_version: SEA_ORM_VERSION,
        in_workspace,
    };
    render_dir(&APP_TEMPLATE, &root, &context)?;
    println!("Created {}", root.display());
    println!("\n  cd {}\n  rustcms migrate\n  rustcms serve\n", root.display());
    Ok(ExitCode::SUCCESS)
}

/// The checkout this binary was built from, when it still exists (development builds).
fn local_checkout() -> Option<String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent()?.parent()?;
    root.join("crates/rustcms/Cargo.toml").exists().then(|| root.display().to_string())
}

pub fn render(template: &str, context: impl Serialize) -> anyhow::Result<String> {
    let mut env = Environment::new();
    env.set_syntax(SyntaxConfig::builder().trim_blocks(true).keep_trailing_newline(true).build()?);
    Ok(env.render_str(template, minijinja::value::Serde(context))?)
}

fn render_dir(dir: &Dir<'_>, target: &Path, context: &TemplateVars) -> anyhow::Result<()> {
    for entry in dir.entries() {
        match entry {
            DirEntry::Dir(sub) => render_dir(sub, target, context)?,
            DirEntry::File(file) => {
                let output = target.join(output_path(file.path()));
                let template =
                    file.contents_utf8().with_context(|| format!("{} is not UTF-8", file.path().display()))?;
                let contents =
                    render(template, context).with_context(|| format!("rendering {}", file.path().display()))?;
                if let Some(parent) = output.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(&output, contents).with_context(|| format!("writing {}", output.display()))?;
            }
        }
    }
    Ok(())
}

/// `config/dot-env.tmpl` → `config/.env`.
fn output_path(path: &Path) -> PathBuf {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    let name = name.strip_suffix(".tmpl").unwrap_or(name);
    let name = name.strip_prefix("dot-").map_or_else(|| name.to_owned(), |rest| format!(".{rest}"));
    path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_template_paths() {
        assert_eq!(output_path(Path::new("dot-env.tmpl")), Path::new(".env"));
        assert_eq!(output_path(Path::new("src/main.rs.tmpl")), Path::new("src/main.rs"));
    }

    #[test]
    fn rejects_existing_directory_and_bad_names() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("blog")).unwrap();
        assert!(new_app("blog", dir.path(), Some("/x"), false).is_err());
        assert!(new_app("../evil", dir.path(), Some("/x"), false).is_err());
    }
}
