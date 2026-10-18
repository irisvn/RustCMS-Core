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
const PAGE: &str = include_str!("../templates/stubs/page.rs.tmpl");
const PAGE_HTML: &str = include_str!("../templates/stubs/page.html.tmpl");
const PAGE_JS: &str = include_str!("../templates/stubs/page.js.tmpl");
const PAGE_TSX: &str = include_str!("../templates/stubs/page.tsx.tmpl");
const PAGE_CSS: &str = include_str!("../templates/stubs/page.css.tmpl");

/// How a custom page is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageKind {
    /// A server template (`resources/views/admin`).
    Template,
    /// A v1 script (`resources/admin/js`).
    Script,
    /// A v2 React page (`resources/admin/src`, built with the SDK).
    React,
}

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
    let index = project.prepare_module(dir)?;
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
    if module.len() > 64 {
        bail!("job kinds are at most 64 characters: `{module}`");
    }
    let contents = render(JOB, json!({ "name": name, "kind": module }))?;
    write_module(project, &layout::JOBS, &module, &contents, &[(APP_PROVIDER, "// rustcms:jobs")])?;
    register(&project.src(APP_PROVIDER), "// rustcms:jobs", &format!("app.job::<crate::jobs::{module}::{name}>();"))?;
    Ok(ExitCode::SUCCESS)
}

pub fn make_seeder(project: &Project, name: &str) -> anyhow::Result<ExitCode> {
    let stem = suffixed(name, "Seeder")?;
    let module = snake_case(name);
    let seeder_name = snake_case(stem);
    if seeder_name == "database" {
        bail!("`DatabaseSeeder` comes with the application (src/database/seeders/mod.rs)");
    }
    if seeder_name.len() > 64 {
        bail!("seeder names are at most 64 characters: `{seeder_name}`");
    }
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
    if !rustcms_core::settings::valid_group(&group) {
        bail!("settings groups are 1-32 of a-z, 0-9 and _: `{group}` (from {name})");
    }
    let vars = json!({ "name": name, "module": module, "group": group, "label": stem });
    let page = format!("{module}_page");
    let data_file = project.src(layout::SETTINGS.path).join(format!("{module}.rs"));
    let page_file = project.src(layout::PAGES.path).join(format!("{page}.rs"));
    // Both halves or neither: every check before the first write.
    preflight(&[&data_file, &page_file], &[(&project.src(ADMIN_PROVIDER), "// rustcms:pages")])?;
    let data_index = project.prepare_module(&layout::SETTINGS)?;
    let page_index = project.prepare_module(&layout::PAGES)?;
    write_new(&data_file, &render(SETTINGS, &vars)?)?;
    write_new(&page_file, &render(SETTINGS_PAGE, &vars)?)?;
    register(&data_index, layout::SETTINGS.anchor, &format!("pub mod {module};"))?;
    register(&page_index, layout::PAGES.anchor, &format!("pub mod {page};"))?;
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
    // The `rustcms` command's own commands would shadow it (`rustcms -- <name>` still reaches it).
    use clap::CommandFactory;
    if crate::Cli::command().get_subcommands().any(|c| c.get_name() == command) {
        bail!("`{command}` is a command of `rustcms` itself: pick another name");
    }
    let module = command.replace([':', '-'], "_");
    if crate::naming::is_keyword(&module) {
        bail!("`{command}` would make the module `{module}`, a Rust keyword: pick another name");
    }
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

pub fn make_page(project: &Project, name: &str, kind: PageKind) -> anyhow::Result<ExitCode> {
    let script = kind != PageKind::Template;
    let react = kind == PageKind::React;
    let stem = suffixed(name, "Page")?;
    let snake = snake_case(stem);
    let module = snake_case(name);
    let slug = snake.replace('_', "-");
    if slug.len() > 64 {
        bail!("page slugs are at most 64 characters: `{slug}`");
    }
    let vendor = project.package.replace('_', "-");
    let script_name = format!("{vendor}/{slug}");
    // As the panel checks scripts at boot: each part 1–32 of a-z, 0-9, `-`.
    let part = |p: &str| {
        (1..=32).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    };
    if script && !(part(&vendor) && part(&slug)) {
        bail!(
            "script pages are named `vendor/name`, each part at most 32 lowercase letters, digits or `-`: `{script_name}` (from the package name and the page name)"
        );
    }
    let label = crate::naming::humanize(stem);
    let vars = json!({
        "name": name,
        "slug": slug,
        "module": snake,
        "label": label,
        "permission": format!("{snake}.view"),
        "script": if script { Some(script_name.clone()) } else { None },
    });
    let page_file = project.src(layout::PAGES.path).join(format!("{module}.rs"));
    let admin = project.root.join("resources/admin");
    let built_js = admin.join(format!("js/{vendor}/{slug}.js"));
    let built_css = admin.join(format!("css/{vendor}/{slug}.css"));
    let source_css = admin.join(format!("src/{vendor}/{slug}.css"));
    let asset = if react {
        admin.join(format!("src/{vendor}/{slug}.tsx"))
    } else if script {
        built_js.clone()
    } else {
        project.root.join(format!("resources/views/admin/{snake}.html"))
    };
    let mut anchors = vec![(project.src(ADMIN_PROVIDER), "// rustcms:pages")];
    if script {
        anchors.push((project.src(ADMIN_PROVIDER), "// rustcms:extensions"));
    }
    let checks: Vec<(&std::path::Path, &str)> = anchors.iter().map(|(f, a)| (f.as_path(), *a)).collect();
    let mut new: Vec<&std::path::Path> = vec![&page_file, &asset];
    if react {
        new.extend([built_js.as_path(), built_css.as_path(), source_css.as_path()]);
    }
    preflight(&new, &checks)?;
    let fresh_setup = if react { crate::react::ensure_setup(&admin)? } else { false };
    let index = project.prepare_module(&layout::PAGES)?;
    std::fs::create_dir_all(asset.parent().expect("a file"))?;
    write_new(&page_file, &render(PAGE, &vars)?)?;
    write_new(
        &asset,
        &render(
            if react {
                PAGE_TSX
            } else if script {
                PAGE_JS
            } else {
                PAGE_HTML
            },
            &vars,
        )?,
    )?;
    if react {
        // Until `npm run build`: the page shows as unavailable, the app builds.
        std::fs::create_dir_all(built_js.parent().expect("a file"))?;
        std::fs::create_dir_all(built_css.parent().expect("a file"))?;
        write_new(&built_js, "// Built by `npm run build` in resources/admin (rustcms-build).\n")?;
        write_new(&built_css, "")?;
        write_new(&source_css, &render(PAGE_CSS, &vars)?)?;
    }
    register(&index, layout::PAGES.anchor, &format!("pub mod {module};"))?;
    let provider = project.src(ADMIN_PROVIDER);
    register(&provider, "// rustcms:pages", &format!(".page::<crate::admin::pages::{module}::{name}>()"))?;
    if react {
        let manifest = "concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/resources/admin";
        register(
            &provider,
            "// rustcms:extensions",
            &format!(
                ".extension_script_v2(\"{slug}\", include_str!({manifest}/js/{vendor}/{slug}.js\")), {})",
                crate::react::REQUIRES
            ),
        )?;
        register(
            &provider,
            "// rustcms:extensions",
            &format!(".extension_style(\"{slug}\", include_str!({manifest}/css/{vendor}/{slug}.css\")))"),
        )?;
    } else if script {
        register(
            &provider,
            "// rustcms:extensions",
            &format!(
                ".extension_script(\"{slug}\", include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/resources/admin/js/{vendor}/{slug}.js\")))"
            ),
        )?;
    }
    if react {
        if fresh_setup {
            println!(
                "Build it: cd resources/admin && npm install && npm run build, then commit package-lock.json and the built js/ and css/ files."
            );
        } else {
            println!("Build it: cd resources/admin && npm ci && npm run build, then commit the built files.");
        }
    }
    println!("`{snake}.view` is declared; run `rustcms permission:sync` and grant it through a role.");
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

        make_page(&project, "SeatOverviewPage", PageKind::Template).unwrap();
        let page = read(&dir, "src/admin/pages/seat_overview_page.rs");
        assert!(
            page.contains("const SLUG: &'static str = \"seat-overview\";")
                && page.contains("declare_permission(\"seat_overview.view\")")
        );
        assert!(page.contains("rustcms::admin::view!(\"seat_overview.html\")"));
        assert!(read(&dir, "resources/views/admin/seat_overview.html").contains("{{ t(\"Seat overview\") }}"));
        assert!(
            read(&dir, "src/providers/admin_panel_provider.rs")
                .contains(".page::<crate::admin::pages::seat_overview_page::SeatOverviewPage>()")
        );
        make_page(&project, "ProgressPage", PageKind::Script).unwrap();
        assert!(read(&dir, "src/admin/pages/progress_page.rs").contains("PageView::Script(\"lms/progress\")"));
        assert!(read(&dir, "resources/admin/js/lms/progress.js").contains("registerPage(\"lms/progress\""));
        assert!(read(&dir, "src/providers/admin_panel_provider.rs").contains("/resources/admin/js/lms/progress.js"));
        assert!(make_page(&project, "SeatOverviewPage", PageKind::Template).is_err(), "never twice");
        assert!(make_page(&project, "Overview", PageKind::Template).is_err(), "names end with Page");
        let long = "AVeryLongNameForAScriptPageThatIsTooLongPage";
        assert!(make_page(&project, long, PageKind::Script).is_err(), "script names the panel would refuse");
        assert!(
            !dir.path().join("src/admin/pages/a_very_long_name_for_a_script_page_that_is_too_long_page.rs").exists()
        );
        // A React page: the source, placeholders for the build, the setup once.
        make_page(&project, "StatsPage", PageKind::React).unwrap();
        assert!(read(&dir, "src/admin/pages/stats_page.rs").contains("PageView::Script(\"lms/stats\")"));
        assert!(read(&dir, "resources/admin/src/lms/stats.tsx").contains("registerPage(\"lms/stats\", StatsPage)"));
        assert!(dir.path().join("resources/admin/js/lms/stats.js").exists());
        assert!(dir.path().join("resources/admin/css/lms/stats.css").exists());
        assert!(dir.path().join("resources/admin/sdk/index.d.ts").exists());
        let provider = read(&dir, "src/providers/admin_panel_provider.rs");
        assert!(provider.contains(".extension_script_v2(\"stats\"") && provider.contains("rustcms::admin::Requires"));
        assert!(provider.contains("/resources/admin/css/lms/stats.css"));
        // A second page leaves the setup (even edited) and the first page alone.
        std::fs::write(dir.path().join("resources/admin/package.json"), "{\"edited\": true}").unwrap();
        make_page(&project, "ChartsPage", PageKind::React).unwrap();
        assert_eq!(read(&dir, "resources/admin/package.json"), "{\"edited\": true}");
        assert!(dir.path().join("resources/admin/src/lms/stats.tsx").exists());
        assert!(make_page(&project, "StatsPage", PageKind::React).is_err(), "never twice");
        let upper = Project { root: dir.path().to_path_buf(), package: "My_Lms".into() };
        assert!(make_page(&upper, "CalendarPage", PageKind::Script).is_err(), "the vendor comes from the package name");

        make_command(&project, "lms:report").unwrap();
        for own in ["doctor", "upgrade", "self-update", "docs", "new"] {
            assert!(make_command(&project, own).is_err(), "{own} is a rustcms command");
        }
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
        for bad in ["", "Lms:report", "lms report", ":x", "type", "match"] {
            assert!(make_command(&project, bad).is_err(), "{bad:?}");
        }
        assert!(make_seeder(&project, "DatabaseSeeder").is_err(), "the application's own");
        let long = format!("{}Settings", "Long".repeat(9));
        assert!(make_settings(&project, &long).is_err(), "a group of more than 32 characters");
        assert!(!_dir.path().join("src/settings").exists() && !_dir.path().join("src/console").exists());
    }

    #[test]
    fn module_chains_are_checked_before_writing() {
        let (dir, project) = project();
        // A flat `services.rs` would clash with `services/mod.rs`.
        std::fs::write(dir.path().join("src/services.rs"), "").unwrap();
        assert!(make_service(&project, "EnrollmentService").is_err());
        assert!(!dir.path().join("src/services").exists());
        // An index the parent does not declare is not a module.
        std::fs::create_dir_all(dir.path().join("src/jobs")).unwrap();
        std::fs::write(dir.path().join("src/jobs/mod.rs"), "// rustcms:jobs\n").unwrap();
        assert!(make_job(&project, "SendWelcome").is_err());
        assert!(!dir.path().join("src/jobs/send_welcome.rs").exists());
        // Settings: both halves or neither.
        std::fs::create_dir_all(dir.path().join("src/admin/pages")).unwrap();
        std::fs::write(dir.path().join("src/admin/mod.rs"), "pub mod pages;\n").unwrap();
        std::fs::write(dir.path().join("src/admin/pages/mod.rs"), "// edited by hand\n").unwrap();
        assert!(make_settings(&project, "SchoolSettings").is_err());
        assert!(!dir.path().join("src/settings/school_settings.rs").exists(), "no half");
    }

    #[test]
    fn a_missing_anchor_stops_before_writing() {
        let (dir, project) = project();
        std::fs::write(dir.path().join("src/providers/app_service_provider.rs"), "// edited by hand\n").unwrap();
        assert!(make_job(&project, "SendWelcome").is_err());
        assert!(!dir.path().join("src/jobs").exists(), "nothing written");
    }
}
