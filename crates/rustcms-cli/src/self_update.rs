//! `rustcms self-update`: replaces this command with another release, when
//! `install.sh` installed it (`<root>/bin/rustcms` next to `<root>/install.json`).

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use anyhow::{Context, bail};

use crate::release::{self, Source};

pub fn run(version: Option<&str>) -> anyhow::Result<ExitCode> {
    let exe = std::env::current_exe()?.canonicalize()?;
    let root = installed_root(&exe)?;
    if let Some(tag) = version {
        release::parse_tag(tag)?;
    }
    // Everything below is decided under the lock, from the command on disk:
    // another update may have replaced it since this one started.
    let _lock = UpdateLock::acquire(&root)?;
    let source = Source::from_env();
    let tag = match version {
        Some(tag) => tag.to_owned(),
        None => source.latest()?,
    };
    let (installed, _) = reported(&exe).context("cannot run the installed rustcms")?;
    let wanted = release::parse_tag(&tag)?;
    if wanted == installed {
        println!("rustcms {installed} is up to date");
        return Ok(ExitCode::SUCCESS);
    }
    // Going back is only done when asked for by version.
    if wanted < installed && version.is_none() {
        println!("rustcms {installed} is newer than the latest release ({tag})");
        return Ok(ExitCode::SUCCESS);
    }
    // The record is read and staged before the command changes.
    let record = Record::stage(&root, &tag)?;
    println!("updating rustcms {installed} to {tag} ({})", release::TARGET);
    install(&source, &tag, &exe)?;
    if let Err(err) = record.commit() {
        eprintln!("warning: rustcms is updated, but {} was not: {err:#}", root.join("install.json").display());
    }
    println!("updated: {}", exe.display());
    Ok(ExitCode::SUCCESS)
}

/// Version and target a command prints: `rustcms 0.4.0 (aarch64-apple-darwin)`;
/// up to 0.3.1 only `rustcms 0.3.1` (no target).
fn reported(exe: &Path) -> anyhow::Result<(semver::Version, Option<String>)> {
    let output = Command::new(exe).arg("--version").output()?;
    let text = String::from_utf8_lossy(&output.stdout);
    anyhow::ensure!(output.status.success(), "`{} --version` failed", exe.display());
    parse_reported(text.trim())
}

fn parse_reported(line: &str) -> anyhow::Result<(semver::Version, Option<String>)> {
    let parsed = line.strip_prefix("rustcms ").and_then(|rest| match rest.split_once(" (") {
        Some((version, target)) => {
            Some((semver::Version::parse(version).ok()?, Some(target.strip_suffix(')')?.to_owned())))
        }
        None => Some((semver::Version::parse(rest).ok()?, None)),
    });
    parsed.with_context(|| format!("unexpected `--version` output: {line}"))
}

/// `<root>` when `exe` is `<root>/bin/rustcms` and `<root>/install.json` exists.
fn installed_root(exe: &Path) -> anyhow::Result<PathBuf> {
    let root = exe.parent().filter(|bin| bin.ends_with("bin")).and_then(Path::parent);
    match root {
        Some(root) if root.join("install.json").is_file() => Ok(root.to_path_buf()),
        _ => bail!(
            "{} was not installed by the install script, so it cannot replace itself: \
             update it the way you installed it (Cargo: `cargo install --git {} --tag <version> rustcms-cli`), \
             or install with https://irisvn.github.io/RustCMS/install.sh",
            exe.display(),
            release::CORE_REPOSITORY
        ),
    }
}

/// Downloads, checks, tries and only then replaces `exe` (a rename in its
/// directory: the old command stays in place until the new one is ready).
fn install(source: &Source, tag: &str, exe: &Path) -> anyhow::Result<()> {
    let bin = exe.parent().context("no directory")?;
    let work = tempfile::Builder::new().prefix(".rustcms-update-").tempdir_in(bin)?;
    let archive = release::archive_name(tag);
    let archive_path = work.path().join(&archive);
    source.download(tag, &archive, &archive_path)?;
    source.download(tag, "SHA256SUMS", &work.path().join("SHA256SUMS"))?;
    let sums = std::fs::read_to_string(work.path().join("SHA256SUMS"))?;
    let expected = release::expected_checksum(&sums, &archive)?;
    if release::sha256_file(&archive_path)? != expected {
        bail!("checksum mismatch for {archive}: the download is corrupted; nothing was changed");
    }
    // Only the command is taken from the archive.
    let out = work.path().join("out");
    std::fs::create_dir(&out)?;
    let status = Command::new("tar").arg("-xzf").arg(&archive_path).arg("-C").arg(&out).arg("rustcms").status()?;
    let new = out.join("rustcms");
    if !status.success() || !new.is_file() {
        bail!("{archive} has no rustcms executable");
    }
    set_executable(&new)?;
    let (version, target) = reported(&new).context("the downloaded rustcms does not run")?;
    // Releases up to 0.3.1 do not print their target: the archive named it.
    if version != release::parse_tag(tag)? || target.as_deref().is_some_and(|t| t != release::TARGET) {
        let target = target.as_deref().unwrap_or("?");
        bail!("the downloaded rustcms is {version} ({target}), not {tag} ({}): nothing was changed", release::TARGET);
    }
    std::fs::rename(&new, exe).with_context(|| format!("cannot replace {}", exe.display()))?;
    Ok(())
}

#[cfg(unix)]
fn set_executable(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

/// `install.json` with the new version, written next to it before the
/// command is replaced, renamed over it after.
struct Record {
    staged: PathBuf,
    path: PathBuf,
}

impl Record {
    fn stage(root: &Path, tag: &str) -> anyhow::Result<Self> {
        let path = root.join("install.json");
        let text = std::fs::read_to_string(&path).with_context(|| format!("cannot read {}", path.display()))?;
        let mut record: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(&text).with_context(|| format!("{} is not valid JSON", path.display()))?;
        record.insert("version".into(), tag.into());
        record.insert("target".into(), release::TARGET.into());
        let staged = root.join(".install.json.new");
        std::fs::write(&staged, serde_json::to_string(&record)? + "\n")?;
        Ok(Self { staged, path })
    }

    fn commit(self) -> std::io::Result<()> {
        std::fs::rename(&self.staged, &self.path)
    }
}

/// One update at a time: an exclusive lock on `<root>/update.lock`, held by
/// the process (released when it ends, however it ends).
struct UpdateLock(#[allow(dead_code)] std::fs::File);

impl UpdateLock {
    fn acquire(root: &Path) -> anyhow::Result<Self> {
        let path = root.join("update.lock");
        let file = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(&path)?;
        match file.try_lock() {
            Ok(()) => Ok(Self(file)),
            Err(std::fs::TryLockError::WouldBlock) => bail!("another update is running (it holds {})", path.display()),
            Err(std::fs::TryLockError::Error(err)) => Err(err).context(format!("cannot lock {}", path.display())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_installer_managed_command_updates_itself() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("bin/rustcms");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        assert!(installed_root(&exe).is_err(), "no install.json");
        std::fs::write(dir.path().join("install.json"), "{}").unwrap();
        assert_eq!(installed_root(&exe).unwrap(), dir.path());
        assert!(installed_root(&dir.path().join("rustcms")).is_err(), "not in bin/");
    }

    #[test]
    fn one_update_at_a_time() {
        let dir = tempfile::tempdir().unwrap();
        let lock = UpdateLock::acquire(dir.path()).unwrap();
        let busy = UpdateLock::acquire(dir.path()).err().expect("held by the first");
        assert!(busy.to_string().contains("another update"), "{busy}");
        drop(lock);
        assert!(UpdateLock::acquire(dir.path()).is_ok(), "released with its holder");
    }

    #[test]
    fn records_the_installed_version_only_when_committed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("install.json");
        std::fs::write(&path, r#"{"version":"v0.1.0","installer":"install.sh"}"#).unwrap();
        let record = Record::stage(dir.path(), "v0.2.0").unwrap();
        assert!(std::fs::read_to_string(&path).unwrap().contains("v0.1.0"), "unchanged until committed");
        record.commit().unwrap();
        let json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(json["version"], "v0.2.0");
        assert_eq!(json["installer"], "install.sh");
        std::fs::write(&path, "not json").unwrap();
        assert!(Record::stage(dir.path(), "v0.3.0").is_err(), "a broken record stops the update");
    }

    #[test]
    fn the_reported_version_and_target_are_exact() {
        let (version, target) = parse_reported("rustcms 0.4.0-rc.1 (aarch64-apple-darwin)").unwrap();
        assert_eq!(version, semver::Version::parse("0.4.0-rc.1").unwrap());
        assert_eq!(target.as_deref(), Some("aarch64-apple-darwin"));
        assert_ne!(parse_reported("rustcms 0.4.10 (x)").unwrap().0, release::parse_tag("v0.4.1").unwrap());
        // 0.3.1 and before: no target.
        assert_eq!(parse_reported("rustcms 0.3.1").unwrap(), (semver::Version::new(0, 3, 1), None));
        for bad in ["other 0.4.0 (x)", "rustcms abc (x)", "rustcms 0.4.0 (x", ""] {
            assert!(parse_reported(bad).is_err(), "{bad}");
        }
    }
}
