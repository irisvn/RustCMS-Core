# Installation

## Requirements

- **Rust**, stable 1.94 or newer, from [rustup.rs](https://rustup.rs). Your
  application is a Rust project: Cargo builds it together with RustCMS.
- **A C compiler**, for the few dependencies with C code (SQLite, TLS):
  - macOS: Xcode Command Line Tools (`xcode-select --install`);
  - Linux: `build-essential` on Debian/Ubuntu, `gcc` or `clang` elsewhere.
- **macOS or Linux.** Windows is not supported: use WSL (Ubuntu), where the
  Linux build works.
- **Network access to GitHub** for the first build: applications depend on
  [RustCMS-Core](https://github.com/irisvn/RustCMS-Core) through a Git tag.
- A database: SQLite works out of the box; PostgreSQL or MySQL 8.4 for
  production. Node.js is not needed (the admin panel is built in).

## Installing the `rustcms` command

```sh
curl -fsSL https://irisvn.github.io/RustCMS/install.sh | sh
```

The script picks the build for your system from the
[releases](https://github.com/irisvn/RustCMS/releases), checks it against the
release's `SHA256SUMS`, and installs it in `~/.rustcms/bin`. It prints the
line to add to your shell profile, or adds it with `sh -s -- --modify-path`.
`rustcms self-update` updates it later; `rustcms doctor` checks that this
machine can build an application (Rust version, C compiler, access to
RustCMS-Core) and says what to install; `rustcms docs` opens this guide for
your version, offline.

- A given version: `RUSTCMS_VERSION=v0.4.0` (release candidates are only
  installed this way). Another directory: `RUSTCMS_INSTALL=/opt/rustcms`.
- To read the script first: `curl -fsSLO https://irisvn.github.io/RustCMS/install.sh`,
  then `sh install.sh`. Or download an archive from the releases page and put
  `rustcms` on your `PATH` yourself.
- The checksum detects a corrupted download; it does not prove who published
  the release. Releases are not code-signed yet: an archive downloaded with a
  browser may be blocked by macOS (`xattr -d com.apple.quarantine rustcms`).
  The install script is not affected.
- Built for macOS on Apple silicon (11 or newer) and Linux (x86_64 and arm64,
  any distribution: the binary is static). On an Intel Mac, build it from
  source with Cargo (below).
- Without the script, Cargo can build it from source:
  `cargo install --git https://github.com/irisvn/RustCMS-Core --tag v0.10.0 rustcms-cli`.

## Creating an application

```sh
rustcms new blog
cd blog
rustcms migrate                    # SQLite file by default (DATABASE_URL in .env)
rustcms make:user --email you@example.com --name You --super-admin
rustcms serve                      # http://localhost:8000/admin
```

`rustcms new` writes a project whose `Cargo.toml` depends on the RustCMS-Core
tag of the command's version, then resolves the dependencies (`Cargo.lock`):

```toml
rustcms = { git = "https://github.com/irisvn/RustCMS-Core", tag = "v0.10.0" }
```

Commit `Cargo.lock`: it pins the exact commit and every dependency, so your
team and your CI build what you tested (the generated `.github/workflows/ci.yml`
runs `cargo test --locked`). Offline, `rustcms new blog --no-fetch` skips the
resolution; Cargo does it on the first build. A tag that does not exist, or
dependencies that do not resolve, fail the command (the project is left in
place to inspect). The first build compiles everything and takes a few minutes;
later builds only compile your code.

`rustcms new` writes a fresh `APP_KEY` to `.env`. Never commit `.env`.

Open **http://localhost:8000/admin** and sign in. Use the host of `APP_URL`:
sign-in and every change are only accepted from that origin (opening
`http://127.0.0.1:8000/admin` shows the page but sign-in answers 403; list
other origins in `auth.admin_origins`).

Inside an application, `rustcms <command>` runs your application's binary
(`cargo run -- <command>`), so your own commands are available too. The
command's own names (`new`, `make:*`, `doctor`, `upgrade`, `self-update`,
`docs`) come first; `rustcms -- <command>` always reaches the application. Once
built, the binary takes the same commands: `./blog migrate`, `./blog serve`.

## Your first resource

```sh
rustcms make:model Post -m                 # src/models/post.rs + a create_posts_table migration
# edit the model and the migration, then:
rustcms migrate
rustcms make:resource Post --generate      # src/admin/resources/post_resource.rs: fields, form, table
rustcms permission:sync                    # declares posts.view_any, posts.create, …
rustcms serve
```

Posts now appear in the panel for super admins. Give other users access with
roles (System → Roles).

## Next steps

- [Configuration](configuration.md) and [directory structure](directory-structure.md)
- [Resources](../admin/resources.md): the heart of the admin panel
- [Routing](../basics/routing.md) for your own JSON API
- [Deployment](deployment.md) when you are ready to ship
