//! `rustcms self-update`: replaces this command with another release, when
//! `install.sh` installed it (`<root>/bin/rustcms` next to `<root>/install.json`).

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::{Duration, SystemTime};

use anyhow::{Context, bail};

use crate::release::{self, Source};

/// An update lock older than this was left by a killed update.
const STALE_LOCK: Duration = Duration::from_secs(600);

pub fn run(version: Option<&str>) -> anyhow::Result<ExitCode> {
    let exe = std::env::current_exe()?.canonicalize()?;
    let root = installed_root(&exe)?;
    let source = Source::from_env();
    let tag = match version {
        Some(tag) => {
            release::parse_tag(tag)?;
            tag.to_owned()
        }
        None => source.latest()?,
    };
    let current = release::parse_tag(&release::tag())?;
    let wanted = release::parse_tag(&tag)?;
    if wanted == current {
        println!("rustcms {} is up to date", release::VERSION);
        return Ok(ExitCode::SUCCESS);
    }
    // Going back is only done when asked for by version.
    if wanted < current && version.is_none() {
        println!("rustcms {} is newer than the latest release ({tag})", release::VERSION);
        return Ok(ExitCode::SUCCESS);
    }
    let _lock = UpdateLock::acquire(&root)?;
    println!("updating rustcms {} to {tag} ({})", release::VERSION, release::TARGET);
    install(&source, &tag, &exe)?;
    record(&root, &tag)?;
    println!("updated: {}", exe.display());
    Ok(ExitCode::SUCCESS)
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
    let version = Command::new(&new).arg("--version").output().context("the downloaded rustcms does not run")?;
    let printed = String::from_utf8_lossy(&version.stdout);
    if !version.status.success() || !printed.contains(tag.trim_start_matches('v')) {
        bail!("the downloaded rustcms does not report {tag} ({})", printed.trim());
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

/// `install.json` follows the installed version.
fn record(root: &Path, tag: &str) -> anyhow::Result<()> {
    let path = root.join("install.json");
    let mut record: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(&std::fs::read_to_string(&path)?).unwrap_or_default();
    record.insert("version".into(), tag.into());
    record.insert("target".into(), release::TARGET.into());
    let staged = root.join(".install.json.new");
    std::fs::write(&staged, serde_json::to_string(&record)? + "\n")?;
    std::fs::rename(staged, path)?;
    Ok(())
}

/// One update at a time (`<root>/update.lock`), removed when done.
struct UpdateLock(PathBuf);

impl UpdateLock {
    fn acquire(root: &Path) -> anyhow::Result<Self> {
        let path = root.join("update.lock");
        for _ in 0..2 {
            match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(_) => return Ok(Self(path)),
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                    let age = std::fs::metadata(&path)?.modified()?;
                    if SystemTime::now().duration_since(age).unwrap_or_default() < STALE_LOCK {
                        bail!("another update is running ({} exists)", path.display());
                    }
                    std::fs::remove_file(&path).ok();
                }
                Err(err) => return Err(err.into()),
            }
        }
        bail!("cannot lock {}", path.display())
    }
}

impl Drop for UpdateLock {
    fn drop(&mut self) {
        std::fs::remove_file(&self.0).ok();
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
        assert!(UpdateLock::acquire(dir.path()).is_err());
        drop(lock);
        assert!(!dir.path().join("update.lock").exists());
        assert!(UpdateLock::acquire(dir.path()).is_ok());
    }

    #[test]
    fn records_the_installed_version() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("install.json"), r#"{"version":"v0.1.0","installer":"install.sh"}"#).unwrap();
        record(dir.path(), "v0.2.0").unwrap();
        let json: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.path().join("install.json")).unwrap()).unwrap();
        assert_eq!(json["version"], "v0.2.0");
        assert_eq!(json["installer"], "install.sh");
    }
}
