//! `rustcms new` + generators produce an app that compiles and passes its tests.
//!
//! Slow (compiles a fresh app): `cargo test -p rustcms-cli -- --ignored` (`just test-cli`).

use std::path::Path;
use std::process::Command;

use assert_cmd::prelude::*;

fn rustcms(dir: &Path) -> Command {
    let mut cmd = Command::cargo_bin("rustcms").unwrap();
    cmd.current_dir(dir);
    cmd
}

#[test]
#[ignore = "compiles a generated application"]
fn generated_app_builds_and_passes_its_tests() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    rustcms(tmp.path()).args(["new", "blog", "--rustcms-path"]).arg(repo).assert().success();
    let app = tmp.path().join("blog");

    rustcms(&app).args(["make:model", "BlogPost", "-m"]).assert().success();
    rustcms(&app.join("src")).args(["make:migration", "add_slug_to_blog_posts"]).assert().success();
    // The blank migration is a `todo!()` for the developer; replace it so the app migrates.
    let blank = std::fs::read_dir(app.join("src/migrations"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.to_string_lossy().ends_with("_add_slug_to_blog_posts.rs"))
        .unwrap();
    let contents = std::fs::read_to_string(&blank)
        .unwrap()
        .replace("todo!(\"add_slug_to_blog_posts: write the migration\")", "Ok(())")
        .replace("todo!(\"add_slug_to_blog_posts: undo the migration\")", "Ok(())");
    std::fs::write(&blank, contents).unwrap();

    let env = std::fs::read_to_string(app.join(".env")).unwrap();
    assert!(env.lines().any(|l| l.starts_with("APP_KEY=base64:")), "fresh key in .env");

    let target = repo.join("target/scaffold-test");
    let status = Command::new(env!("CARGO"))
        .current_dir(&app)
        .env("CARGO_TARGET_DIR", &target)
        .args(["test", "--quiet"])
        .status()
        .unwrap();
    assert!(status.success(), "generated app tests failed");

    rustcms(&app).arg("migrate").env("CARGO_TARGET_DIR", &target).assert().success();
    rustcms(&app)
        .arg("migrate:status")
        .env("CARGO_TARGET_DIR", &target)
        .assert()
        .success()
        .stdout(predicates::str::contains("create_blog_posts_table"));
}

#[test]
fn outside_a_project_app_commands_fail_with_a_hint() {
    let tmp = tempfile::tempdir().unwrap();
    rustcms(tmp.path()).arg("migrate").assert().failure().stderr(predicates::str::contains("rustcms new"));
}
