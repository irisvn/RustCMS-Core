# Introduction

RustCMS is a Rust framework for building admin panels, back-offices and
headless CMS backends. It takes its ideas from **Laravel** (applications,
service providers, an artisan-like console, migrations, testing helpers) and
**Filament** (resources that describe forms, tables and actions, rendered as a
complete admin panel).

You describe your data in Rust:

```rust,ignore
impl Resource for PostResource {
    type Entity = post::Entity;
    const SLUG: &'static str = "posts";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Post").title("title").group("Content").soft_deletes()
    }

    fn fields() -> Vec<Field> {
        vec![
            Field::column(post::Column::Title).required().searchable().sortable(),
            Field::column(post::Column::CategoryId).belongs_to("categories").filterable(),
            Field::column(post::Column::Published).updatable(|ctx| ctx.can("posts.publish")),
        ]
    }
}
```

and RustCMS serves an authorized admin panel and JSON API for it, compiled into
one binary: no Node, PHP or separate frontend to deploy.

## What you get

- **Admin panel**: lists with search, filters, sorting, and
  [row and bulk actions](admin/actions.md); forms with sections, tabs,
  relationships, file uploads, rich text and key/value inputs; soft deletes;
  has-many and belongs-to-many relations; custom field widgets;
  [dashboard widgets](admin/dashboard.md);
  [CSV import and export](admin/import-export.md);
  [React pages](admin/react-pages.md) and [realtime](admin/realtime.md)
  events and sockets; [settings](admin/settings.md) edited in the panel;
  users, roles and account pages; English and Vietnamese.
- **Security by default**: deny-by-default permissions per resource, record and
  field; scopes (tenancy, ownership) applied on every path; admin sessions with
  CSRF and origin checks; API tokens and JWT with immediate revocation; rate
  limits.
- **Application layer**: typed configuration, service providers and a
  container, an HTTP stack with sane limits, JSON APIs with declared access,
  validation and pagination, SQLite, PostgreSQL and MySQL, file storage (local
  or S3-compatible), several instances with Redis.
- **Background work and records**: [queues and jobs](digging-deeper/queues.md),
  [schedules](digging-deeper/scheduling.md), [mail](digging-deeper/mail.md),
  an [audit log](digging-deeper/audit-log.md) of every change, and
  [encrypted columns](digging-deeper/encryption.md) for your secrets.
- **Tooling**: `rustcms new`, generators (`make:model`, `make:resource`,
  `make:api`, …), migrations, and a testing kit that runs every database test
  on all three databases.

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
| `Illuminate\Testing` | `rustcms-testing` (`TestApp`, `db_test!`, factories) |

## Status

RustCMS is pre-1.0: APIs may change between minor versions; the
[upgrade guide](upgrade.md) lists what to change. See
[Limitations](reference/limitations.md) for what is not supported yet.
