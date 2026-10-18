//! React pages (host API v2, ADR 0032): the vendored SDK and the
//! `resources/admin` build setup (`make:page --react`, `make:sdk`).

use std::path::Path;

use anyhow::Context;
use include_dir::{Dir, include_dir};
use sha2::{Digest, Sha256};

/// The SDK, a generated copy of `admin-ui/sdk` (checked equal by `just generated`).
pub static SDK: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/templates/sdk");
static SETUP: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/templates/admin");

/// What a page built with this SDK needs from the panel.
pub const REQUIRES: &str = "rustcms::admin::Requires { react: \"19.3\", react_query: \"5.104\", ui: 1 }";

fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// The SDK version this command carries.
pub fn sdk_version() -> &'static str {
    SDK.get_file("VERSION").and_then(|f| f.contents_utf8()).map_or("", str::trim)
}

/// Writes the SDK into `dir` (`resources/admin/sdk`). A file is replaced
/// only if it is new or unchanged since it was installed (by the checksums
/// of `MANIFEST`); edited files are kept and listed.
pub fn install_sdk(dir: &Path) -> anyhow::Result<Vec<String>> {
    let manifest = dir.join("MANIFEST");
    let installed: std::collections::BTreeMap<String, String> = std::fs::read_to_string(&manifest)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.split_once("  ").map(|(h, p)| (p.to_owned(), h.to_owned())))
        .collect();
    let mut kept = Vec::new();
    let mut lines = Vec::new();
    for file in SDK.files() {
        let rel = file.path().to_string_lossy().replace('\\', "/");
        if rel == "MANIFEST" {
            continue;
        }
        let target = dir.join(&rel);
        let fresh = digest(file.contents());
        let edited = match std::fs::read(&target) {
            Ok(current) => installed.get(&rel).is_none_or(|h| *h != digest(&current)) && digest(&current) != fresh,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
            // Unreadable is not unedited: never overwrite what cannot be compared.
            Err(e) => return Err(e).with_context(|| format!("reading {}", target.display())),
        };
        if edited {
            kept.push(rel.clone());
            lines.push(format!("{}  {rel}", installed.get(&rel).cloned().unwrap_or_default()));
            continue;
        }
        std::fs::create_dir_all(target.parent().expect("a file"))?;
        std::fs::write(&target, file.contents()).with_context(|| format!("writing {}", target.display()))?;
        lines.push(format!("{fresh}  {rel}"));
    }
    lines.sort();
    std::fs::write(&manifest, format!("{}\n", lines.join("\n")))?;
    Ok(kept)
}

/// `resources/admin`'s setup (package.json, tsconfig.json, .gitignore, sdk/),
/// written only where missing. Returns whether this is a new setup.
pub fn ensure_setup(admin: &Path) -> anyhow::Result<bool> {
    let new = !admin.join("package.json").exists();
    for (from, to) in
        [("package.json", "package.json"), ("tsconfig.json", "tsconfig.json"), ("gitignore", ".gitignore")]
    {
        let target = admin.join(to);
        if !target.exists() {
            std::fs::create_dir_all(admin)?;
            let contents = SETUP.get_file(from).expect("a template").contents();
            std::fs::write(&target, contents)?;
        }
    }
    if !admin.join("sdk").exists() {
        install_sdk(&admin.join("sdk"))?;
    }
    Ok(new)
}

/// The warning `rustcms upgrade` prints when the app's SDK copy is older.
pub fn sdk_warning(root: &Path) -> Option<String> {
    let version = std::fs::read_to_string(root.join("resources/admin/sdk/VERSION")).ok()?;
    let version = version.trim();
    (version != sdk_version()).then(|| {
        format!(
            "resources/admin/sdk is version {version}, this command carries {}: run `rustcms make:sdk` (edited files are kept), then `npm run build` in resources/admin",
            sdk_version()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sdk_is_the_canonical_copy() {
        let canonical = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../admin-ui/sdk");
        for file in SDK.files() {
            let path = canonical.join(file.path());
            assert_eq!(
                std::fs::read(&path).ok().as_deref(),
                Some(file.contents()),
                "{} differs: run `just generated`",
                path.display()
            );
        }
    }

    #[test]
    fn setup_versions_follow_the_panel() {
        let host: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../admin-ui/package.json"))
                .unwrap(),
        )
        .unwrap();
        let app: serde_json::Value =
            serde_json::from_slice(SETUP.get_file("package.json").unwrap().contents()).unwrap();
        for name in ["react", "@tanstack/react-query", "@types/react", "typescript", "vite"] {
            let host_version = host["dependencies"][name].as_str().or(host["devDependencies"][name].as_str());
            assert_eq!(app["devDependencies"][name].as_str(), host_version, "{name}");
        }
        assert!(REQUIRES.contains("\"19.3\"") && REQUIRES.contains("\"5.104\""));
    }

    #[test]
    fn installing_keeps_edited_files() {
        let dir = tempfile::tempdir().unwrap();
        let sdk = dir.path().join("sdk");
        assert!(install_sdk(&sdk).unwrap().is_empty());
        std::fs::write(sdk.join("index.js"), "// mine").unwrap();
        std::fs::remove_file(sdk.join("ui.js")).unwrap();
        let kept = install_sdk(&sdk).unwrap();
        assert_eq!(kept, ["index.js"]);
        assert_eq!(std::fs::read_to_string(sdk.join("index.js")).unwrap(), "// mine");
        assert!(sdk.join("ui.js").exists(), "a missing file comes back");
        // Unreadable (here a directory in place of the file) stops, nothing overwritten.
        std::fs::remove_file(sdk.join("host.js")).unwrap();
        std::fs::create_dir(sdk.join("host.js")).unwrap();
        assert!(install_sdk(&sdk).is_err());
        assert!(sdk.join("host.js").is_dir());
    }
}
