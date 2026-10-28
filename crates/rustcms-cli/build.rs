//! The target the command is built for (`self-update` picks its archive).

fn main() {
    println!("cargo::rustc-env=RUSTCMS_TARGET={}", std::env::var("TARGET").expect("TARGET"));
}
