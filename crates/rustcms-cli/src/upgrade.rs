//! `rustcms upgrade`: moves the application to this command's version, the
//! RustCMS-Core tag and the SeaORM pins that go with it. `Cargo.toml` keeps
//! its comments and features; a failed resolution restores it and the lockfile.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use anyhow::{Context, bail};
use toml_edit::{DocumentMut, Item, Value};

use crate::project::Project;
use crate::release;
use crate::scaffold::SEA_ORM_VERSION;

pub struct Options {
    pub dry_run: bool,
    /// Allows going to an older version.
    pub force: bool,
}

pub fn run(project: &Project, options: &Options) -> anyhow::Result<ExitCode> {
    run_with(project, options, &cargo)
}

/// `cargo`: the command Cargo is run with (tests: offline, empty home).
fn run_with(project: &Project, options: &Options, cargo: &dyn Fn() -> Command) -> anyhow::Result<ExitCode> {
    // An older vendored SDK, whether or not Cargo.toml needs changing.
    if let Some(warning) = crate::react::sdk_warning(&project.root) {
        println!("warning: {warning}");
        for hint in crate::react::older_registrations_hint(&project.root) {
            println!("warning: {hint}");
        }
    }
    let manifest_path = project.root.join("Cargo.toml");
    let before = std::fs::read_to_string(&manifest_path)?;
    let Some(after) = upgraded(&before, &release::tag(), options.force)? else {
        println!("{} already uses RustCMS {}", project.package, release::VERSION);
        return Ok(ExitCode::SUCCESS);
    };
    print_changes(&before, &after);
    if options.dry_run {
        println!("(dry run: nothing written)");
        return Ok(ExitCode::SUCCESS);
    }
    let lock_path = lockfile(&project.root, cargo)?;
    // Only a missing lockfile is "none": anything else stops before a change.
    let lock_before = match std::fs::read(&lock_path) {
        Ok(lock) => Some(lock),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => return Err(err).with_context(|| format!("cannot read {}", lock_path.display())),
    };
    std::fs::write(&manifest_path, &after)?;
    if let Err(err) = update_lockfile(&project.root, cargo) {
        std::fs::write(&manifest_path, &before)?;
        match &lock_before {
            Some(lock) => std::fs::write(&lock_path, lock)?,
            None => std::fs::remove_file(&lock_path).or_else(ignore_missing)?,
        }
        return Err(err.context("Cargo.toml and Cargo.lock were restored"));
    }
    println!("\nUpgraded {} to RustCMS {}. Next:", project.package, release::VERSION);
    println!("  read {}/v/{}/upgrade.html", release::PAGES, release::VERSION);
    println!("  rustcms migrate && cargo test");
    println!("  commit Cargo.toml and Cargo.lock together");
    Ok(ExitCode::SUCCESS)
}

fn ignore_missing(err: std::io::Error) -> std::io::Result<()> {
    if err.kind() == std::io::ErrorKind::NotFound { Ok(()) } else { Err(err) }
}

/// The manifest moved to `tag`, or `None` when it is already there.
pub fn upgraded(manifest: &str, tag: &str, force: bool) -> anyhow::Result<Option<String>> {
    let target = release::parse_tag(tag)?;
    let mut doc: DocumentMut = manifest.parse().context("Cargo.toml is not valid TOML")?;
    let current = current_version(&doc)?;
    if let Some(current) = &current
        && *current > target
        && !force
    {
        bail!(
            "the application uses RustCMS {current}, newer than this command ({target}): update the command, or pass --force"
        );
    }
    for (section, krate) in [("dependencies", "rustcms"), ("dev-dependencies", "rustcms-testing")] {
        let Some(dep) = doc.get_mut(section).and_then(|s| s.get_mut(krate)) else {
            if krate == "rustcms" {
                bail!("Cargo.toml has no `rustcms` dependency");
            }
            continue;
        };
        point_at_tag(dep, krate, tag)?;
    }
    for krate in ["sea-orm", "sea-orm-migration"] {
        if let Some(dep) = doc.get_mut("dependencies").and_then(|s| s.get_mut(krate)) {
            pin(dep, &format!("={SEA_ORM_VERSION}"));
        }
    }
    let metadata = doc
        .get_mut("package")
        .and_then(|p| p.get_mut("metadata"))
        .and_then(|m| m.get_mut("rustcms"))
        .and_then(Item::as_table_like_mut)
        .context("Cargo.toml has no [package.metadata.rustcms]")?;
    set(metadata, "version", target.to_string().into());
    // Every edit is checked and made first: an application already on the
    // tag but not fully (its tests' dependency, the pins) is completed too.
    let after = doc.to_string();
    Ok((after != manifest).then_some(after))
}

/// The RustCMS version the application is on: its tag, else its metadata.
fn current_version(doc: &DocumentMut) -> anyhow::Result<Option<semver::Version>> {
    let tag = doc.get("dependencies").and_then(|d| d.get("rustcms")).and_then(|d| d.get("tag")).and_then(Item::as_str);
    if let Some(tag) = tag {
        return Ok(Some(release::parse_tag(tag)?));
    }
    let metadata = doc
        .get("package")
        .and_then(|p| p.get("metadata"))
        .and_then(|m| m.get("rustcms"))
        .and_then(|r| r.get("version"));
    Ok(metadata.and_then(Item::as_str).and_then(|v| semver::Version::parse(v).ok()))
}

/// `{ git = Core, tag }`, keeping every other key (features, optional, …).
fn point_at_tag(dep: &mut Item, krate: &str, tag: &str) -> anyhow::Result<()> {
    // `rustcms = "0.3.0"` (before 0.3.1): a table from now on.
    if dep.is_str() {
        let mut table = toml_edit::InlineTable::new();
        table.insert("git", release::CORE_REPOSITORY.into());
        table.insert("tag", tag.into());
        let mut value = Value::InlineTable(table);
        if let Some(old) = dep.as_value() {
            *value.decor_mut() = old.decor().clone();
        }
        *dep = Item::Value(value);
        return Ok(());
    }
    let table = dep.as_table_like_mut().with_context(|| format!("`{krate}` is not a dependency table"))?;
    if table.contains_key("path") {
        bail!("`{krate}` uses a local checkout (`path`): update that checkout instead");
    }
    if table.contains_key("workspace") {
        bail!("`{krate}` is inherited from the workspace: upgrade it in the workspace's Cargo.toml");
    }
    if table.contains_key("package") {
        bail!("`{krate}` is renamed (`package = …`): upgrade it by hand");
    }
    if let Some(git) = table.get("git").and_then(Item::as_str)
        && git.trim_end_matches(".git").trim_end_matches('/') != release::CORE_REPOSITORY
    {
        bail!("`{krate}` comes from {git}, not {}: upgrade it by hand", release::CORE_REPOSITORY);
    }
    for key in ["version", "branch", "rev", "registry"] {
        table.remove(key);
    }
    set(table, "git", release::CORE_REPOSITORY.into());
    set(table, "tag", tag.into());
    Ok(())
}

/// `key = value`, keeping the comments and spacing around an existing value.
fn set(table: &mut dyn toml_edit::TableLike, key: &str, value: Value) {
    match table.get_mut(key).and_then(Item::as_value_mut) {
        Some(old) => {
            let decor = old.decor().clone();
            *old = value;
            *old.decor_mut() = decor;
        }
        None => {
            table.insert(key, Item::Value(value));
        }
    }
}

/// The version requirement of a dependency (a string or a table's `version`).
fn pin(dep: &mut Item, requirement: &str) {
    if let Some(old) = dep.as_value_mut().filter(|v| v.is_str()) {
        let decor = old.decor().clone();
        *old = requirement.into();
        *old.decor_mut() = decor;
    } else if let Some(table) = dep.as_table_like_mut()
        && table.contains_key("version")
    {
        set(table, "version", requirement.into());
    }
}

fn print_changes(before: &str, after: &str) {
    println!("Cargo.toml:");
    let old: Vec<&str> = before.lines().collect();
    let new: Vec<&str> = after.lines().collect();
    for line in old.iter().filter(|l| !new.contains(l)) {
        println!("  - {line}");
    }
    for line in new.iter().filter(|l| !old.contains(l)) {
        println!("  + {line}");
    }
}

/// The lockfile Cargo uses for the application (a parent workspace's, if any).
fn lockfile(root: &Path, cargo: &dyn Fn() -> Command) -> anyhow::Result<PathBuf> {
    let output =
        cargo().args(["locate-project", "--workspace", "--message-format", "plain"]).current_dir(root).output()?;
    if !output.status.success() {
        bail!("cargo locate-project failed: {}", String::from_utf8_lossy(&output.stderr).trim());
    }
    let manifest = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    Ok(manifest.with_file_name("Cargo.lock"))
}

fn update_lockfile(root: &Path, cargo: &dyn Fn() -> Command) -> anyhow::Result<()> {
    println!("Resolving dependencies…");
    let output = cargo()
        .args(["update", "-p", "rustcms", "-p", "sea-orm", "-p", "sea-orm-migration"])
        .current_dir(root)
        .output()
        .context("cannot run cargo")?;
    if !output.status.success() {
        bail!("cargo update failed:\n{}", String::from_utf8_lossy(&output.stderr).trim());
    }
    Ok(())
}

fn cargo() -> Command {
    Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CORE: &str = release::CORE_REPOSITORY;

    fn app(rustcms: &str, testing: &str) -> String {
        format!(
            r#"[package]
name = "blog"
version = "0.1.0"

[package.metadata.rustcms]
version = "0.3.1"

[dependencies]
# The framework.
rustcms = {rustcms}
sea-orm = {{ version = "=2.0.0", default-features = false, features = ["macros"] }}
sea-orm-migration = {{ version = "=2.0.0", default-features = false }}

[dev-dependencies]
rustcms-testing = {testing}
"#
        )
    }

    fn git(tag: &str) -> String {
        format!(r#"{{ git = "{CORE}", tag = "{tag}", features = ["redis"] }}"#)
    }

    #[test]
    fn moves_the_tag_and_the_pins_and_keeps_the_rest() {
        let before = app(&git("v0.3.1"), &format!(r#"{{ git = "{CORE}", tag = "v0.3.1" }}"#));
        let after = upgraded(&before, "v9.0.0", false).unwrap().unwrap();
        let doc: toml::Table = after.parse().unwrap();
        assert_eq!(doc["dependencies"]["rustcms"]["tag"].as_str(), Some("v9.0.0"));
        assert_eq!(doc["dependencies"]["rustcms"]["features"][0].as_str(), Some("redis"));
        assert_eq!(doc["dev-dependencies"]["rustcms-testing"]["tag"].as_str(), Some("v9.0.0"));
        let pin = format!("={SEA_ORM_VERSION}");
        assert_eq!(doc["dependencies"]["sea-orm"]["version"].as_str(), Some(pin.as_str()));
        assert_eq!(doc["dependencies"]["sea-orm"]["features"][0].as_str(), Some("macros"));
        assert_eq!(doc["dependencies"]["sea-orm-migration"]["version"].as_str(), Some(pin.as_str()));
        assert_eq!(doc["package"]["metadata"]["rustcms"]["version"].as_str(), Some("9.0.0"));
        assert!(after.contains("# The framework."), "comments kept");
        assert_eq!(upgraded(&after, "v9.0.0", false).unwrap(), None, "already there");
    }

    #[test]
    fn trailing_comments_stay() {
        let before = app(&git("v0.3.1"), r#""0.3.0" # tests"#)
            .replace("version = \"0.3.1\"\n\n[dependencies]", "version = \"0.3.1\" # app version\n\n[dependencies]");
        let after = upgraded(&before, "v9.0.0", false).unwrap().unwrap();
        assert!(after.contains("# tests"), "{after}");
        assert!(after.contains("\"9.0.0\" # app version"), "{after}");
    }

    #[test]
    fn an_application_half_upgraded_is_completed() {
        // On the tag already, but not its tests' dependency nor the pins.
        let before = app(&git("v9.0.0"), &format!(r#"{{ git = "{CORE}", tag = "v0.3.1" }}"#));
        let after = upgraded(&before, "v9.0.0", false).unwrap().expect("completed");
        let doc: toml::Table = after.parse().unwrap();
        assert_eq!(doc["dev-dependencies"]["rustcms-testing"]["tag"].as_str(), Some("v9.0.0"));
        assert_eq!(doc["package"]["metadata"]["rustcms"]["version"].as_str(), Some("9.0.0"));
        // And a dependency it cannot upgrade is refused even on the right tag.
        let path = app(&git("v9.0.0"), r#"{ path = "../RustCMS/crates/rustcms-testing" }"#);
        assert!(upgraded(&path, "v9.0.0", false).is_err());
    }

    #[test]
    fn an_application_from_before_the_git_tags_is_moved_to_one() {
        let after = upgraded(&app(r#""0.3.0""#, r#""0.3.0""#), "v0.4.0", false).unwrap().unwrap();
        let doc: toml::Table = after.parse().unwrap();
        assert_eq!(doc["dependencies"]["rustcms"]["git"].as_str(), Some(CORE));
        assert_eq!(doc["dependencies"]["rustcms"]["tag"].as_str(), Some("v0.4.0"));
        assert_eq!(doc["dev-dependencies"]["rustcms-testing"]["tag"].as_str(), Some("v0.4.0"));
    }

    #[test]
    fn refuses_what_it_cannot_upgrade_safely() {
        let testing = format!(r#"{{ git = "{CORE}", tag = "v0.3.1" }}"#);
        for (rustcms, why) in [
            (r#"{ path = "../RustCMS/crates/rustcms" }"#.to_owned(), "local checkout"),
            ("{ workspace = true }".to_owned(), "inherited"),
            (format!(r#"{{ git = "{CORE}", tag = "v0.3.1", package = "other" }}"#), "renamed"),
            (r#"{ git = "https://example.com/fork", tag = "v0.3.1" }"#.to_owned(), "example.com"),
        ] {
            let err = upgraded(&app(&rustcms, &testing), "v0.4.0", false).unwrap_err().to_string();
            assert!(err.contains(why), "{err}");
        }
        let newer = app(&git("v0.5.0"), &testing);
        assert!(upgraded(&newer, "v0.4.0", false).unwrap_err().to_string().contains("newer"));
        assert!(upgraded(&newer, "v0.4.0", true).unwrap().is_some(), "--force goes back");
    }

    /// A broken resolution leaves the application as it was.
    #[test]
    fn a_failed_resolution_restores_the_manifest_and_the_lockfile() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = app(&git("v0.3.0"), &format!(r#"{{ git = "{CORE}", tag = "v0.3.0" }}"#))
            + "\n[workspace]\n\n[[bin]]\nname = \"blog\"\npath = \"main.rs\"\n";
        std::fs::write(dir.path().join("Cargo.toml"), &manifest).unwrap();
        std::fs::write(dir.path().join("main.rs"), "fn main() {}\n").unwrap();
        std::fs::write(dir.path().join("Cargo.lock"), "# lock\n").unwrap();
        let project = Project { root: dir.path().to_path_buf(), package: "blog".into() };
        // An offline Cargo with an empty home cannot fetch the tag: the update fails.
        let home = tempfile::tempdir().unwrap();
        let offline = || {
            let mut command = cargo();
            command.env("CARGO_HOME", home.path()).env("CARGO_NET_OFFLINE", "true");
            command
        };
        let err = run_with(&project, &Options { dry_run: false, force: false }, &offline).unwrap_err();
        assert!(err.to_string().contains("restored"), "{err:#}");
        assert_eq!(std::fs::read_to_string(dir.path().join("Cargo.toml")).unwrap(), manifest);
        assert_eq!(std::fs::read_to_string(dir.path().join("Cargo.lock")).unwrap(), "# lock\n");
    }
}
