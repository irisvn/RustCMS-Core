//! The rustcms application around the current directory.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use anyhow::{Context, bail};

/// An application created by `rustcms new` (has `[package.metadata.rustcms]`).
pub struct Project {
    /// Directory holding `Cargo.toml`.
    pub root: PathBuf,
    /// Package name.
    pub package: String,
}

impl Project {
    /// Looks for the project in the current directory and its parents.
    pub fn find() -> anyhow::Result<Self> {
        let cwd = std::env::current_dir()?;
        Self::find_from(&cwd)
    }

    pub fn find_from(start: &Path) -> anyhow::Result<Self> {
        for dir in start.ancestors() {
            let manifest = dir.join("Cargo.toml");
            let Ok(contents) = std::fs::read_to_string(&manifest) else { continue };
            if contents.contains("[package.metadata.rustcms]") {
                let package =
                    package_name(&contents).with_context(|| format!("no package name in {}", manifest.display()))?;
                return Ok(Self { root: dir.to_path_buf(), package });
            }
        }
        bail!(
            "not inside a rustcms application (no Cargo.toml with [package.metadata.rustcms]); create one with `rustcms new`"
        )
    }

    /// Runs the application binary with `args` (`cargo run -- <args>`).
    pub fn forward(&self, args: &[String]) -> anyhow::Result<ExitCode> {
        let status = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
            .current_dir(&self.root)
            .args(["run", "--quiet", "--package", &self.package, "--bin", &self.package, "--"])
            .args(args)
            .status()
            .context("cannot run cargo")?;
        Ok(ExitCode::from(status.code().and_then(|c| u8::try_from(c).ok()).unwrap_or(1)))
    }
}

/// `name = "..."` in the `[package]` table.
fn package_name(manifest: &str) -> Option<String> {
    let mut in_package = false;
    for line in manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            in_package = line == "[package]";
        } else if in_package && let Some(value) = line.strip_prefix("name") {
            return Some(value.trim_start().strip_prefix('=')?.trim().trim_matches('"').to_owned());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_package_name() {
        let manifest = "[workspace]\nname = \"x\"\n[package]\nname = \"blog\"\nversion = \"0.1.0\"\n";
        assert_eq!(package_name(manifest).as_deref(), Some("blog"));
    }

    #[test]
    fn finds_project_in_parent() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname = \"blog\"\n\n[package.metadata.rustcms]\n")
            .unwrap();
        std::fs::create_dir_all(dir.path().join("src/models")).unwrap();
        let project = Project::find_from(&dir.path().join("src/models")).unwrap();
        assert_eq!(project.package, "blog");
    }
}
