fn main() -> std::process::ExitCode {
    rustcms::Console::<demo::migrations::Migrator>::new(demo::app::build).run_blocking()
}
