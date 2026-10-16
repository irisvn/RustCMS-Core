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
            let Ok(doc) = contents.parse::<toml::Table>() else { continue };
            let Some(package) = doc.get("package").and_then(toml::Value::as_table) else { continue };
            if package.get("metadata").and_then(|m| m.get("rustcms")).is_some() {
                let package = package
                    .get("name")
                    .and_then(toml::Value::as_str)
                    .with_context(|| format!("no package name in {}", manifest.display()))?;
                return Ok(Self { root: dir.to_path_buf(), package: package.to_owned() });
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_comment_or_another_table_is_not_an_application() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = "# [package.metadata.rustcms]\n[package]\nname = \"x\"\n";
        std::fs::write(dir.path().join("Cargo.toml"), manifest).unwrap();
        assert!(Project::find_from(dir.path()).is_err());
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
