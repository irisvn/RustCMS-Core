//! `rustcms doctor`: what building an application needs, checked on this
//! machine, with what to do about each problem.

use std::fmt;
use std::path::Path;
use std::process::{Command, ExitCode, Stdio};

use crate::project::Project;
use crate::release::{self, Source};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Ok,
    Warn,
    Fail,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Check {
    pub status: Status,
    pub name: &'static str,
    pub detail: String,
    /// What to do, for a warning or a failure.
    pub fix: Option<String>,
}

impl Check {
    fn ok(name: &'static str, detail: impl Into<String>) -> Self {
        Self { status: Status::Ok, name, detail: detail.into(), fix: None }
    }
    fn warn(name: &'static str, detail: impl Into<String>, fix: impl Into<String>) -> Self {
        Self { status: Status::Warn, name, detail: detail.into(), fix: Some(fix.into()) }
    }
    fn fail(name: &'static str, detail: impl Into<String>, fix: impl Into<String>) -> Self {
        Self { status: Status::Fail, name, detail: detail.into(), fix: Some(fix.into()) }
    }
}

impl fmt::Display for Check {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mark = match self.status {
            Status::Ok => "ok  ",
            Status::Warn => "warn",
            Status::Fail => "FAIL",
        };
        write!(f, "[{mark}] {}: {}", self.name, self.detail)?;
        if let Some(fix) = &self.fix {
            write!(f, "\n       → {fix}")?;
        }
        Ok(())
    }
}

pub fn run() -> anyhow::Result<ExitCode> {
    let cwd = std::env::current_dir()?;
    let project = Project::find_from(&cwd).ok();
    // In the application, rustup applies its toolchain file.
    let dir = project.as_ref().map_or(cwd.as_path(), |p| p.root.as_path());
    let macos = cfg!(target_os = "macos");
    let mut checks = vec![
        Check::ok("rustcms", format!("{} ({})", release::VERSION, release::TARGET)),
        rustc(stdout(dir, "rustc", &["--version"]).as_deref(), release::MSRV),
        cargo(stdout(dir, "cargo", &["--version"]).as_deref()),
        c_compiler(compiles_c(dir), macos),
        git(stdout(dir, "git", &["--version"]).as_deref()),
    ];
    if checks.last().is_some_and(|c| c.status == Status::Ok) {
        checks.push(core_tag(ls_remote(dir)));
    }
    checks.push(curl(stdout(dir, "curl", &["--version"]).as_deref()));
    checks.push(latest(Source::from_env().latest().ok().as_deref()));
    if let Some(project) = &project {
        checks.push(application(project));
    }
    for check in &checks {
        println!("{check}");
    }
    let failed = checks.iter().filter(|c| c.status == Status::Fail).count();
    if failed > 0 {
        println!("\n{failed} problem(s) to fix before building an application.");
        return Ok(ExitCode::FAILURE);
    }
    println!("\nReady.");
    Ok(ExitCode::SUCCESS)
}

/// The first line a command prints, if it runs and succeeds.
fn stdout(dir: &Path, program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).current_dir(dir).stderr(Stdio::null()).output().ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    output.status.success().then(|| text.lines().next().unwrap_or_default().trim().to_owned())
}

/// `rustc 1.94.0 (…)` → 1.94.0.
fn version_of(line: &str) -> Option<semver::Version> {
    let word = line.split_whitespace().nth(1)?;
    let plain = word.split(['-', '+']).next()?;
    semver::Version::parse(plain).ok()
}

fn rustc(line: Option<&str>, msrv: &str) -> Check {
    const FIX: &str = "install Rust from https://rustup.rs (then `rustup update stable`)";
    let Some(line) = line else { return Check::fail("rust", "rustc not found", FIX) };
    let minimum = semver::Version::parse(&format!("{msrv}.0")).or_else(|_| semver::Version::parse(msrv));
    match (version_of(line), minimum) {
        (Some(found), Ok(minimum)) if found >= minimum => Check::ok("rust", line),
        (Some(_), Ok(_)) => Check::fail("rust", format!("{line}, {msrv} or newer is needed"), "rustup update stable"),
        _ => Check::warn("rust", format!("cannot read the version: {line}"), FIX),
    }
}

fn cargo(line: Option<&str>) -> Check {
    match line {
        Some(line) => Check::ok("cargo", line),
        None => Check::fail("cargo", "cargo not found", "install Rust from https://rustup.rs"),
    }
}

/// Whether a C file compiles and links here (SQLite and TLS build C code).
fn compiles_c(dir: &Path) -> Option<bool> {
    let work = tempfile::tempdir().ok()?;
    let source = work.path().join("probe.c");
    std::fs::write(&source, "int main(void) { return 0; }\n").ok()?;
    let compiler = std::env::var("CC").unwrap_or_else(|_| "cc".into());
    // Not found at all: None; found but failing: Some(false).
    let status = Command::new(compiler)
        .arg(&source)
        .arg("-o")
        .arg(work.path().join("probe"))
        .current_dir(dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .ok()?;
    Some(status.success())
}

fn c_compiler(compiles: Option<bool>, macos: bool) -> Check {
    let fix = if macos {
        "install the Xcode Command Line Tools: xcode-select --install"
    } else {
        "install a C compiler: sudo apt install build-essential (Debian/Ubuntu), or gcc/clang"
    };
    match compiles {
        Some(true) => Check::ok("c compiler", "compiles and links C"),
        Some(false) => Check::fail("c compiler", "cc is installed but cannot build a C program", fix),
        None => Check::fail("c compiler", "cc not found", fix),
    }
}

fn git(line: Option<&str>) -> Check {
    match line {
        Some(line) => Check::ok("git", line),
        // Cargo fetches Git dependencies on its own.
        None => Check::warn("git", "not found", "optional: install git to check access to RustCMS-Core here"),
    }
}

/// The exit code of `git ls-remote --exit-code <Core> refs/tags/<tag>`.
fn ls_remote(dir: &Path) -> Option<i32> {
    let status = Command::new("git")
        .args(["ls-remote", "--exit-code", release::CORE_REPOSITORY])
        .arg(format!("refs/tags/{}", release::tag()))
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .ok()?;
    status.code()
}

fn core_tag(code: Option<i32>) -> Check {
    let detail = format!("{} {}", release::CORE_REPOSITORY, release::tag());
    match code {
        Some(0) => Check::ok("rustcms-core", detail),
        // 2: reached, but no such tag.
        Some(2) => Check::fail("rustcms-core", format!("{detail}: no such tag"), "rustcms self-update"),
        _ => Check::warn(
            "rustcms-core",
            format!("{detail}: GitHub not reachable"),
            "the first build fetches RustCMS-Core from GitHub: check the network or proxy",
        ),
    }
}

fn curl(line: Option<&str>) -> Check {
    match line {
        Some(line) => Check::ok("curl", line.split(" (").next().unwrap_or(line)),
        None => Check::warn("curl", "not found", "install curl (`rustcms self-update` downloads with it)"),
    }
}

fn latest(tag: Option<&str>) -> Check {
    let Some(tag) = tag else {
        return Check::warn("latest release", "unknown (offline?)", format!("see {}", release::PAGES));
    };
    let current = release::parse_tag(&release::tag()).ok();
    match (release::parse_tag(tag).ok(), current) {
        (Some(latest), Some(current)) if latest > current => {
            Check::warn("latest release", format!("{tag} is available"), "rustcms self-update")
        }
        _ => Check::ok("latest release", format!("{tag} (this is up to date)")),
    }
}

fn application(project: &Project) -> Check {
    let manifest = std::fs::read_to_string(project.root.join("Cargo.toml")).unwrap_or_default();
    let tag = manifest
        .parse::<toml::Table>()
        .ok()
        .and_then(|doc| doc.get("dependencies")?.get("rustcms")?.get("tag")?.as_str().map(str::to_owned));
    application_tag(&project.package, tag.as_deref())
}

fn application_tag(package: &str, tag: Option<&str>) -> Check {
    let current = release::tag();
    match tag {
        Some(tag) if tag == current => Check::ok("application", format!("{package} on {tag}")),
        Some(tag) => match (release::parse_tag(tag), release::parse_tag(&current)) {
            (Ok(app), Ok(cli)) if app < cli => {
                Check::warn("application", format!("{package} on {tag}, this command is {current}"), "rustcms upgrade")
            }
            _ => Check::warn(
                "application",
                format!("{package} on {tag}, this command is {current}"),
                "rustcms self-update --version ".to_owned() + tag,
            ),
        },
        None => Check::warn(
            "application",
            format!("{package} does not depend on a RustCMS-Core tag"),
            "rustcms upgrade (a local `path` stays as it is)",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_version_against_the_minimum() {
        assert_eq!(rustc(Some("rustc 1.94.0 (4d91de4e4 2026-08-01)"), "1.94").status, Status::Ok);
        assert_eq!(rustc(Some("rustc 1.99.1-nightly (abc 2026-10-01)"), "1.94").status, Status::Ok);
        assert_eq!(rustc(Some("rustc 1.93.1 (x)"), "1.94").status, Status::Fail);
        assert_eq!(rustc(None, "1.94").status, Status::Fail);
        assert_eq!(rustc(Some("garbage"), "1.94").status, Status::Warn);
    }

    #[test]
    fn c_compiler_and_tools() {
        assert_eq!(c_compiler(Some(true), true).status, Status::Ok);
        let missing = c_compiler(None, true);
        assert_eq!(missing.status, Status::Fail);
        assert!(missing.fix.unwrap().contains("xcode-select"));
        assert!(c_compiler(Some(false), false).fix.unwrap().contains("build-essential"));
        assert_eq!(cargo(None).status, Status::Fail);
        assert_eq!(git(None).status, Status::Warn, "Cargo has its own git");
        assert_eq!(curl(None).status, Status::Warn);
    }

    #[test]
    fn access_to_the_core_tag() {
        assert_eq!(core_tag(Some(0)).status, Status::Ok);
        assert_eq!(core_tag(Some(2)).status, Status::Fail, "no such tag");
        assert_eq!(core_tag(Some(128)).status, Status::Warn, "network");
        assert_eq!(core_tag(None).status, Status::Warn);
    }

    #[test]
    fn newer_releases_and_the_application_version() {
        assert_eq!(latest(None).status, Status::Warn, "offline is not a failure");
        assert_eq!(latest(Some(&release::tag())).status, Status::Ok);
        assert_eq!(latest(Some("v999.0.0")).status, Status::Warn);
        assert_eq!(application_tag("blog", Some(&release::tag())).status, Status::Ok);
        let old = application_tag("blog", Some("v0.0.1"));
        assert_eq!(old.fix.as_deref(), Some("rustcms upgrade"));
        let newer = application_tag("blog", Some("v999.0.0"));
        assert!(newer.fix.unwrap().contains("self-update --version v999.0.0"));
        assert_eq!(application_tag("blog", None).status, Status::Warn);
    }
}
