//! `rustcms self-update` replaces a copy of the command installed like
//! `install.sh` does, from a local mirror of a release (no network).
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

const NEW: &str = "v9.9.9";

/// `<root>/bin/rustcms` (a copy of the built command) and `<root>/install.json`.
fn installed(root: &Path) -> PathBuf {
    let bin = root.join("bin/rustcms");
    std::fs::create_dir_all(bin.parent().unwrap()).unwrap();
    std::fs::copy(env!("CARGO_BIN_EXE_rustcms"), &bin).unwrap();
    std::fs::write(root.join("install.json"), r#"{"version":"v0.0.0","installer":"install.sh"}"#).unwrap();
    bin
}

/// The target in `rustcms 0.3.1 (aarch64-apple-darwin)`.
fn target() -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_rustcms")).arg("--version").output().unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    text.split('(').nth(1).unwrap().trim_end().trim_end_matches(')').to_owned()
}

/// A release `NEW` whose "command" is a script printing that version.
fn mirror(dir: &Path, corrupt: bool) {
    mirror_reporting(dir, corrupt, "9.9.9");
}

/// The same, its command printing `reports` as its version.
fn mirror_reporting(dir: &Path, corrupt: bool, reports: &str) {
    let pkg = dir.join("pkg");
    std::fs::create_dir_all(&pkg).unwrap();
    let script = pkg.join("rustcms");
    std::fs::write(&script, format!("#!/bin/sh\necho \"rustcms {reports} ({})\"\n", target())).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let release = dir.join("mirror").join(NEW);
    std::fs::create_dir_all(&release).unwrap();
    let archive = format!("rustcms-{NEW}-{}.tar.gz", target());
    let status = Command::new("tar")
        .arg("-czf")
        .arg(release.join(&archive))
        .arg("-C")
        .arg(&pkg)
        .arg("rustcms")
        .status()
        .unwrap();
    assert!(status.success());
    let mut hash = hex::encode(Sha256::digest(std::fs::read(release.join(&archive)).unwrap()));
    if corrupt {
        hash = "0".repeat(64);
    }
    std::fs::write(release.join("SHA256SUMS"), format!("{hash}  {archive}\n")).unwrap();
    std::fs::write(dir.join("mirror/latest.txt"), format!("{NEW}\n")).unwrap();
}

fn self_update(bin: &Path, dir: &Path) -> std::process::Output {
    Command::new(bin)
        .arg("self-update")
        .env("RUSTCMS_DOWNLOAD_BASE", format!("file://{}", dir.join("mirror").display()))
        .env("RUSTCMS_LATEST_URL", format!("file://{}", dir.join("mirror/latest.txt").display()))
        .output()
        .unwrap()
}

fn version(bin: &Path) -> String {
    String::from_utf8(Command::new(bin).arg("--version").output().unwrap().stdout).unwrap()
}

#[test]
fn updates_to_the_latest_release() {
    let dir = tempfile::tempdir().unwrap();
    let bin = installed(&dir.path().join("root"));
    mirror(dir.path(), false);
    let out = self_update(&bin, dir.path());
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(version(&bin).contains("9.9.9"));
    let record = std::fs::read_to_string(dir.path().join("root/install.json")).unwrap();
    assert!(record.contains(NEW), "{record}");
    let leftovers: Vec<_> = std::fs::read_dir(dir.path().join("root/bin")).unwrap().collect();
    assert_eq!(leftovers.len(), 1, "only the command is left in bin/");
}

#[test]
fn a_corrupted_download_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let bin = installed(&dir.path().join("root"));
    let before = version(&bin);
    mirror(dir.path(), true);
    let out = self_update(&bin, dir.path());
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("checksum mismatch"));
    assert_eq!(version(&bin), before, "the old command still runs");
}

#[test]
fn a_command_installed_another_way_is_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    let bin = installed(&dir.path().join("root"));
    std::fs::remove_file(dir.path().join("root/install.json")).unwrap();
    mirror(dir.path(), false);
    let out = self_update(&bin, dir.path());
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("not installed by the install script"));
}

#[test]
fn a_command_reporting_another_version_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let bin = installed(&dir.path().join("root"));
    let before = version(&bin);
    // 9.9.90 contains "9.9.9": only an exact version is accepted.
    mirror_reporting(dir.path(), false, "9.9.90");
    let out = self_update(&bin, dir.path());
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("nothing was changed"));
    assert_eq!(version(&bin), before);
}
