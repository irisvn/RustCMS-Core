//! The target the command is built for (`self-update` picks its archive) and,
//! for release builds, the guide to embed (`rustcms docs`).

use std::path::Path;

fn main() {
    println!("cargo::rustc-check-cfg=cfg(rustcms_docs)");
    println!("cargo::rerun-if-env-changed=RUSTCMS_DOCS_DIR");
    println!("cargo::rerun-if-env-changed=RUSTCMS_RELEASE");
    println!("cargo::rustc-env=RUSTCMS_TARGET={}", std::env::var("TARGET").expect("TARGET"));
    match std::env::var("RUSTCMS_DOCS_DIR") {
        Ok(dir) if !dir.is_empty() => {
            let dir = Path::new(&dir).canonicalize().unwrap_or_else(|e| panic!("RUSTCMS_DOCS_DIR {dir}: {e}"));
            assert!(dir.join("index.html").is_file(), "RUSTCMS_DOCS_DIR has no index.html: {}", dir.display());
            println!("cargo::rerun-if-changed={}", dir.display());
            println!("cargo::rustc-env=RUSTCMS_DOCS_DIR={}", dir.display());
            println!("cargo::rustc-cfg=rustcms_docs");
        }
        // A release always ships its guide.
        _ => assert!(std::env::var("RUSTCMS_RELEASE").is_err(), "a release build needs RUSTCMS_DOCS_DIR"),
    }
}
