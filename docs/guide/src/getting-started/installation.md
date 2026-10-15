# Installation

## Requirements

- Rust (stable, 1.94 or newer).
- A database: SQLite works out of the box; PostgreSQL or MySQL 8.4 for
  production.

## Creating an application

RustCMS is not published on crates.io yet. Install the `rustcms` command from
the Git URL or archive you were given:

```sh
cargo install --git <rustcms-git-url> rustcms-cli    # the `rustcms` command
rustcms new blog
cd blog
rustcms migrate                    # SQLite file by default (DATABASE_URL in .env)
rustcms make:user --email you@example.com --name You --super-admin
rustcms serve                      # http://localhost:8000/admin
```

`rustcms new` writes a fresh `APP_KEY` to `.env`. Never commit `.env`.

Open **http://localhost:8000/admin** and sign in. Use the host of `APP_URL`:
sign-in and every change are only accepted from that origin (opening
`http://127.0.0.1:8000/admin` shows the page but sign-in answers 403; list
other origins in `auth.admin_origins`).

Inside an application, `rustcms <command>` runs your application's binary
(`cargo run -- <command>`), so your own commands are available too. Once
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
