# Introduction

rustcms is a Rust framework for building admin panels and headless CMS backends,
inspired by Laravel and FilamentPHP. You describe *resources* in Rust (fields,
permissions, hooks, actions); rustcms serves an authorized CRUD API and a React
admin panel generated from that description.

> **Status:** pre-release (milestone M1). APIs will change until 0.1.

| Laravel                     | rustcms                                      |
|-----------------------------|----------------------------------------------|
| `laravel new`               | `rustcms new`                                |
| `php artisan …`             | `rustcms …` (forwarded to your app binary)   |
| `config/*.php` + `.env`     | `config/*.toml` + `.env`                     |
| Service providers           | `ServiceProvider` (`register` / `boot`)      |
| Service container           | `AppBuilder::bind` + `Inject<T>`             |
| Eloquent models             | SeaORM entities (`make:model`)               |
| Migrations                  | SeaORM migrations (`make:migration`)         |
| `Illuminate\Testing`        | `rustcms-testing`                            |
| Filament resources          | `rustcms::admin::Resource`                   |
