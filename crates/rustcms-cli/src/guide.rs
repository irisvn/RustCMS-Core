//! `rustcms docs`: opens the published guide of this version
//! (`https://irisvn.github.io/RustCMS/v/X.Y.Z/`). The guide is not embedded.

use std::process::{Command, ExitCode};

use crate::release;

pub fn run(print_path: bool) -> anyhow::Result<ExitCode> {
    let target = url();
    if print_path {
        println!("{target}");
        return Ok(ExitCode::SUCCESS);
    }
    println!("Opening {target}");
    let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    let opened = Command::new(opener).arg(&target).status().is_ok_and(|s| s.success());
    if !opened {
        println!("Open it in a browser: {target}");
    }
    Ok(ExitCode::SUCCESS)
}

/// The guide of this version.
fn url() -> String {
    format!("{}/v/{}/", release::PAGES, release::VERSION)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_the_guide_of_this_version() {
        assert_eq!(url(), format!("https://irisvn.github.io/RustCMS/v/{}/", env!("CARGO_PKG_VERSION")));
    }
}
