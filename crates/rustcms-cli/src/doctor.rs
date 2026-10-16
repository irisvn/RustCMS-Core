//! `rustcms doctor`: what building an application needs, checked on this
//! machine, with what to do about each problem.

use std::fmt;
use std::path::Path;
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

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
    let mut failed = 0;
    // Each result as soon as it is known: a slow network shows where it waits.
    let mut report = |check: Check| {
        failed += usize::from(check.status == Status::Fail);
        println!("{check}");
        check.status
    };
    report(Check::ok("rustcms", format!("{} ({})", release::VERSION, release::TARGET)));
    report(toolchain("rust", first_line(dir, "rustc", &["--version"]).as_deref(), release::MSRV));
    report(toolchain("cargo", first_line(dir, "cargo", &["--version"]).as_deref(), release::MSRV));
    // Without the Command Line Tools, macOS's `cc` and `git` open an install
    // dialog: they are not run then.
    let tools = !cfg!(target_os = "macos") || developer_tools(dir);
    report(c_compiler(if tools { compiles_c(dir) } else { None }, cfg!(target_os = "macos")));
    let git_found = if tools { first_line(dir, "git", &["--version"]) } else { None };
    if report(git(git_found.as_deref())) == Status::Ok {
        report(core_tag(ls_remote(dir)));
    }
    report(curl(first_line(dir, "curl", &["--version"]).as_deref()));
    report(latest(Source::from_env().latest_quick().ok().as_deref()));
    if let Some(project) = &project {
        report(application(project));
    }
    if failed > 0 {
        println!("\n{failed} problem(s) to fix before building an application.");
        return Ok(ExitCode::FAILURE);
    }
    println!("\nReady.");
    Ok(ExitCode::SUCCESS)
}

/// How long one probe may take.
const PROBE: Duration = Duration::from_secs(20);

/// Runs `command` for at most `limit`; `None` if it cannot start or is too slow.
fn bounded(command: &mut Command, limit: Duration) -> Option<std::process::Output> {
    let mut child = command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let deadline = Instant::now() + limit;
    loop {
        if child.try_wait().ok()?.is_some() {
            return child.wait_with_output().ok();
        }
        if Instant::now() >= deadline {
            child.kill().ok();
            child.wait().ok();
            return None;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The first line a command prints, if it runs and succeeds in time.
fn first_line(dir: &Path, program: &str, args: &[&str]) -> Option<String> {
    let output = bounded(Command::new(program).args(args).current_dir(dir), PROBE)?;
    let text = String::from_utf8_lossy(&output.stdout);
    output.status.success().then(|| text.lines().next().unwrap_or_default().trim().to_owned())
}

/// The Command Line Tools (or Xcode) are installed: `xcode-select -p` only
/// prints the selected directory, it never offers to install.
fn developer_tools(dir: &Path) -> bool {
    bounded(Command::new("xcode-select").arg("-p").current_dir(dir), PROBE).is_some_and(|o| o.status.success())
}

/// `rustc 1.94.0 (…)` → 1.94.0.
fn version_of(line: &str) -> Option<semver::Version> {
    let word = line.split_whitespace().nth(1)?;
    let plain = word.split(['-', '+']).next()?;
    semver::Version::parse(plain).ok()
}

/// `rustc` or `cargo` against the minimum Rust version.
fn toolchain(name: &'static str, line: Option<&str>, msrv: &str) -> Check {
    const FIX: &str = "install Rust from https://rustup.rs (then `rustup update stable`)";
    let Some(line) = line else { return Check::fail(name, format!("{name} not found"), FIX) };
    let minimum = semver::Version::parse(&format!("{msrv}.0")).or_else(|_| semver::Version::parse(msrv));
    match (version_of(line), minimum) {
        (Some(found), Ok(minimum)) if found >= minimum => Check::ok(name, line),
        (Some(_), Ok(_)) => Check::fail(name, format!("{line}, {msrv} or newer is needed"), "rustup update stable"),
        _ => Check::warn(name, format!("cannot read the version: {line}"), FIX),
    }
}

/// Whether a C file compiles and links here (SQLite and TLS build C code).
fn compiles_c(dir: &Path) -> Option<bool> {
    let work = tempfile::tempdir().ok()?;
    let source = work.path().join("probe.c");
    std::fs::write(&source, "int main(void) { return 0; }\n").ok()?;
    let compiler = std::env::var("CC").unwrap_or_else(|_| "cc".into());
    // Not found at all: None; found but failing: Some(false).
    let output = bounded(
        Command::new(compiler).arg(&source).arg("-o").arg(work.path().join("probe")).current_dir(dir),
        Duration::from_secs(60),
    )?;
    Some(output.status.success())
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
/// Never asks for credentials (the repository is public): no prompt, no
/// askpass program, no credential helper.
fn ls_remote(dir: &Path) -> Option<i32> {
    let mut command = Command::new("git");
    command
        .args(["-c", "credential.helper=", "-c", "core.askPass=", "ls-remote", "--exit-code", release::CORE_REPOSITORY])
        .arg(format!("refs/tags/{}", release::tag()))
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ASKPASS", "true")
        .env("SSH_ASKPASS", "true")
        .env_remove("GIT_SSH_COMMAND");
    bounded(&mut command, PROBE)?.status.code()
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
    fn rust_and_cargo_against_the_minimum() {
        assert_eq!(toolchain("rust", Some("rustc 1.94.0 (4d91de4e4 2026-08-01)"), "1.94").status, Status::Ok);
        assert_eq!(toolchain("rust", Some("rustc 1.99.1-nightly (abc 2026-10-01)"), "1.94").status, Status::Ok);
        assert_eq!(toolchain("rust", Some("rustc 1.93.1 (x)"), "1.94").status, Status::Fail);
        assert_eq!(toolchain("cargo", Some("cargo 1.93.0 (x 2026-01-01)"), "1.94").status, Status::Fail);
        assert_eq!(toolchain("cargo", None, "1.94").status, Status::Fail);
        assert_eq!(toolchain("rust", Some("garbage"), "1.94").status, Status::Warn);
    }

    #[test]
    fn a_slow_probe_is_given_up() {
        let started = Instant::now();
        assert!(bounded(Command::new("sleep").arg("5"), Duration::from_millis(200)).is_none());
        assert!(started.elapsed() < Duration::from_secs(3));
        let done = bounded(Command::new("echo").arg("hi"), PROBE).unwrap();
        assert_eq!(String::from_utf8_lossy(&done.stdout).trim(), "hi");
    }

    #[test]
    fn c_compiler_and_tools() {
        assert_eq!(c_compiler(Some(true), true).status, Status::Ok);
        let missing = c_compiler(None, true);
        assert_eq!(missing.status, Status::Fail);
        assert!(missing.fix.unwrap().contains("xcode-select"));
        assert!(c_compiler(Some(false), false).fix.unwrap().contains("build-essential"));
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
