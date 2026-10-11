//! `make:model` and `make:migration`.

use std::path::Path;
use std::process::ExitCode;

use anyhow::{Context, bail};
use serde_json::json;

use crate::naming::{pascal_case, plural, snake_case, validate_model, validate_snake};
use crate::project::Project;
use crate::scaffold::render;

const MODEL: &str = include_str!("../templates/stubs/model.rs.tmpl");
const MIGRATION_CREATE: &str = include_str!("../templates/stubs/migration_create.rs.tmpl");
const MIGRATION_BLANK: &str = include_str!("../templates/stubs/migration_blank.rs.tmpl");

pub fn make_model(project: &Project, name: &str, migration: bool) -> anyhow::Result<ExitCode> {
    validate_model(name)?;
    let module = snake_case(name);
    let table = plural(&module);
    let file = project.root.join("src/models").join(format!("{module}.rs"));
    write_new(&file, &render(MODEL, json!({ "table": table }))?)?;
    register(&project.root.join("src/models/mod.rs"), "// rustcms:models", &format!("pub mod {module};"))?;
    if migration {
        make_migration(project, &format!("create_{table}_table"))?;
    }
    Ok(ExitCode::SUCCESS)
}

pub fn make_migration(project: &Project, name: &str) -> anyhow::Result<ExitCode> {
    validate_snake(name)?;
    let module = format!("{}_{name}", chrono::Utc::now().format("m%Y%m%d_%H%M%S"));
    let contents = match name.strip_prefix("create_").and_then(|n| n.strip_suffix("_table")) {
        Some(table) => render(MIGRATION_CREATE, json!({ "iden": pascal_case(table) }))?,
        None => render(MIGRATION_BLANK, json!({ "name": name }))?,
    };
    let dir = project.root.join("src/migrations");
    write_new(&dir.join(format!("{module}.rs")), &contents)?;
    let index = dir.join("mod.rs");
    register(&index, "// rustcms:migration-mods", &format!("mod {module};"))?;
    register(&index, "// rustcms:migrations", &format!("Box::new({module}::Migration),"))?;
    Ok(ExitCode::SUCCESS)
}

fn write_new(path: &Path, contents: &str) -> anyhow::Result<()> {
    if path.exists() {
        bail!("{} already exists", path.display());
    }
    std::fs::write(path, contents).with_context(|| format!("writing {}", path.display()))?;
    println!("Created {}", path.display());
    Ok(())
}

/// Inserts `line` above the anchor comment, with the anchor's indentation.
/// Without the anchor, prints the line for the developer to paste.
fn register(file: &Path, anchor: &str, line: &str) -> anyhow::Result<()> {
    let contents = std::fs::read_to_string(file).unwrap_or_default();
    let Some(index) = contents.lines().position(|l| l.trim() == anchor) else {
        println!("Add to {} (anchor `{anchor}` not found):\n    {line}", file.display());
        return Ok(());
    };
    let mut lines: Vec<&str> = contents.lines().collect();
    let indent: String = lines[index].chars().take_while(|c| c.is_whitespace()).collect();
    let inserted = format!("{indent}{line}");
    lines.insert(index, &inserted);
    std::fs::write(file, lines.join("\n") + "\n").with_context(|| format!("writing {}", file.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> (tempfile::TempDir, Project) {
        let dir = tempfile::tempdir().unwrap();
        for (path, contents) in [
            ("src/models/mod.rs", "// rustcms:models\n"),
            (
                "src/migrations/mod.rs",
                "// rustcms:migration-mods\nfn x() {\n    vec![\n        // rustcms:migrations\n    ]\n}\n",
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

        let index = std::fs::read_to_string(dir.path().join("src/migrations/mod.rs")).unwrap();
        let module = index.lines().next().unwrap().trim_start_matches("mod ").trim_end_matches(';').to_owned();
        assert!(module.ends_with("_create_blog_posts_table"), "{index}");
        assert!(
            index.contains(&format!("        Box::new({module}::Migration),\n        // rustcms:migrations")),
            "{index}"
        );
        let migration = std::fs::read_to_string(dir.path().join(format!("src/migrations/{module}.rs"))).unwrap();
        assert!(migration.contains("enum BlogPosts"));

        assert!(make_model(&project, "BlogPost", false).is_err(), "never overwrites");
    }

    #[test]
    fn blank_migration_and_missing_anchor() {
        let (dir, project) = project();
        std::fs::write(dir.path().join("src/migrations/mod.rs"), "// edited by hand\n").unwrap();
        make_migration(&project, "add_slug_to_posts").unwrap();
        assert_eq!(std::fs::read_to_string(dir.path().join("src/migrations/mod.rs")).unwrap(), "// edited by hand\n");
        assert!(make_migration(&project, "Bad Name").is_err());
    }
}
