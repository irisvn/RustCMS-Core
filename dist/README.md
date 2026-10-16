# RustCMS

Build admin panels and headless CMS backends in Rust. This repository holds the
`rustcms` command (see [Releases](https://github.com/irisvn/RustCMS/releases)),
its install scripts and the [guide](https://irisvn.github.io/RustCMS/). The
framework's source is [irisvn/RustCMS-Core](https://github.com/irisvn/RustCMS-Core).

## Install

Requires [Rust](https://rustup.rs) (stable, 1.94 or newer) and a C compiler:
Xcode Command Line Tools on macOS (`xcode-select --install`), `build-essential`
(or equivalent) on Linux, Visual Studio Build Tools ("Desktop development with
C++") on Windows.

macOS and Linux:

```sh
curl -fsSL https://irisvn.github.io/RustCMS/install.sh | sh
```

Windows (PowerShell):

```powershell
irm https://irisvn.github.io/RustCMS/install.ps1 | iex
```

The scripts download the release for your system and check it against the
release's `SHA256SUMS`. You can also download an archive from
[Releases](https://github.com/irisvn/RustCMS/releases) yourself, or read the
script before running it (`curl -fsSLO …/install.sh`, then `sh install.sh`).

## Start an application

```sh
rustcms new blog
cd blog
rustcms migrate
rustcms make:user --email you@example.com --name You --super-admin
rustcms serve           # http://localhost:8000/admin
```

`rustcms new` makes a Rust project whose `Cargo.toml` depends on the matching
tag of RustCMS-Core; Cargo fetches it from GitHub. Commit `Cargo.lock`.

Read the [guide](https://irisvn.github.io/RustCMS/) next.

---

This repository is published from RustCMS-Core by its release workflow: report
issues and send changes there.

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
