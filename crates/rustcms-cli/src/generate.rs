//! `make:model`, `make:migration`, `make:test` and `make:widget`.

use std::path::Path;
use std::process::ExitCode;

use anyhow::{Context, bail};
use serde_json::json;

use crate::layout;
use crate::naming::{pascal_case, plural, snake_case, validate_model, validate_snake};
use crate::project::Project;
use crate::scaffold::render;

const MODEL: &str = include_str!("../templates/stubs/model.rs.tmpl");
const MIGRATION_CREATE: &str = include_str!("../templates/stubs/migration_create.rs.tmpl");
const MIGRATION_BLANK: &str = include_str!("../templates/stubs/migration_blank.rs.tmpl");
const TEST: &str = include_str!("../templates/stubs/test.rs.tmpl");
const WIDGET: &str = include_str!("../templates/stubs/widget.js.tmpl");

pub fn make_model(project: &Project, name: &str, migration: bool) -> anyhow::Result<ExitCode> {
    validate_model(name)?;
    let module = snake_case(name);
    let table = plural(&module);
    let file = project.src(layout::MODELS.path).join(format!("{module}.rs"));
    preflight(&[&file], &[(&project.src("models/mod.rs"), layout::MODELS.anchor)])?;
    if migration {
        preflight_migration(project)?;
    }
    write_new(&file, &render(MODEL, json!({ "table": table }))?)?;
    register(&project.src("models/mod.rs"), layout::MODELS.anchor, &format!("pub mod {module};"))?;
    if migration {
        make_migration(project, &format!("create_{table}_table"))?;
    }
    Ok(ExitCode::SUCCESS)
}

fn preflight_migration(project: &Project) -> anyhow::Result<()> {
    let index = project.src(layout::MIGRATIONS).join("mod.rs");
    preflight(&[], &[(&index, MIGRATION_MODS), (&index, MIGRATION_LIST)])
}

const MIGRATION_MODS: &str = "// rustcms:migration-mods";
const MIGRATION_LIST: &str = "// rustcms:migrations";

pub fn make_migration(project: &Project, name: &str) -> anyhow::Result<ExitCode> {
    validate_snake(name)?;
    let module = format!("{}_{name}", chrono::Utc::now().format("m%Y%m%d_%H%M%S"));
    let contents = match name.strip_prefix("create_").and_then(|n| n.strip_suffix("_table")) {
        Some(table) => render(MIGRATION_CREATE, json!({ "iden": pascal_case(table) }))?,
        None => render(MIGRATION_BLANK, json!({ "name": name }))?,
    };
    let dir = project.src(layout::MIGRATIONS);
    let file = dir.join(format!("{module}.rs"));
    preflight(&[&file], &[])?;
    preflight_migration(project)?;
    write_new(&file, &contents)?;
    let index = dir.join("mod.rs");
    register(&index, MIGRATION_MODS, &format!("mod {module};"))?;
    register(&index, MIGRATION_LIST, &format!("Box::new({module}::Migration),"))?;
    Ok(ExitCode::SUCCESS)
}

/// The feature suite's entry point: one test binary (`cargo test --test feature`).
const TESTS_ANCHOR: &str = "// rustcms:tests";

pub fn make_test(project: &Project, name: &str) -> anyhow::Result<ExitCode> {
    validate_model(name)?;
    let module = snake_case(name);
    let dir = project.root.join("tests/feature");
    let file = dir.join(format!("{module}.rs"));
    let main = dir.join("main.rs");
    preflight(&[&file], &[(&main, TESTS_ANCHOR)])?;
    let contents = render(TEST, json!({ "name": module, "crate_name": project.package.replace('-', "_") }))?;
    write_new(&file, &contents)?;
    register(&main, TESTS_ANCHOR, &format!("mod {module};"))?;
    Ok(ExitCode::SUCCESS)
}

/// Writes `assets/admin/<vendor>/<name>.js` for the widget `vendor/name` and
/// prints how to register it.
pub fn make_widget(project: &Project, widget: &str) -> anyhow::Result<ExitCode> {
    let part = |p: &str| {
        !p.is_empty() && p.len() <= 32 && p.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    };
    let Some((vendor, name)) = widget.split_once('/').filter(|(v, n)| part(v) && part(n)) else {
        bail!("widget names are `vendor/name`, each part 1-32 of a-z, 0-9 and -: {widget}");
    };
    let extension = extension_name(widget);
    let dir = project.root.join("assets/admin").join(vendor);
    std::fs::create_dir_all(&dir)?;
    let contents = render(WIDGET, json!({ "widget": widget, "class": format!("{extension}-input") }))?;
    write_new(&dir.join(format!("{name}.js")), &contents)?;
    println!(
        "Register it on the admin provider:\n    .extension_script(\"{extension}\", include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/assets/admin/{vendor}/{name}.js\")))\nand use it on a field:\n    CustomInput::make(\"field\", \"{widget}\")"
    );
    Ok(ExitCode::SUCCESS)
}

/// A valid extension name (≤ 64 characters) for `widget`, distinct for every
/// widget name: `acme/color-picker` and `acme-color/picker` differ by the hash.
fn extension_name(widget: &str) -> String {
    use sha2::Digest;
    let hash: String = sha2::Sha256::digest(widget).iter().take(4).map(|b| format!("{b:02x}")).collect();
    let readable: String = widget.replace('/', "-").chars().take(55).collect();
    format!("{readable}-{hash}")
}

/// Checks before writing anything: none of `new` exists yet, and every
/// `(file, anchor)` has its anchor line (generators never half-apply).
pub(crate) fn preflight(new: &[&Path], anchors: &[(&Path, &str)]) -> anyhow::Result<()> {
    for path in new {
        if path.exists() {
            bail!("{} already exists", path.display());
        }
    }
    for (file, anchor) in anchors {
        let contents = std::fs::read_to_string(file)
            .with_context(|| format!("{} not found (see the upgrade guide, \"Application layout\")", file.display()))?;
        if !contents.lines().any(|l| l.trim() == *anchor) {
            bail!("{} has no `{anchor}` line: add it where generated code should be registered", file.display());
        }
    }
    Ok(())
}

pub(crate) fn write_new(path: &Path, contents: &str) -> anyhow::Result<()> {
    if path.exists() {
        bail!("{} already exists", path.display());
    }
    std::fs::write(path, contents).with_context(|| format!("writing {}", path.display()))?;
    println!("Created {}", path.display());
    Ok(())
}

/// Inserts `line` above the anchor comment, with the anchor's indentation;
/// returns whether the line is now in the file. Already present: nothing to do.
/// Without the file or the anchor, prints the line for the developer to paste.
pub(crate) fn register(file: &Path, anchor: &str, line: &str) -> anyhow::Result<bool> {
    let Ok(contents) = std::fs::read_to_string(file) else {
        println!("Add to {} (file not found):\n    {line}", file.display());
        return Ok(false);
    };
    if contents.lines().any(|l| l.trim() == line.trim()) {
        return Ok(true);
    }
    let Some(index) = contents.lines().position(|l| l.trim() == anchor) else {
        println!("Add to {} (anchor `{anchor}` not found):\n    {line}", file.display());
        return Ok(false);
    };
    let newline = if contents.contains("\r\n") { "\r\n" } else { "\n" };
    let mut lines: Vec<&str> = contents.lines().collect();
    let indent: String = lines[index].chars().take_while(|c| c.is_whitespace()).collect();
    let inserted = format!("{indent}{line}");
    lines.insert(index, &inserted);
    std::fs::write(file, lines.join(newline) + newline).with_context(|| format!("writing {}", file.display()))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> (tempfile::TempDir, Project) {
        let dir = tempfile::tempdir().unwrap();
        for (path, contents) in [
            ("src/models/mod.rs", "// rustcms:models\n"),
            (
                "src/database/migrations/mod.rs",
                "// rustcms:migration-mods\nfn x() {\n    vec![\n        // rustcms:migrations\n    ]\n}\n",
            ),
            (
                "tests/feature/main.rs",
                "#[path = \"../support/mod.rs\"]\nmod support;\n\nmod health;\n// rustcms:tests\n",
            ),
        ] {
            let file = dir.path().join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, contents).unwrap();
        }
        let project = Project { root: dir.path().to_path_buf(), package: "blog".into() };
        (dir, project)
    }

    #[test]
    fn model_with_migration_registers_both() {
        let (dir, project) = project();
        make_model(&project, "BlogPost", true).unwrap();
        let model = std::fs::read_to_string(dir.path().join("src/models/blog_post.rs")).unwrap();
        assert!(model.contains("table_name = \"blog_posts\""));
        assert_eq!(
            std::fs::read_to_string(dir.path().join("src/models/mod.rs")).unwrap(),
            "pub mod blog_post;\n// rustcms:models\n"
        );

        let index = std::fs::read_to_string(dir.path().join("src/database/migrations/mod.rs")).unwrap();
        let module = index.lines().next().unwrap().trim_start_matches("mod ").trim_end_matches(';').to_owned();
        assert!(module.ends_with("_create_blog_posts_table"), "{index}");
        assert!(
            index.contains(&format!("        Box::new({module}::Migration),\n        // rustcms:migrations")),
            "{index}"
        );
        let migration =
            std::fs::read_to_string(dir.path().join(format!("src/database/migrations/{module}.rs"))).unwrap();
        assert!(migration.contains("enum BlogPosts"));

        assert!(make_model(&project, "BlogPost", false).is_err(), "never overwrites");
    }

    #[test]
    fn test_stub_uses_the_crate_name() {
        let (dir, project) = project();
        let project = Project { package: "my-blog".into(), ..project };
        make_test(&project, "PostAdminTest").unwrap();
        let test = std::fs::read_to_string(dir.path().join("tests/feature/post_admin_test.rs")).unwrap();
        assert!(test.contains("use my_blog::database::migrations::Migrator;"), "{test}");
        assert!(test.contains("db_test!(post_admin_test,"));
        let main = std::fs::read_to_string(dir.path().join("tests/feature/main.rs")).unwrap();
        assert!(main.contains("mod health;\nmod post_admin_test;\n// rustcms:tests"), "{main}");
        assert!(make_test(&project, "PostAdminTest").is_err(), "never overwrites");
    }

    #[test]
    fn widget_stub_registers_the_widget() {
        let (dir, project) = project();
        make_widget(&project, "acme/color").unwrap();
        let script = std::fs::read_to_string(dir.path().join("assets/admin/acme/color.js")).unwrap();
        assert!(script.contains("window.rustcms.registerWidget(\"acme/color\", {"));
        assert!(script.contains("apiVersion: 1,"));
        assert!(make_widget(&project, "acme/color").is_err(), "never overwrites");
        // The longest names fit; names that only differ by where the `/` is get distinct extensions.
        make_widget(&project, &format!("{}/{}", "v".repeat(32), "n".repeat(32))).unwrap();
        assert_eq!(extension_name(&format!("{}/{}", "v".repeat(32), "n".repeat(32))).len(), 64);
        assert_ne!(extension_name("acme/color-picker"), extension_name("acme-color/picker"));
        assert!(extension_name("acme/color").starts_with("acme-color-"));
        for bad in ["color", "Acme/color", "acme/col or", "acme/", "a/b/c", &format!("acme/{}", "x".repeat(33))] {
            assert!(make_widget(&project, bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn blank_migration_and_missing_anchor() {
        let (dir, project) = project();
        std::fs::write(dir.path().join("src/database/migrations/mod.rs"), "// edited by hand\n").unwrap();
        // Without its anchors, nothing is written.
        assert!(make_migration(&project, "add_slug_to_posts").is_err());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("src/database/migrations/mod.rs")).unwrap(),
            "// edited by hand\n"
        );
        assert_eq!(std::fs::read_dir(dir.path().join("src/database/migrations")).unwrap().count(), 1);
        assert!(make_migration(&project, "Bad Name").is_err());
    }
}
