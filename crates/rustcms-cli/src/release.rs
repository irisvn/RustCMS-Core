//! Releases of the command on irisvn/RustCMS (ADR 0025): versions, downloads,
//! checksums. `doctor`, `self-update` and `docs` share them with `install.sh`
//! (same URLs, same test overrides).

use std::path::Path;
use std::process::Command;

use anyhow::{Context, bail};
use sha2::{Digest, Sha256};

/// This command's version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// The target it was built for (`aarch64-apple-darwin`, …).
pub const TARGET: &str = env!("RUSTCMS_TARGET");
/// Minimum Rust version of applications (the workspace `rust-version`).
pub const MSRV: &str = env!("CARGO_PKG_RUST_VERSION");
/// RustCMS-Core, which applications depend on.
pub const CORE_REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");
const DIST_REPOSITORY: &str = "https://github.com/irisvn/RustCMS";
pub const PAGES: &str = "https://irisvn.github.io/RustCMS";

/// `v0.4.0` for this version.
pub fn tag() -> String {
    format!("v{VERSION}")
}

/// A release tag: `vX.Y.Z` or `vX.Y.Z-rc.N`.
pub fn parse_tag(tag: &str) -> anyhow::Result<semver::Version> {
    let version = tag.strip_prefix('v').and_then(|v| semver::Version::parse(v).ok());
    match version {
        Some(v) if v.build.is_empty() && (v.pre.is_empty() || is_rc(&v.pre)) => Ok(v),
        _ => bail!("not a release tag: `{tag}` (vX.Y.Z or vX.Y.Z-rc.N)"),
    }
}

fn is_rc(pre: &semver::Prerelease) -> bool {
    pre.as_str().strip_prefix("rc.").is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// Where release files come from: GitHub, or a test mirror (`file://…`,
/// as `install.sh` reads them).
pub struct Source {
    download_base: String,
    latest_url: String,
    /// Only a mirror given explicitly may be anything but HTTPS.
    protocols: &'static str,
}

impl Source {
    pub fn from_env() -> Self {
        let base = std::env::var("RUSTCMS_DOWNLOAD_BASE").ok().filter(|v| !v.is_empty());
        let latest = std::env::var("RUSTCMS_LATEST_URL").ok().filter(|v| !v.is_empty());
        let protocols = if base.is_some() || latest.is_some() { "=https,file" } else { "=https" };
        Self {
            download_base: base.unwrap_or_else(|| format!("{DIST_REPOSITORY}/releases/download")),
            latest_url: latest.unwrap_or_else(|| format!("{PAGES}/latest.txt")),
            protocols,
        }
    }

    /// The latest stable release.
    pub fn latest(&self) -> anyhow::Result<String> {
        let body = self.fetch_text(&self.latest_url).context("cannot read the latest release")?;
        let tag = body.trim().to_owned();
        parse_tag(&tag)?;
        Ok(tag)
    }

    /// Downloads a file of release `tag` to `out`.
    pub fn download(&self, tag: &str, name: &str, out: &Path) -> anyhow::Result<()> {
        self.fetch(&format!("{}/{tag}/{name}", self.download_base), out)
    }

    fn fetch_text(&self, url: &str) -> anyhow::Result<String> {
        let dir = tempfile::tempdir()?;
        let out = dir.path().join("body");
        self.fetch(url, &out)?;
        Ok(std::fs::read_to_string(out)?)
    }

    fn fetch(&self, url: &str, out: &Path) -> anyhow::Result<()> {
        let output = Command::new("curl")
            .args(["--proto", self.protocols, "--proto-redir", self.protocols, "--tlsv1.2", "-fsSL"])
            .args(["--retry", "3", "--connect-timeout", "10", "--max-time", "300", "-o"])
            .arg(out)
            .arg(url)
            .output()
            .context("cannot run curl (needed to download releases)")?;
        if !output.status.success() {
            bail!("download failed: {url}: {}", String::from_utf8_lossy(&output.stderr).trim());
        }
        Ok(())
    }
}

/// The release archive of this platform.
pub fn archive_name(tag: &str) -> String {
    format!("rustcms-{tag}-{TARGET}.tar.gz")
}

/// The one checksum `SHA256SUMS` gives for `name`.
pub fn expected_checksum(sums: &str, name: &str) -> anyhow::Result<String> {
    let found: Vec<&str> = sums
        .lines()
        .filter_map(|line| line.split_once(char::is_whitespace))
        .filter(|(_, file)| {
            let file = file.trim_start();
            file == name || file.strip_prefix('*') == Some(name)
        })
        .map(|(hash, _)| hash)
        .collect();
    match found.as_slice() {
        [hash] if hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()) => Ok(hash.to_ascii_lowercase()),
        _ => bail!("SHA256SUMS has no single entry for {name}"),
    }
}

pub fn sha256_file(path: &Path) -> anyhow::Result<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_tags() {
        assert_eq!(parse_tag("v0.4.0").unwrap(), semver::Version::new(0, 4, 0));
        assert!(parse_tag("v0.4.0-rc.2").unwrap() < parse_tag("v0.4.0").unwrap());
        assert!(parse_tag("v0.4.0-rc.2").unwrap() > parse_tag("v0.4.0-rc.1").unwrap());
        for bad in ["0.4.0", "v0.4", "v0.4.0-beta.1", "v0.4.0+x", "v0.4.0-rc.", "v0.4.0;rm", ""] {
            assert!(parse_tag(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn exactly_one_checksum() {
        let a = "a".repeat(64);
        let b = "b".repeat(64);
        let sums = format!("{a}  rustcms-v1.0.0-x.tar.gz\n{b} *rustcms-v1.0.0-y.tar.gz\n");
        assert_eq!(expected_checksum(&sums, "rustcms-v1.0.0-x.tar.gz").unwrap(), a);
        assert_eq!(expected_checksum(&sums, "rustcms-v1.0.0-y.tar.gz").unwrap(), b);
        assert!(expected_checksum(&sums, "rustcms-v1.0.0-z.tar.gz").is_err());
        let twice = format!("{a}  f.tar.gz\n{b}  f.tar.gz\n");
        assert!(expected_checksum(&twice, "f.tar.gz").is_err());
        assert!(expected_checksum("nothex  f.tar.gz\n", "f.tar.gz").is_err());
    }

    #[test]
    fn file_checksum() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("x");
        std::fs::write(&file, b"abc").unwrap();
        assert_eq!(sha256_file(&file).unwrap(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }
}
