//! `rustcms docs`: the guide of this version. Release builds embed it
//! (`RUSTCMS_DOCS_DIR` at build time) and open it offline from a cache
//! directory; other builds open the published copy of this version.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use anyhow::Context;

use crate::release;

#[cfg(rustcms_docs)]
static BOOK: include_dir::Dir<'_> = include_dir::include_dir!("$RUSTCMS_DOCS_DIR");

pub fn run(print_path: bool) -> anyhow::Result<ExitCode> {
    let target = match embedded() {
        Some(files) => {
            let dir = cache_dir()?.join("rustcms/docs").join(release::VERSION);
            extract(&dir, &files)?;
            dir.join("index.html").display().to_string()
        }
        None => format!("{}/v/{}/", release::PAGES, release::VERSION),
    };
    if print_path {
        println!("{target}");
        return Ok(ExitCode::SUCCESS);
    }
    println!("Opening {target}");
    let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    let opened = Command::new(opener).arg(&target).status().is_ok_and(|s| s.success());
    if !opened {
        println!("Open it in a browser: {target}");
    }
    Ok(ExitCode::SUCCESS)
}

/// The embedded guide: relative path and contents of every file.
#[cfg(rustcms_docs)]
fn embedded() -> Option<Vec<(PathBuf, &'static [u8])>> {
    fn walk(dir: &'static include_dir::Dir<'static>, out: &mut Vec<(PathBuf, &'static [u8])>) {
        for file in dir.files() {
            out.push((file.path().to_path_buf(), file.contents()));
        }
        for sub in dir.dirs() {
            walk(sub, out);
        }
    }
    let mut files = Vec::new();
    walk(&BOOK, &mut files);
    Some(files)
}

#[cfg(not(rustcms_docs))]
fn embedded() -> Option<Vec<(PathBuf, &'static [u8])>> {
    None
}

/// `~/Library/Caches` on macOS, `$XDG_CACHE_HOME` or `~/.cache` elsewhere.
fn cache_dir() -> anyhow::Result<PathBuf> {
    let home = || std::env::var_os("HOME").map(PathBuf::from).context("HOME is not set");
    if cfg!(target_os = "macos") {
        return Ok(home()?.join("Library/Caches"));
    }
    match std::env::var_os("XDG_CACHE_HOME").filter(|v| !v.is_empty()) {
        Some(dir) => Ok(PathBuf::from(dir)),
        None => Ok(home()?.join(".cache")),
    }
}

/// Writes the guide into `dir` once. Writers take turns (a lock next to it);
/// the first complete guide wins and is never removed; a reader only uses a
/// directory marked complete.
fn extract(dir: &Path, files: &[(PathBuf, &[u8])]) -> anyhow::Result<()> {
    if dir.join(".complete").is_file() {
        return Ok(());
    }
    let parent = dir.parent().context("no parent")?;
    std::fs::create_dir_all(parent)?;
    let name = dir.file_name().context("no name")?.to_string_lossy().into_owned();
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(parent.join(format!(".{name}.lock")))?;
    lock.lock()?;
    // Another writer finished while this one waited.
    if dir.join(".complete").is_file() {
        return Ok(());
    }
    let staging = tempfile::Builder::new().prefix(".docs-").tempdir_in(parent)?;
    for (path, contents) in files {
        // Paths come from the embedded book: relative, no `..`.
        anyhow::ensure!(path.is_relative() && !path.components().any(|c| c.as_os_str() == ".."), "bad path");
        let out = staging.path().join(path);
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&out, contents)?;
    }
    std::fs::write(staging.path().join(".complete"), release::VERSION)?;
    // Under the lock, an existing directory is an interrupted extraction.
    if dir.exists() {
        std::fs::remove_dir_all(dir)?;
    }
    std::fs::rename(staging.keep(), dir).with_context(|| format!("cannot write the guide to {}", dir.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_once_and_completely() {
        let cache = tempfile::tempdir().unwrap();
        let dir = cache.path().join("rustcms/docs/1.0.0");
        let files: Vec<(PathBuf, &[u8])> =
            vec![(PathBuf::from("index.html"), b"<h1>x</h1>"), (PathBuf::from("a/searchindex.js"), b"x")];
        extract(&dir, &files).unwrap();
        assert_eq!(std::fs::read(dir.join("index.html")).unwrap(), b"<h1>x</h1>");
        assert!(dir.join("a/searchindex.js").is_file() && dir.join(".complete").is_file());
        // A second run keeps it; an incomplete directory is replaced.
        extract(&dir, &files).unwrap();
        std::fs::remove_file(dir.join(".complete")).unwrap();
        std::fs::write(dir.join("index.html"), "half").unwrap();
        extract(&dir, &files).unwrap();
        assert_eq!(std::fs::read(dir.join("index.html")).unwrap(), b"<h1>x</h1>");
        let staging = std::fs::read_dir(dir.parent().unwrap())
            .unwrap()
            .filter(|e| e.as_ref().unwrap().file_name().to_string_lossy().starts_with(".docs-"))
            .count();
        assert_eq!(staging, 0, "no staging directory left");
    }

    #[test]
    fn concurrent_readers_get_one_complete_guide() {
        let cache = tempfile::tempdir().unwrap();
        let dir = cache.path().join("rustcms/docs/1.0.0");
        let files: Vec<(PathBuf, &'static [u8])> = (0..50)
            .map(|i| (PathBuf::from(format!("p{i}.html")), b"page".as_slice()))
            .chain([(PathBuf::from("index.html"), b"<h1>x</h1>".as_slice())])
            .collect();
        std::thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(|| extract(&dir, &files).unwrap());
            }
        });
        assert!(dir.join(".complete").is_file() && dir.join("p49.html").is_file());
    }

    #[test]
    fn refuses_paths_outside_the_guide() {
        let cache = tempfile::tempdir().unwrap();
        let files: Vec<(PathBuf, &[u8])> = vec![(PathBuf::from("../evil"), b"x")];
        assert!(extract(&cache.path().join("d"), &files).is_err());
    }
}
