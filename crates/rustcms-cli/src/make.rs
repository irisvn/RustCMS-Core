//! `make:service`, `make:job`, `make:seeder`, `make:factory`, `make:settings`
//! and `make:command`: one file each, registered where the framework finds it.

use std::process::ExitCode;

use anyhow::bail;
use serde_json::json;

use crate::generate::{preflight, register, write_new};
use crate::layout::{self, ModuleDir};
use crate::naming::{pascal_case, plural, snake_case, validate_model};
use crate::project::Project;
use crate::scaffold::render;

const SERVICE: &str = include_str!("../templates/stubs/service.rs.tmpl");
const JOB: &str = include_str!("../templates/stubs/job.rs.tmpl");
const SEEDER: &str = include_str!("../templates/stubs/seeder.rs.tmpl");
const FACTORY: &str = include_str!("../templates/stubs/factory.rs.tmpl");
const SETTINGS: &str = include_str!("../templates/stubs/settings.rs.tmpl");
const SETTINGS_PAGE: &str = include_str!("../templates/stubs/settings_page.rs.tmpl");
const COMMAND: &str = include_str!("../templates/stubs/command.rs.tmpl");

const APP_PROVIDER: &str = "providers/app_service_provider.rs";
const ADMIN_PROVIDER: &str = "providers/admin_panel_provider.rs";
const SEEDER_CALLS: &str = "// rustcms:seeder-calls";

/// `Name` with the `suffix` it must end with (`EnrollmentService`), and the
/// stem before it (`Enrollment`).
fn suffixed<'a>(name: &'a str, suffix: &str) -> anyhow::Result<&'a str> {
    validate_model(name)?;
    match name.strip_suffix(suffix).filter(|s| !s.is_empty()) {
        Some(stem) => Ok(stem),
        None => bail!("{suffix} names end with `{suffix}`, e.g. `Course{suffix}`"),
    }
}

/// Writes `src/<dir>/<module>.rs` after checking every other `(file, anchor)`.
fn write_module(
    project: &Project,
    dir: &ModuleDir,
    module: &str,
    contents: &str,
    anchors: &[(&str, &str)],
) -> anyhow::Result<()> {
    let file = project.src(dir.path).join(format!("{module}.rs"));
    let anchors: Vec<(std::path::PathBuf, &str)> = anchors.iter().map(|(f, a)| (project.src(f), *a)).collect();
    let checks: Vec<(&std::path::Path, &str)> = anchors.iter().map(|(f, a)| (f.as_path(), *a)).collect();
    preflight(&[&file], &checks)?;
    let index = project.ensure_module(dir)?;
    preflight(&[], &[(&index, dir.anchor)])?;
    write_new(&file, contents)?;
    register(&index, dir.anchor, &format!("pub mod {module};"))?;
    Ok(())
}

pub fn make_service(project: &Project, name: &str) -> anyhow::Result<ExitCode> {
    let stem = suffixed(name, "Service")?;
    let module = snake_case(name);
    let contents = render(SERVICE, json!({ "label": stem }))?;
    write_module(project, &layout::SERVICES, &module, &contents, &[])?;
    Ok(ExitCode::SUCCESS)
}

pub fn make_job(project: &Project, name: &str) -> anyhow::Result<ExitCode> {
    validate_model(name)?;
    let module = snake_case(name);
    let contents = render(JOB, json!({ "name": name, "kind": module }))?;
    write_module(project, &layout::JOBS, &module, &contents, &[(APP_PROVIDER, "// rustcms:jobs")])?;
    register(&project.src(APP_PROVIDER), "// rustcms:jobs", &format!("app.job::<crate::jobs::{module}::{name}>();"))?;
    Ok(ExitCode::SUCCESS)
}

pub fn make_seeder(project: &Project, name: &str) -> anyhow::Result<ExitCode> {
    let stem = suffixed(name, "Seeder")?;
    let module = snake_case(name);
    let seeder_name = snake_case(stem);
    let contents = render(SEEDER, json!({ "name": name, "seeder_name": seeder_name }))?;
    let seeders = format!("{}/mod.rs", layout::SEEDERS.path);
    write_module(
        project,
        &layout::SEEDERS,
        &module,
        &contents,
        &[(APP_PROVIDER, "// rustcms:seeders"), (&seeders, SEEDER_CALLS)],
    )?;
    register(
        &project.src(APP_PROVIDER),
        "// rustcms:seeders",
        &format!("app.seeder(crate::database::seeders::{module}::{name});"),
    )?;
    register(&project.src(&seeders), SEEDER_CALLS, &format!("&{module}::{name},"))?;
    println!("`DatabaseSeeder` calls it last; reorder the calls in {seeders} if it depends on others.");
    Ok(ExitCode::SUCCESS)
}

pub fn make_factory(project: &Project, model: &str) -> anyhow::Result<ExitCode> {
    validate_model(model)?;
    let module = snake_case(model);
    if !project.src("models").join(format!("{module}.rs")).exists() {
        bail!("src/models/{module}.rs not found: create the model first (`rustcms make:model {model} -m`)");
    }
    let contents = render(FACTORY, json!({ "name": model, "module": module, "table": plural(&module) }))?;
    write_module(project, &layout::FACTORIES, &format!("{module}_factory"), &contents, &[])?;
    Ok(ExitCode::SUCCESS)
}

pub fn make_settings(project: &Project, name: &str) -> anyhow::Result<ExitCode> {
    let stem = suffixed(name, "Settings")?;
    let module = snake_case(name);
    let group = snake_case(stem);
    let vars = json!({ "name": name, "module": module, "group": group, "label": stem });
    let page = format!("{module}_page");
    let page_file = project.src(layout::PAGES.path).join(format!("{page}.rs"));
    preflight(&[&page_file], &[(&project.src(ADMIN_PROVIDER), "// rustcms:pages")])?;
    write_module(project, &layout::SETTINGS, &module, &render(SETTINGS, &vars)?, &[])?;
    write_module(project, &layout::PAGES, &page, &render(SETTINGS_PAGE, &vars)?, &[])?;
    register(
        &project.src(ADMIN_PROVIDER),
        "// rustcms:pages",
        &format!(".settings::<crate::settings::{module}::{name}>()"),
    )?;
    println!(
        "Permissions `settings.{group}.view` and `settings.{group}.update` are declared; run `rustcms permission:sync`."
    );
    Ok(ExitCode::SUCCESS)
}

pub fn make_command(project: &Project, command: &str) -> anyhow::Result<ExitCode> {
    let valid = !command.is_empty()
        && command.len() <= 64
        && command.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"-_:".contains(&b))
        && command.starts_with(|c: char| c.is_ascii_lowercase());
    if !valid {
        bail!("command names are lowercase words joined by `:`, `-` or `_`, e.g. `lms:report`: {command}");
    }
    let module = command.replace([':', '-'], "_");
    let name = pascal_case(&module);
    let contents = render(COMMAND, json!({ "name": name, "command": command }))?;
    write_module(project, &layout::CONSOLE, &module, &contents, &[(APP_PROVIDER, "// rustcms:commands")])?;
    register(
        &project.src(APP_PROVIDER),
        "// rustcms:commands",
        &format!("app.command(crate::console::{module}::{name});"),
    )?;
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An application as `rustcms new` lays it out (the parts these generators touch).
    fn project() -> (tempfile::TempDir, Project) {
        let dir = tempfile::tempdir().unwrap();
        let template = &crate::scaffold::APP_TEMPLATE;
        for path in [
            "src/lib.rs.tmpl",
            "src/providers/app_service_provider.rs.tmpl",
            "src/providers/admin_panel_provider.rs.tmpl",
            "src/database/mod.rs.tmpl",
            "src/database/seeders/mod.rs.tmpl",
            "src/bootstrap/mod.rs.tmpl",
        ] {
            let file = dir.path().join(path.trim_end_matches(".tmpl"));
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, template.get_file(path).unwrap().contents()).unwrap();
        }
        std::fs::create_dir_all(dir.path().join("src/models")).unwrap();
        std::fs::write(dir.path().join("src/models/course.rs"), "").unwrap();
        let project = Project { root: dir.path().to_path_buf(), package: "lms".into() };
        (dir, project)
    }

    fn read(dir: &tempfile::TempDir, path: &str) -> String {
        std::fs::read_to_string(dir.path().join(path)).unwrap()
    }

    #[test]
    fn each_generator_writes_and_registers_once() {
        let (dir, project) = project();
        make_service(&project, "EnrollmentService").unwrap();
        assert!(read(&dir, "src/lib.rs").contains("pub mod services;\n// rustcms:mods"));
        assert!(read(&dir, "src/services/mod.rs").contains("pub mod enrollment_service;"));

        make_job(&project, "SendWelcome").unwrap();
        assert!(read(&dir, "src/jobs/send_welcome.rs").contains("const KIND: &'static str = \"send_welcome\";"));
        assert!(
            read(&dir, "src/providers/app_service_provider.rs")
                .contains("app.job::<crate::jobs::send_welcome::SendWelcome>();")
        );

        make_seeder(&project, "CourseSeeder").unwrap();
        assert!(read(&dir, "src/database/seeders/course_seeder.rs").contains("\"course\""));
        assert!(read(&dir, "src/database/seeders/mod.rs").contains("pub mod course_seeder;"));
        assert!(read(&dir, "src/database/seeders/mod.rs").contains("&course_seeder::CourseSeeder,"));
        assert!(
            read(&dir, "src/providers/app_service_provider.rs")
                .contains("app.seeder(crate::database::seeders::course_seeder::CourseSeeder);")
        );

        make_factory(&project, "Course").unwrap();
        assert!(read(&dir, "src/database/mod.rs").contains("pub mod factories;\n// rustcms:database-mods"));
        assert!(read(&dir, "src/database/factories/course_factory.rs").contains("pub struct CourseFactory;"));
        assert!(make_factory(&project, "Lesson").is_err(), "the model must exist");

        make_settings(&project, "SchoolSettings").unwrap();
        assert!(read(&dir, "src/settings/school_settings.rs").contains("const GROUP: &'static str = \"school\";"));
        assert!(read(&dir, "src/admin/pages/school_settings_page.rs").contains("impl SettingsPage for SchoolSettings"));
        assert!(
            read(&dir, "src/providers/admin_panel_provider.rs")
                .contains(".settings::<crate::settings::school_settings::SchoolSettings>()")
        );

        make_command(&project, "lms:report").unwrap();
        assert!(read(&dir, "src/console/lms_report.rs").contains("Command::new(\"lms:report\")"));
        assert!(
            read(&dir, "src/providers/app_service_provider.rs")
                .contains("app.command(crate::console::lms_report::LmsReport);")
        );

        // Never twice, never half: a second run fails before writing anything.
        for again in [
            make_service(&project, "EnrollmentService"),
            make_job(&project, "SendWelcome"),
            make_seeder(&project, "CourseSeeder"),
            make_settings(&project, "SchoolSettings"),
            make_command(&project, "lms:report"),
        ] {
            assert!(again.is_err());
        }
        assert_eq!(read(&dir, "src/providers/app_service_provider.rs").matches("SendWelcome").count(), 1);
    }

    #[test]
    fn names_are_checked() {
        let (_dir, project) = project();
        assert!(make_service(&project, "Enrollment").is_err(), "suffix");
        assert!(make_seeder(&project, "Seeder").is_err(), "stem");
        assert!(make_settings(&project, "school_settings").is_err(), "PascalCase");
        for bad in ["", "Lms:report", "lms report", ":x"] {
            assert!(make_command(&project, bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_missing_anchor_stops_before_writing() {
        let (dir, project) = project();
        std::fs::write(dir.path().join("src/providers/app_service_provider.rs"), "// edited by hand\n").unwrap();
        assert!(make_job(&project, "SendWelcome").is_err());
        assert!(!dir.path().join("src/jobs").exists(), "nothing written");
    }
}
