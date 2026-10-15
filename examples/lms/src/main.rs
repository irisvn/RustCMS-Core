fn main() -> std::process::ExitCode {
    rustcms::Console::<lms::database::migrations::Migrator>::new(lms::bootstrap::build).run_blocking()
}
