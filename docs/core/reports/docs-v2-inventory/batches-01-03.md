# Docs v2 inventory: batches 1–3 (source at d4e87e3)

Paths relative to the repo root; guide pages in `docs/guide/src/`.

## introduction.md
- Correct: every call in the sample exists (`crates/rustcms-admin/src/resources/resource.rs:146,169,192`, `field.rs:129,159,165`, `engine/context.rs:176`).
- Missing: `AdminRoutes`/`AdminAccess` (ADR 0026, `examples/demo/src/routes/admin.rs`); custom pages, slots, theme, runtime branding (ADR 0019/0033, `examples/demo/src/providers/admin_panel_provider.rs:15,42-46`); System → Queue and Schedules pages; Roles-page catalog (`permission_area/meta/preset`, `crates/rustcms-auth/src/provider.rs:88-116`); media library `AdminProvider::media()`.
- Examples: `examples/demo/src/admin/resources/post_resource.rs` (`meta` :22, `fields` :26) — needs a region; real meta adds `.icon()` `.exportable()`.
- Screenshots: hero = posts list, post form, dashboard (RBlog nicer; local demo has all); RDockPanel for "back-offices".

## getting-started/installation.md
- Wrong: line 33 `RUSTCMS_INSTALL` is the install root, the command goes to its `bin/` (`dist/install.sh:9,21`).
- Missing: `rustcms new --path`, `--in-workspace` (`crates/rustcms-cli/src/main.rs:38-46`); `upgrade --dry-run/--force` (:155-162); `self-update --version` (:165-169); `docs --print-path` (:171-175); `make:resource --belongs-to field=slug` (:68-70); new `.env` has `APP_ENV=local`, `APP_DEBUG=true`; `.env.example` lists PG/MySQL URLs + MySQL collation note; SQLite file `database.sqlite`; Rosetta shell gets native build (`dist/install.sh:59-64`).
- Screenshots: sign-in page, empty panel, Posts after `make:resource` (fresh app/local demo).
- Linked anchors: `#installing-the-rustcms-command` (upgrade.md:7).

## getting-started/configuration.md
- Wrong: line 7 list misses `APP_PREVIOUS_KEYS`, `REDIS_URL` (`crates/rustcms-core/src/config.rs:249-258`); `app.debug` is read by no framework code (`config.rs:49,252`) — page and `security/production.md:3` imply it matters.
- Defaults verified correct (config.rs:119-127, auth config.rs:70-118, branding.rs:284-302, audit.rs:43-65, redis.rs:14-22,110-121, realtime/mod.rs:46-81, schedule/mod.rs:418-448).
- Missing: `app.locale` ∈ `app.locales`, tags validated, boot fails (`config.rs:283-292`); `app.env` effects: production refuses `migrate:fresh/refresh`, `db:seed` without `--force` (`console.rs:140,157-175`), mail `log_body` only in local (`mail/mod.rs:155`), template reload in local debug builds (`mail/template.rs:137`, `panel/provider.rs:290`); `auth.admin_path` segment rules + trailing `/` stripped (`auth/config.rs:123-137`); boot panics on out-of-range `auth.invitation_days` (`provider.rs:136-140`) and JWT bounds (`config.rs:78-89`); `[realtime]` values clamped (`realtime/mod.rs:101-106`); `redis.prefix` non-empty, no spaces; connect timeout 2 s/5 s; `Config::with` (`config.rs:310`), `figment`/`from_figment` (:234,280), `Environment` enum (:33); template `config/*.toml` defaults.
- Anchors: none incoming; keep section ids if possible.

## getting-started/directory-structure.md
- Wrong: `storage/` not created by `rustcms new` (no StorageProvider in template); tree misses `README.md`, `src/routes/mod.rs`, `src/admin/mod.rs`, `src/http/mod.rs`, `src/database/mod.rs`, `tests/feature/{main,health}.rs`, `database.sqlite`; lines 78-84 `admin_panel_provider.rs` snippet ≠ template (no `.resource` before anchors, third anchor `// rustcms:extensions`); anchor list is "…" — real anchors: `rustcms:api-routes, commands, extensions, pages, resources, jobs, seeders, mods, models, migrations, policies, services, settings, tests, factories, seeder-calls`, `*-mods`.
- Missing: `make:page --react`, `make:sdk` (`main.rs:132-146`); `src/routes/admin.rs` for AdminRoutes; settings group name rule 1-32 of `a-z0-9_` (`make.rs:136`); script page name rules (`make.rs:224-230`).
- Examples: `examples/demo/src/bootstrap/mod.rs` `build`; `examples/lms/src/bootstrap/mod.rs` (= template); `examples/demo/src/main.rs`; demo `admin_panel_provider.rs` `provider`; `examples/lms/src/services/enrollment_service.rs`.

## getting-started/deployment.md
- Correct: feature table (`crates/rustcms/Cargo.toml:13-23`), `--allow-newer` flow.
- Wrong/incomplete: line 131 graceful stop bounded by `realtime.shutdown_grace_secs` (5), then connections dropped (`app.rs:416-418,455-466`), queue workers drain up to `queue.shutdown_secs`; line 85 `/health` failure = 503 `{"status":"degraded","database":"unavailable"}` (`http/mod.rs:180-185`).
- Missing: `serve` runs queue workers (`queue.enabled`, `workers` 2) and the scheduler (`app.rs:407-422`); `serve --listen`; production `--force` guards; APP_KEY rotation link (`operations/app-key.md`); `*_FILE` secrets; MySQL setup check (`db/check.rs:4-16`); `.dockerignore` hint.
- Linked anchors: `#upgrading` (database.md:118, configuration.md:28, release-notes.md:98, production.md:29, upgrade.md:143).

## architecture/lifecycle.md
- Wrong: line 11 "provider boot starts background jobs" — workers and scheduler start in `App::serve_on` after boot, only for `serve` (`app.rs:401-422`, ADR 0031); only AuthProvider's prune loop runs in boot. Lines 7-12 start sequence misses MySQL check (`app.rs:342`), Redis connect (:343-357), queue/schedule validation (:357-358), keyring (:365), boot (:366-368), router (:369); logging right after config load (`console.rs:139`). Line 15: `key:generate` runs before any config is loaded (`console.rs:136-138`). Line 16: `migrate*` connect + MySQL check, `migrate:status` only connects, `db:seed` and app commands build and boot (`console.rs:146-225`). Lines 24-25: request id kept only if 1-64 chars `[A-Za-z0-9._:-]` and sent once, else new UUID (`http/mod.rs:225-240`). Guard: admin asset paths skip the session; admin responses `Cache-Control: private, no-store` (`guards/guard.rs:101-121`).
- Missing: `AppBuilder::middleware` (`app.rs:217-225`); `rustcms::http::request_deadline()` (`http/mod.rs:154-159`); `/health` 503 body; malformed XFF fallback (`http/mod.rs:92-101`); 408 empty body (`http/mod.rs:213`); `AdminAccess` for panel JSON routes; shutdown sequence (`app.rs:423-488`); `App::serve_on`, `build_with_db`.
- Linked anchors: `#client-ip-and-proxies` (security/authentication.md:163).

## architecture/providers.md
- Wrong: text says three providers (template) but the imported demo region registers `StorageProvider`, `MailProvider` and takes `admin: &AdminRoutes` — explain or import lms; line 6 omits `MailProvider`; line 71 `describe_routes(source, routes)` with `(Method, String)` pairs (`app.rs:174-186`); line 23 when `boot` runs: `serve`, `db:seed`, `migrate:fresh --seed`, app commands — not `migrate*`, `migrate:status`, `route:list`.
- Missing: `AppBuilder::middleware` (:219), `shutdown()` (:202, demo Ticker), `add_encrypted_column`, `encrypted_columns`, `seeders()`, `commands()`, `described_routes()` (:188,270,276,309,320), `build`, `build_with_db` (:332,338), `AppState::keyring()` (:49), `bind` replaces earlier binding (:206), `AppCommand` trait (:96-103), ADR 0031 rule (schedule, never spawn loops), `AuthProvider` builders (`provider.rs:53-127`).
- Examples: demo `app_service_provider.rs` (bind, shutdown, seeders); lms `app_service_provider.rs` (job, mail_template); demo `auth_service_provider.rs`, `admin_panel_provider.rs`; demo `http/controllers/live_controller.rs` (`Inject<Ticker>`); demo `jobs/post_digest.rs` `schedule`.

## basics/routing.md
- Wrong: line 31 `show` uses `Access::can("posts.view")` (make:api, `crates/rustcms-cli/src/api.rs:173`); lines 17-18 excluded columns: `password, secret, token, hash, api_key, salt` (`api.rs:22`) + unsupported types (`api.rs:229`); template wires `api_permissions` through `providers::auth_service_provider::provider(api)`.
- Missing: `.put/.patch/.delete` (`crates/rustcms-auth/src/http/api.rs:140-146`); `scope()`, `.nest(prefix, routes)` (:137,151); `.list()`, `.permissions()` (:172,177); panics (duplicate route :203, bad `Access::can` :69-77, paths not starting with `/` :132,152,161); `Throttle<N>` 401 without token (:255-273); `MaybeAuth` (`guards/guard.rs:79`); `body_limit`, `middleware`; prelude exports (`crates/rustcms/src/lib.rs:35-47`).
- Examples: demo `routes/api.rs` `routes`; `http/controllers/post_controller.rs` `index`/`store`; `http/resources/post_resource.rs` `PostJson`; `tests/feature/api.rs` (401/403).
- Linked anchors: `#access` (security/authorization.md:4).

## basics/validation.md
- Small: not JSON or missing `Content-Type` → 400 (415 mapped, `http/mod.rs:49-58`).
- Missing: `Pagination::new/from_query/page/per_page` (`crates/rustcms-core/src/http/api.rs:47-93`); `MAX_PAGE`, `MAX_PER_PAGE`, `DEFAULT_PER_PAGE` (:24-28); `ApiPageMeta` (:107); page past last → empty data, `lastPage` ≥ 1 (:129-131,152); 422 messages (:51,78,85); `into_result`/`field` (cross-link errors); Throttle key = user + matched route, 401 without token; 413 via `body_limit`.
- Examples: demo `http/requests/post_request.rs` `NewPost` + `impl Validate`; `post_controller.rs` `store`, `index`.

## basics/errors.md
- Wrong: "every error answers JSON" — 408 empty body (`http/mod.rs:213`), `/health` 503 own body; `errors/formErrors/issues/formIssues` only on 422, `reason` only on 409 (`error.rs:227-276`); retryable conversion also covers busy SQLite and serialization failures, `reason: "busy"`; PG 55P03/40P01/40001, MySQL 1205/1213/3572 (`error.rs:168-225`).
- Missing: `AppError::internal`, `.status()`, `.code()` (:122-160); `ValidationErrors::is_empty/issues/form_issues` (:41-61); `rustcms::i18n::Issue` (`i18n/issues.rs:17-37`, code `custom`); fixed messages per variant (:247-262); `BadRequest(detail)` is shown (no secrets); 408 row; app-defined `Conflict.code`.
- Screenshot: form with field + form-level errors (local demo).

## No page at all
- `AppBuilder::middleware`; logging/observability (`[log]`, `RUST_LOG`, request span, `/health`); `request_deadline()`; `app.env` effects (and `app.debug` has no effect); process model (which commands boot, `serve` runs workers + scheduler, grace periods); embedding/testing entry points (`serve_on`, `build_with_db`, `Config::with`); `ApiRoutes::nest/scope`, `put/patch/delete`.
