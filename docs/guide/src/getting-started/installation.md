# Installation

Install the `rustcms` command, create an application with it, and sign in to
its admin panel. This takes a few minutes, most of it the first build.

## Quick start

```sh
curl -fsSL https://irisvn.github.io/RustCMS/install.sh | sh
export PATH="$HOME/.rustcms/bin:$PATH"   # as the script prints; add it to your shell profile
rustcms new blog
cd blog
rustcms migrate                    # creates database.sqlite (DATABASE_URL in .env)
rustcms make:user --email you@example.com --name You --super-admin
rustcms serve                      # http://localhost:8000/admin
```

![The sign-in page of the admin panel](/img/getting-started/sign-in.webp)

*The demo application's sign-in page at `/admin` (password reset is enabled
there, hence "Forgot password?"; a new application does not show it).*

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

`rustcms doctor` checks this machine (Rust version, C compiler, access to
RustCMS-Core) and says what to install.

## Installing the `rustcms` command

```sh
curl -fsSL https://irisvn.github.io/RustCMS/install.sh | sh
```

The script picks the build for your system from the
[releases](https://github.com/irisvn/RustCMS/releases), checks it against the
release's `SHA256SUMS`, and installs it as `~/.rustcms/bin/rustcms`. It prints
the line to add to your shell profile, or adds it itself with
`sh -s -- --modify-path`.

| Variable | Default | |
|---|---|---|
| `RUSTCMS_VERSION` | the latest release | a release tag, e.g. `v0.4.0`; release candidates (`v0.4.0-rc.1`) are only installed this way |
| `RUSTCMS_INSTALL` | `~/.rustcms` | the install root: the command goes in its `bin/` (`/opt/rustcms` → `/opt/rustcms/bin/rustcms`) |

```sh
curl -fsSL https://irisvn.github.io/RustCMS/install.sh | RUSTCMS_VERSION=v0.10.3 sh
```

- Builds exist for macOS on Apple silicon (11 or newer) and Linux (x86_64 and
  arm64, any distribution: the binary is static). An x86_64 shell running
  under Rosetta on Apple silicon gets the native build. On an Intel Mac, build
  the command from source with Cargo:
  `cargo install --git https://github.com/irisvn/RustCMS-Core --tag v0.10.3 rustcms-cli`.
- To read the script first: `curl -fsSLO https://irisvn.github.io/RustCMS/install.sh`,
  then `sh install.sh`. Or download an archive from the releases page and put
  `rustcms` on your `PATH` yourself.

::: warning Checksums are not signatures
The checksum detects a corrupted download; it does not prove who published the
release. Releases are not code-signed yet: an archive downloaded with a browser
may be blocked by macOS (`xattr -d com.apple.quarantine rustcms`). The install
script is not affected.
:::

### Keeping it up to date

| Command | Does |
|---|---|
| `rustcms self-update` | replaces the command with the latest release; `--version v0.4.0` installs a given one (it may be older). Only for a command installed by the script |
| `rustcms upgrade` | moves the application in the current directory to the command's version (`Cargo.toml` and `Cargo.lock`); `--dry-run` shows the changes, `--force` allows going back to an older version |
| `rustcms docs` | opens this guide for the command's version; `--print-path` prints its URL instead |
| `rustcms doctor` | checks that this machine can build an application |

Run `self-update` first, then `upgrade` in each application. The
[upgrade guide](../upgrade.md) lists what changes between versions.

## Creating an application

```sh
rustcms new blog                       # in the current directory
rustcms new blog --path ~/code         # in ~/code/blog
```

`rustcms new` writes the project (see [Directory structure](directory-structure.md)),
then resolves its dependencies (`Cargo.lock`). Its `Cargo.toml` depends on the
RustCMS-Core tag of the command's version:

```toml
rustcms = { git = "https://github.com/irisvn/RustCMS-Core", tag = "v0.10.3" }
```

| Option | |
|---|---|
| `--path <dir>` | the parent directory (default: the current one) |
| `--no-fetch` | do not resolve the dependencies now (offline); Cargo does it on the first build |
| `--in-workspace` | do not declare a standalone `[workspace]`, for an application inside another Cargo workspace (dependencies are not resolved either) |

A tag that does not exist, or dependencies that do not resolve, fail the
command; the project is left in place to inspect. The first build compiles
everything and takes a few minutes; later builds only compile your code.

Commit `Cargo.lock`: it pins the exact commit and every dependency, so your
team and your CI build what you tested (the generated `.github/workflows/ci.yml`
runs `cargo test --locked`).

### The `.env` file

`rustcms new` writes `.env` with a fresh `APP_KEY`, and `.env.example` without
it:

```sh
APP_NAME="blog"
APP_ENV=local
APP_DEBUG=true
APP_URL=http://localhost:8000
APP_KEY=base64:…

DATABASE_URL=sqlite://database.sqlite?mode=rwc
```

`.env.example` also lists a PostgreSQL and a MySQL `DATABASE_URL` to start
from (a MySQL database needs `CHARACTER SET utf8mb4 COLLATE utf8mb4_0900_bin`;
see [MySQL](../basics/database.md#mysql)). `.gitignore` already excludes `.env`,
`*.sqlite` and `/storage`.

::: danger
Never commit `.env`: `APP_KEY` signs admin sessions and JWTs and encrypts your
secrets. A teammate copies `.env.example` to `.env` and runs
`rustcms key:generate`.
:::

### Signing in

Open **http://localhost:8000/admin** and sign in with the user you created.
Use the host of `APP_URL`: sign-in and every change are only accepted from that
origin. Opening `http://127.0.0.1:8000/admin` shows the page, but sign-in
answers 403; list other origins in `auth.admin_origins`
([Configuration](configuration.md)).

### `rustcms` inside an application

Inside an application, `rustcms <command>` runs your application's binary
(`cargo run -- <command>`), so your own commands are available too. The
command's own names (`new`, `make:*`, `doctor`, `upgrade`, `self-update`,
`docs`, `docs:cli`) come first; `rustcms -- <command>` always reaches the
application. Once built, the binary takes the same commands: `./blog migrate`,
`./blog serve`. Every command is listed in the [CLI reference](../reference/cli.md).

## Your first resource

```sh
rustcms make:model Post -m                 # src/models/post.rs + a create_posts_table migration
# edit the model and the migration, then:
rustcms migrate
rustcms make:resource Post --generate      # src/admin/resources/post_resource.rs: fields, form, table
rustcms permission:sync                    # declares posts.view_any, posts.create, …
rustcms serve
```

`--generate` writes fields, form, table and filters from the model's columns.
For a foreign key, `--belongs-to category_id=categories` (repeatable) makes the
column a select of the `categories` resource.

Posts now appear in the panel for super admins. Give other users access with
roles (System → Roles, see [Users & roles](../admin/users-roles.md)).


## Pitfalls

- **Sign-in answers 403** on `127.0.0.1` or another host: use the host of
  `APP_URL`, or add the origin to `auth.admin_origins`.
- **`rustcms self-update` refuses**: the command was not installed by the
  script (Cargo or a manual copy). Update it the way you installed it
  (`cargo install --git … --tag <version> rustcms-cli`), or switch to the
  script.
- **The first build fails to fetch RustCMS-Core**: check access to GitHub, or
  create the project with `--no-fetch` and build when online.

## Next steps

- [Configuration](configuration.md) and [directory structure](directory-structure.md)
- [Resources](../admin/resources.md): the heart of the admin panel
- [Routing](../basics/routing.md) for your own JSON API
- [Deployment](deployment.md) when you are ready to ship
