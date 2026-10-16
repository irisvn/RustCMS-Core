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

/// How `rustcms new` writes the application.
pub struct NewOptions<'a> {
    /// A local rustcms checkout instead of the Git tag (contributors, CI).
    pub rustcms_path: Option<&'a str>,
    /// Inside another workspace: no `[workspace]`, no lockfile of its own.
    pub in_workspace: bool,
    /// Resolve the dependencies (`cargo generate-lockfile`) once written.
    pub fetch: bool,
}

pub fn new_app(name: &str, parent: &Path, options: &NewOptions<'_>) -> anyhow::Result<ExitCode> {
    validate_package(name)?;
    let root = parent.join(name);
    if root.exists() {
        bail!("{} already exists", root.display());
    }
    let (rustcms_dep, rustcms_testing_dep) = dependencies(options.rustcms_path);
    let context = TemplateVars {
        name: name.to_owned(),
        crate_name: name.replace('-', "_"),
        app_key: rustcms_core::AppKey::generate().encode(),
        rustcms_version: RUSTCMS_VERSION,
        rustcms_dep,
        rustcms_testing_dep,
        sea_orm_version: SEA_ORM_VERSION,
        in_workspace: options.in_workspace,
    };
    render_dir(&APP_TEMPLATE, &root, &context)?;
    println!("Created {}", root.display());
    if options.fetch && !options.in_workspace {
        resolve(&root)?;
    }
    println!("\n  cd {}\n  rustcms migrate\n  rustcms serve\n", root.display());
    Ok(ExitCode::SUCCESS)
}

/// `rustcms` and `rustcms-testing` from the same source: the Git tag of this
/// version, or a local checkout.
fn dependencies(rustcms_path: Option<&str>) -> (String, String) {
    let dep = |krate: &str| match rustcms_path {
        Some(path) => format!("{{ path = {} }}", toml_string(&format!("{path}/crates/{krate}"))),
        None => format!(
            "{{ git = {}, tag = {} }}",
            toml_string(env!("CARGO_PKG_REPOSITORY")),
            toml_string(&format!("v{RUSTCMS_VERSION}"))
        ),
    };
    (dep("rustcms"), dep("rustcms-testing"))
}

/// A TOML basic string (quotes and backslashes escaped: Windows paths).
fn toml_string(value: &str) -> String {
    toml::Value::String(value.to_owned()).to_string()
}

/// Writes `Cargo.lock`: the tag exists and every dependency resolves. A
/// network failure leaves the application usable offline later; anything
/// else is a broken application and fails.
fn resolve(root: &Path) -> anyhow::Result<()> {
    println!("Resolving dependencies…");
    let output = std::process::Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .arg("generate-lockfile")
        .current_dir(root)
        .output()
        .context("cannot run cargo (is Rust installed? see `rustcms doctor`)")?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    match classify(&stderr) {
        Failure::Network => {
            eprintln!(
                "warning: dependencies not resolved (network): Cargo fetches them on the first build.\n{}",
                stderr.trim()
            );
            Ok(())
        }
        Failure::Broken => {
            bail!("{} was created, but its dependencies do not resolve:\n{}", root.display(), stderr.trim())
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Failure {
    Network,
    Broken,
}

fn classify(stderr: &str) -> Failure {
    let lower = stderr.to_lowercase();
    // A missing tag or revision is never a network problem, whatever else Cargo says.
    let broken = ["not found", "failed to find", "no matching package", "failed to parse", "failed to select"];
    let network = [
        "could not resolve host",
        "failed to connect",
        "timed out",
        "network",
        "connection refused",
        "ssl",
        "spurious",
    ];
    if broken.iter().any(|m| lower.contains(m)) {
        Failure::Broken
    } else if network.iter().any(|m| lower.contains(m)) {
        Failure::Network
    } else {
        Failure::Broken
    }
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

/// `config/dot-env.tmpl` → `config/.env`, `dot-github/…` → `.github/…`.
fn output_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.iter() {
        let name = part.to_str().unwrap_or_default();
        let name = name.strip_suffix(".tmpl").unwrap_or(name);
        out.push(name.strip_prefix("dot-").map_or_else(|| name.to_owned(), |rest| format!(".{rest}")));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_template_paths() {
        assert_eq!(output_path(Path::new("dot-env.tmpl")), Path::new(".env"));
        assert_eq!(output_path(Path::new("src/main.rs.tmpl")), Path::new("src/main.rs"));
        assert_eq!(output_path(Path::new("dot-github/workflows/ci.yml.tmpl")), Path::new(".github/workflows/ci.yml"));
    }

    #[test]
    fn rejects_existing_directory_and_bad_names() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("blog")).unwrap();
        let options = NewOptions { rustcms_path: Some("/x"), in_workspace: false, fetch: false };
        assert!(new_app("blog", dir.path(), &options).is_err());
        assert!(new_app("../evil", dir.path(), &options).is_err());
    }

    /// The generated manifest, parsed.
    fn manifest(rustcms_path: Option<&str>) -> toml::Table {
        let dir = tempfile::tempdir().unwrap();
        new_app("blog", dir.path(), &NewOptions { rustcms_path, in_workspace: false, fetch: false }).unwrap();
        std::fs::read_to_string(dir.path().join("blog/Cargo.toml")).unwrap().parse().unwrap()
    }

    #[test]
    fn depends_on_the_git_tag_of_this_version_by_default() {
        let manifest = manifest(None);
        let tag = format!("v{RUSTCMS_VERSION}");
        for (section, krate) in [("dependencies", "rustcms"), ("dev-dependencies", "rustcms-testing")] {
            let dep = &manifest[section][krate];
            assert_eq!(dep["git"].as_str(), Some("https://github.com/irisvn/RustCMS-Core"), "{krate}");
            assert_eq!(dep["tag"].as_str(), Some(tag.as_str()), "{krate}");
            assert!(dep.get("path").is_none());
        }
        assert_eq!(manifest["package"]["metadata"]["rustcms"]["version"].as_str(), Some(RUSTCMS_VERSION));
    }

    #[test]
    fn a_local_checkout_is_used_only_when_given_and_any_path_stays_valid_toml() {
        for path in ["/src/rust cms", r"C:\Users\Dev\RustCMS", r#"/odd"quote"#] {
            let manifest = manifest(Some(path));
            let dep = manifest["dependencies"]["rustcms"]["path"].as_str().unwrap().to_owned();
            assert_eq!(dep, format!("{path}/crates/rustcms"));
            let testing = manifest["dev-dependencies"]["rustcms-testing"]["path"].as_str().unwrap().to_owned();
            assert_eq!(testing, format!("{path}/crates/rustcms-testing"));
        }
    }

    #[test]
    fn writes_the_application_ci() {
        let dir = tempfile::tempdir().unwrap();
        new_app("blog", dir.path(), &NewOptions { rustcms_path: None, in_workspace: false, fetch: false }).unwrap();
        let ci = std::fs::read_to_string(dir.path().join("blog/.github/workflows/ci.yml")).unwrap();
        assert!(ci.contains("cargo test --locked"), "{ci}");
        assert!(ci.contains("${{ runner.os }}"), "GitHub expressions kept: {ci}");
    }

    #[test]
    fn resolution_failures_are_classified() {
        let tag = "error: failed to get `rustcms` as a dependency\n  failed to find tag `v9.9.9`\n  network failure seems to have happened";
        assert_eq!(classify(tag), Failure::Broken);
        assert_eq!(classify("error: revspec 'refs/tags/v9.9.9' not found"), Failure::Broken);
        assert_eq!(classify("failed to fetch: Could not resolve host: github.com"), Failure::Network);
        assert_eq!(classify("error: operation timed out"), Failure::Network);
        assert_eq!(classify("error: something else entirely"), Failure::Broken);
    }
}
