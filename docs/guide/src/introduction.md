# Introduction

RustCMS is a Rust framework for admin panels, back-offices and headless CMS
backends. You describe your data in Rust; RustCMS serves an authorized admin
panel and a JSON API for it, compiled into one binary. It takes its ideas from
**Laravel** (applications, service providers, an artisan-like console,
migrations, testing helpers) and **Filament** (resources that describe forms,
tables and actions, rendered as a complete admin panel).

![The posts table of a RustCMS admin panel](/img/introduction/posts-list.webp)

*RBlog, a blog built on RustCMS: the posts table.*

## A resource in a few lines

A resource names a SeaORM entity and declares which columns the panel shows and
how:

<<< @/../../../examples/demo/src/admin/resources/post_resource.rs#intro-post [src/admin/resources/post_resource.rs]

From these declarations RustCMS builds the list (search, filters, sorting,
export), validation, and the permissions `posts.view_any`, `posts.create`, … .
The layout of the form (tabs, the cover picker, the rich-text and key/value
editors) is declared in `form()`:

<<< @/../../../examples/demo/src/admin/resources/post_resource.rs#form [src/admin/resources/post_resource.rs]

Only declared fields the user may read appear in the record data the panel
receives (the record's key is sent beside them), and the panel's own forms
only accept declared fields the user may write. Rules can depend on the user:

```rust
Field::column(post::Column::Published).updatable(|ctx| ctx.can("posts.publish"))
```

![The post form](/img/introduction/post-form.webp)

*RBlog: a post form with publishing sections, a cover-image picker and rich text.*

No Node, PHP or separate frontend is deployed: the panel is built into the
binary. Start with [Installation](getting-started/installation.md).

## What you get

- **Admin panel**: [resources](admin/resources.md) with lists (search, filters,
  sorting, [row and bulk actions](admin/actions.md)) and forms (sections, tabs,
  relationships, file uploads, rich text, key/value inputs); soft deletes;
  has-many and belongs-to-many [relationships](admin/relationships.md);
  [custom field widgets](admin/custom-fields.md);
  [dashboard widgets](admin/dashboard.md);
  [CSV import and export](admin/import-export.md); a
  [media library](admin/media.md); [settings](admin/settings.md) edited in the
  panel; users, roles and account pages, with a Roles page you can organize
  ([Users & roles](admin/users-roles.md)); English and Vietnamese
  ([Languages](admin/i18n.md)).
- **Your own screens in the panel**: [custom pages and slots](admin/customizing.md)
  from HTML templates, a theme and runtime branding,
  [React pages](admin/react-pages.md), [JSON routes on the admin session](admin/admin-routes.md),
  and [realtime](admin/realtime.md) event streams and WebSockets.
- **Security by default**: everything is denied until a permission grants it.
  The panel's resource operations (lists, forms, relationships, exports,
  actions) enforce resource and record authorization,
  [scopes](admin/authorization.md) (tenancy, ownership) and the field
  restrictions you configure, on the server. Custom action handlers and your
  own API handlers write with your code: they must enforce their own field
  and business rules. Admin sessions with CSRF and origin checks; API tokens
  and JWT with immediate revocation; rate limits
  ([Authentication](security/authentication.md)).
- **Application layer**: typed [configuration](getting-started/configuration.md),
  [service providers and a container](architecture/providers.md), an HTTP stack
  with limits on body size and time, JSON APIs where every route declares its
  access ([Routing](basics/routing.md)), validation and pagination; SQLite,
  PostgreSQL and MySQL; [file storage](digging-deeper/storage.md) on disk or
  S3-compatible; [several instances](digging-deeper/instances.md) with Redis.
- **Background work and records**: [queues and jobs](digging-deeper/queues.md)
  and [schedules](digging-deeper/scheduling.md), each with its page under
  System; [mail](digging-deeper/mail.md); an
  [audit log](digging-deeper/audit-log.md) of the changes made through the
  panel (on by default per resource; writes from your API, jobs and services
  are recorded when your code records them); and
  [encrypted columns](digging-deeper/encryption.md) for your secrets.
- **Tooling**: `rustcms new`, generators (`make:model`, `make:resource`,
  `make:api`, `make:page`, …), migrations, and a [testing kit](basics/testing.md)
  that runs every database test on SQLite, and on PostgreSQL and MySQL when
  `RUSTCMS_TEST_PG` / `RUSTCMS_TEST_MYSQL` name a server.

![The dashboard of the admin panel](/img/introduction/dashboard.webp)

*The demo application: a dashboard with a stat, a chart and the latest posts.*

## Coming from Laravel and Filament

| Laravel / Filament | RustCMS |
|---|---|
| `laravel new` | `rustcms new` |
| `php artisan …` | `rustcms …` (or `cargo run -- …`, or your app's binary) |
| `config/*.php` + `.env` | `config/*.toml` + `.env` |
| Service providers, container | `ServiceProvider` (`register` / `boot`), `AppBuilder::bind`, `Inject<T>` |
| Routes, controllers, form requests, API resources | `ApiRoutes` + handlers, `Validated<T>`, JSON structs |
| Eloquent models, migrations | SeaORM entities, migrations (`make:model`, `make:migration`) |
| Gates, policies, Spatie permissions | `Gate`, `Policy`, roles and permissions per guard |
| Filament resources, forms, tables, actions | `Resource`, `Form`, `Table`, `Action` |
| Filament relation managers | `Relation::has_many`, `Relation::belongs_to_many` (and `BelongsToManySelect`) |
| Filament custom pages | `make:page`: an HTML template, a script or a React component |
| Scheduler (`schedule:run`) | `AppBuilder::schedule`, run by `serve` |
| `Illuminate\Testing` | `rustcms-testing` (`TestApp`, `db_test!`, factories) |

## Status

RustCMS is pre-1.0: APIs may change between minor versions; the
[upgrade guide](upgrade.md) lists what to change. See
[Limitations](reference/limitations.md) for what is not supported yet.

## Related pages

- [Installation](getting-started/installation.md)
- [Directory structure](getting-started/directory-structure.md)
- [Resources](admin/resources.md)
- [Authorization](admin/authorization.md)
