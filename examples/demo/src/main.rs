// #region main
fn main() -> std::process::ExitCode {
    rustcms::Console::<demo::database::migrations::Migrator>::new(demo::bootstrap::build).run_blocking()
}
// #endregion main
