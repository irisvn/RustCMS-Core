# Docs v2 inventory: batches 4 and 5

Commit: `d4e87e3`. Pages are under `docs/guide/src/`. Source paths are relative to the repo root.

---

### basics/database.md

- **Public API covered**: `DATABASE_URL`; `rustcms::models::user`; `rustcms::migration::{create_table, create_index, add_column, drop_column, drop_table, ColumnDefExt::{instant, instant_now, local_date_time}}`; `migrate`, `migrate --allow-newer`, `migrate:status`, `migrate:rollback --step N`, `migrate:refresh [--force]`, `migrate:fresh [--force] [--seed]`, `db:seed [--class] [--force]`; `rustcms::database::Seeder` (`name`, `run`); `make:model -m`, `make:migration`, `make:seeder`; `use_transaction() -> Some(true)`; `RbacService` (mentioned).
- **Wrong or outdated**
  - The model sample (`title`, `published_at`, `created_at`) is not what `make:model Post -m` writes. The generator writes only `id`, `created_at` and `updated_at` (`crates/rustcms-cli/templates/stubs/model.rs.tmpl:7-12`; migration `stubs/migration_create.rs.tmpl:21-27`). The sample also has no `updated_at`.
  - "`serve`, the `migrate` commands and the app builder refuse to start…": `migrate:status` connects without `db::check` (`crates/rustcms-core/src/console.rs:187-193`). Only `migrate`, `migrate:rollback`, `migrate:refresh`, `migrate:fresh` and the builder run it (`console.rs:148,154,169`; `db/mod.rs:60`; `app.rs:342`).
  - The page says "Keep `Some(true)` in your own migrations", but the examples do not: `examples/demo/src/database/migrations/m20261022_000001_create_tags.rs` and `m20261101_000001_create_integrations.rs`, and every LMS migration (`examples/lms/src/database/migrations/m20261020_00000{1..4}_*.rs`, `m20261026_000005_create_news_tables.rs`), have no `use_transaction`. Fix the examples before using them as snippets.
  - The seeding table leaves out the rule for seeder names: 1–64 of `a-z`, `0-9`, `_`, and registration panics on an invalid or repeated name (`crates/rustcms-core/src/database.rs:26,53-55`).
- **Missing**
  - Transactions and locks (public, and used by the queue, audit and resource pages): `rustcms::db::begin_write` (`db/transactions.rs:17`: SQLite `BEGIN IMMEDIATE`, MySQL `READ COMMITTED` and a 10 s lock wait), `begin_snapshot` (`:40`), `set_lock_wait` / `MYSQL_LOCK_WAIT_SECONDS` (`:7,:30`), `xact_lock` / `lock_slot` / `LOCK_SLOTS` (`db/locks.rs:7,20,46`). Note: on MySQL `xact_lock` only accepts names preseeded in `rustcms_locks` (`rbac`, `lineage:<slot>`, `idem:<slot>`; `locks.rs:12-14,34-35`). An app cannot use a name of its own there, and the guide never says so.
  - `rustcms::db::connect`, `connect_for_migrations` (one connection), `migrate_fresh`, `ping`, `check`, `MYSQL_COLLATION` (`db/mod.rs:22,30,49,58,73`; `db/check.rs:14`); `migrate_up`, `migration_status`, `unknown_migrations`, `MigrationSettings` (`db/migrations.rs:16,33,102,124`).
  - `[database]` keys `max_connections` (10), `log_statements`, `allow_newer_migrations` (`config.rs:71-78,123`; `db/migrations.rs:16-19`). They are on the configuration page but not linked from here.
  - `Factory` (`rustcms::database::Factory`: `definition(n)`, `try_create`, `try_create_many`, `create`, `create_many`; `database.rs:83-130`) and `make:factory` (written for seeders as well as tests). Today only testing.md mentions it, in one line.
  - `rustcms::database::call(state, &[…])` and `DEFAULT_SEEDER` (`database.rs:17,34`); `AppBuilder::seeder` / `seeders()` (`app.rs:303,309`).
  - The framework entities `rustcms::models::{audit_log, job, job_receipt, job_run, schedule_run, schedule_slot, schedule_state, setting, user}` (`crates/rustcms-core/src/models/mod.rs:3-11`). The page names only `user`.
  - `rustcms::migration::framework()` (`crates/rustcms/src/lib.rs:28`) and the `Migrator` layout of the template (`crates/rustcms-cli/templates/app/src/database/migrations/mod.rs.tmpl`).
  - `migrate:refresh` runs `check(db, false)` (database only), and `migrate:fresh` turns SQLite foreign keys off while it drops tables (`db/mod.rs:58-70`).
- **Examples available**
  - Migration: `examples/demo/src/database/migrations/m20261011_155203_create_posts_table.rs` (`Migration`, with `use_transaction`, `create_table`, `instant_now`); `m20261013_000001_add_deleted_at_to_posts.rs` (`add_column` + `instant`).
  - Migrator: `crates/rustcms-cli/templates/app/src/database/migrations/mod.rs.tmpl` (`Migrator`), or `examples/demo/src/database/migrations/mod.rs`.
  - Model: `examples/demo/src/models/post.rs` (`Model`).
  - Seeders: `examples/demo/src/database/seeders/mod.rs` (`DatabaseSeeder`); `user_seeder.rs` (`UserSeeder`, `refuse_in_production`, `account`: refusing in production and finding by natural key); `examples/lms/src/database/seeders/role_seeder.rs` (`RoleSeeder`: roles through `RbacService`).
  - Registration: `examples/demo/src/providers/app_service_provider.rs:18-21` (`app.seeder(...)`).
  - Factory: `examples/lms/src/database/factories/course_factory.rs` (`CourseFactory`).
  - None of these has `#region` markers yet. The only snippet in the guide today is `<<< …/providers.rs#register` in `architecture/providers.md:15`, so markers must be added.
- **Screenshots needed**: none (database and console only).
- **Linked anchors**: `#mysql` (`getting-started/configuration.md:25`, `reference/limitations.md:44`, and in-page at `:37`); `#seeding` (`architecture/providers.md:76`).

### basics/console.md

- **Public API covered**: `rustcms <command>` forwarding, `cargo run -- --help`, `rustcms --help`, `route:list`; `make:model -m`, `make:migration`, `make:resource --generate --belongs-to`, `make:policy`, `make:api`, `make:service`, `make:job`, `make:seeder`, `make:factory`, `make:settings`, `make:command`, `make:page [--script]`, `make:widget`, `make:test`; `AppCommand` (`definition`, `run`), `rustcms::clap`, `AppBuilder::command`; `config::Environment::Production`.
- **Wrong or outdated**
  - The page says nothing that is wrong. Small details are off:
    - "`cargo run -- <command>`": the tool runs `cargo run --quiet --package <pkg> --bin <pkg> -- …` in the project root, found from a subdirectory too (`crates/rustcms-cli/src/project.rs:43-50`).
    - "An `Err` … exits with status 1" holds (`console.rs:250-253`), but argument errors exit through clap (`err.exit()`, status 2; `console.rs:203-206`).
    - An unknown command lists the application's commands (`console.rs:199-202`).
- **Missing**
  - `make:page --react` and `make:sdk` (`docs/guide/src/reference/cli.md:123-135`; `crates/rustcms-cli/src/react.rs`).
  - `new` (`--path`, `--in-workspace`, `--no-fetch`), `doctor`, `upgrade [--dry-run] [--force]`, `self-update [--version]`, `docs [--print-path]`, `docs:cli` (`reference/cli.md:13-176`).
  - Framework runtime commands: `serve [--listen]` and `key:generate [--show] [--force]` (`console.rs:26-30,82-90`; it keeps other `.env` lines and replaces every dotenv form, `console.rs:255-307`). The page also lists none of the commands providers add: `make:user`, `role:assign`, `token:create`, `permission:sync`, `session:prune`, `audit:prune` (`crates/rustcms-auth/src/provider.rs:172-177`); `queue:*`, `schedule:*`, `key:reencrypt`, `media:prune`, `idempotency:prune`, `lang:missing`, `admin:branding`. A table that links to the CLI reference would cover them.
  - The entry point `rustcms::Console::<Migrator>::new(build).run_blocking()` / `run()` (`console.rs:105-132`; `templates/app/src/main.rs.tmpl`).
  - `rustcms::console::command()` (`console.rs:94`, the clap definition used by `docs:cli`); `AppBuilder::commands()` (`app.rs:320`).
  - The output of `route:list` (METHOD, PATH, SOURCE; `console.rs:178-186`) and `AppBuilder::describe_routes`.
  - `key:generate` runs without loading config or the database; every other command loads `Config` first (`console.rs:135-139`).
  - Commands boot every provider (`console.rs:207-208`), so provider boot side effects run too, for example the auth prune task (`provider.rs:203-232`).
- **Examples available**
  - `crates/rustcms-cli/templates/stubs/command.rs.tmpl` (the generated `AppCommand`).
  - No `AppCommand` exists in `examples/demo` or `examples/lms` (no `src/console/`). A snippet needs a new example command, for example in the LMS (`lms:report`, already used by the CLI test `crates/rustcms-cli/src/make.rs:451`).
  - Entry point: `examples/demo/src/main.rs`.
  - `refuse_in_production`: `examples/demo/src/database/seeders/user_seeder.rs:67-72` (the same environment check as the page's last sample).
- **Screenshots needed**: none (a terminal capture of `route:list` could help, but it is optional).
- **Linked anchors**: none (no `console.md#…` links).

### basics/testing.md

- **Public API covered**: `TestApp::{new, with_config, get, post_json, request_from, state, acting_as, login_as, with_token, client, make_super_admin, mails, take_mails}`; `TestResponse::{assert_status, assert_json, assert_validation_error}`; `TestClient::{origin, set_csrf, from, socket, socket_with}`; `TestSocket::{send_text, next_data, closed, inner}`, `Refused { status, body }`; `TestDb`; `db_test!` (`migrate = M`); `UserFactory::new().password()`; `Factory::{definition, create, create_many}`; `rustcms_testing::create_table`; `TestS3::{from_env, configure}`; env vars `RUSTCMS_TEST_PG`, `RUSTCMS_TEST_MYSQL`, `RUSTCMS_TEST_S3`, `RUSTCMS_TEST_REDIS`, `RUSTCMS_TEST_REQUIRE`, `RUSTCMS_TEST_BACKENDS`, `RUSTCMS_TEST_SMTP`, `RUSTCMS_TEST_SMTP_TLS`, `RUSTCMS_TEST_SMTP_TLS_CA`; feature `mail`; `queue::run_until_idle`; `just test-fast`.
- **Wrong or outdated**
  - "With the `mail` feature of `rustcms-testing`, `test_config` sets the `memory` mail driver": `test_config` always sets `mail.driver = "memory"`. Only `mails()` / `take_mails()` need the feature (`crates/rustcms-testing/src/app.rs:18-27,58-70`).
  - "Authorization oracles: keep a hand-written table…" never names the helper that does this for `AdminRoutes`: `rustcms_testing::admin_routes::{Case, assert_access}` (`crates/rustcms-testing/src/admin_routes.rs:27-53`). It checks that every route has a case. Today only `admin/admin-routes.md:92-102` documents it.
  - The first sample (`/api/v1/posts`, `assert_json("/meta/total", json!(0))`, `make_super_admin` before `with_token`) is hand-written and leaves out imports (`StatusCode`, `json`). The demo test that matches it is real: `examples/demo/tests/feature/api.rs` (`token_api_lists_and_creates_posts`).
  - The page shows the `#[path = "../support/mod.rs"] mod support;` line as if each suite writes it. In the template it sits once in `tests/feature/main.rs` (`crates/rustcms-cli/templates/app/tests/feature/main.rs.tmpl:4-5`), and modules use `crate::support`.
- **Missing**
  - `rustcms_testing::test_config(url)` (`app.rs:18`: `testing` env, random `APP_KEY`, the app's `config/`); `TestApp::{db, router, request, json(method, uri, body), listen}` (`app.rs:54,72,77,103`; `socket.rs:33`); `TestApp::admin_api(path)` (`client.rs:97`).
  - `TestResponse::{json, header, text}` (`app.rs:135-150`).
  - `TestClient::{bearer, languages, cookie, csrf_token, fetch_csrf, send, request, get, post_json}` (`client.rs:105-185`).
  - `TestSocket::{send_binary, protocol}` (`socket.rs:60-65,128`).
  - `TestDb::{sqlite, empty, migrated, postgres, mysql, migrate, single_connection, reopen, conn, url, backend, cleanup}` and `Backend` (`crates/rustcms-testing/src/db.rs:8,46-291`). `reopen` acts as a second instance and `single_connection` reads the schema between migration steps.
  - The `db_test!(name, |db| …)` form without `migrate` (empty database; `crates/rustcms-testing/src/lib.rs:145-152`).
  - `UserFactory::{name, email, inactive}`, and the fact that without `.password()` the stored hash matches nothing (`crates/rustcms-testing/src/factory.rs:5-50`).
  - `Factory::{try_create, try_create_many}`. `Factory` lives in `rustcms::database` and `rustcms_testing::Factory` is a re-export (`crates/rustcms-testing/src/lib.rs:85`; `crates/rustcms-core/src/database.rs:83-130`).
  - `TestDb` naming: templates are `rustcms_tpl_<fingerprint of names>` (`db.rs:104,151`). The page has this, and it is correct.
  - `RUSTCMS_TEST_REQUIRE=s3` (`lib.rs:97-115`).
  - `acting_as` signs in to the admin guard only, and `with_token` issues a token named `test` (`client.rs:45-95`).
- **Examples available**
  - `examples/demo/tests/feature/api.rs` (`token_api_lists_and_creates_posts`, `api_access_comes_from_api_guard_permissions_only`): `with_token`, `acting_as`, `assert_validation_error`.
  - `examples/demo/tests/feature/health.rs` (`health_and_ping`, `admin_sign_in`: `login_as`).
  - `examples/demo/tests/feature/live.rs` (`only_allowed_users_reach_the_live_routes`: `assert_access`; `the_echo_socket_answers_and_authorizes_its_command`: `socket`, `Refused`). Use it in place of the hypothetical docker-exec socket sample.
  - `examples/demo/tests/feature/content.rs` (`seeding_completes_after_an_interrupted_run`, `migrations_roll_back_and_forward_keeping_ids`).
  - `examples/lms/tests/feature/enrollments.rs` (`the_last_seat_goes_to_one_request`, `enrolling_queues_a_welcome`: queue in tests); `examples/lms/tests/support/mod.rs`.
  - Template: `crates/rustcms-cli/templates/app/tests/support/mod.rs.tmpl` (`app`), `tests/feature/health.rs.tmpl`, `stubs/test.rs.tmpl`.
  - Factory: `examples/lms/src/database/factories/course_factory.rs` (`CourseFactory`).
  - Oracle files `fixtures/authz/{auth,admin}.toml` with `crates/rustcms-auth/tests/oracle.rs`, as a model for "Authorization oracles".
  - Test doubles: no example binds a double. The `bind` newtype sample stays hand-written, or needs an example added.
- **Screenshots needed**: none.
- **Linked anchors**: none.

---

### security/authentication.md

- **Public API covered**: `AuthProvider`; guards `admin` / `api`; `ApiRoutes`, `Access::can`, `Auth` (`.0.user`, `principal()`, `authorize`), `Validated`; config `auth.admin_path`, `session_lifetime_minutes`, `remember_days`, `anonymous_session_lifetime_minutes`, `anonymous_sessions_per_ip`, `anonymous_sessions_per_minute`, `password_reset`, `reset_requests_per_ip`, `reset_requests_per_email`, `reset_cooldown_secs`, `password_reset_minutes`, `revoke_tokens_on_password_change`, `invitation_days`, `admin_origins`, `jwt.access_ttl_secs`, `attempts_per_identifier`, `attempts_per_ip`, `[redis] url`; `APP_KEY`, `APP_URL`; cookie `rustcms_session`; header `X-CSRF-Token`; endpoints `<admin>/api/auth/{csrf, login, logout, me, locale, password, logout-others, forgot-password, reset-password, invitation}`, `/api/v1/auth/{token, me, jwt, refresh, jwt/logout}`; commands `key:generate --force`, `session:prune`, `idempotency:prune`, `token:create`.
- **Wrong or outdated**
  - "Every request reloads the user: a disabled user, or one whose `auth_version` changed, is signed out immediately." Only a disabled or deleted user's session is ended (`session.logout()`, `crates/rustcms-auth/src/guards/guard.rs:150`). On an `auth_version` mismatch the request is anonymous and the session is *frozen*, not ended, so a concurrent re-issue can win (`guard.rs:145-149`, comment `:141-144`). The other sessions are ended by the credential change itself (`guards/credentials.rs:37-52`).
  - "`serve` deletes expired rows hourly": the hourly prune is a `tokio::spawn` loop started in `AuthProvider::boot` (`provider.rs:203-232`). It runs in every process that builds the app (console commands too), coordinated by `jobs::exclusive`. It also prunes JWT families, reset and invitation tokens, and audit entries (`provider.rs:213-230`). It does not run through `AppBuilder::schedule`, so it does not appear on the Schedules page.
  - The CSRF endpoint table lacks `forgot-password`, `reset-password` and `invitation` (`http/routes.rs:47-49`). They are described above the table but not listed in it.
  - "Sign-in attempts (admin login and token issuance)": JWT sign-in uses the same `attempt` path (`routes.rs:474`). The password confirmation in `password` / `logout-others` is also throttled per user under `attempts_per_identifier` (`routes.rs:294-302`, key `confirm:<id>`).
  - The `Secure` flag: the page says "when `APP_URL` is https", but `auth.secure_cookies` overrides it (`config.rs:15-16`; `state.rs:117`).
- **Missing**
  - Extractors and types: `MaybeAuth` (`guard.rs:79`); `Authenticated { principal, user, token_id, jwt_family }` (`guard.rs:33-42`); `Auth::authorize_guard` (`guard.rs:56`); `Session` as an extractor (`guard.rs:88-98`; `login`, `login_remembered`, `logout`, `restamp`, `csrf_token` in `guards/session.rs:81-155`); `rustcms_auth::routes::confirm_password` (`routes.rs:294`, for app routes that re-ask the password).
  - Headers and constants: `CSRF_HEADER` (`guard.rs:29`), `USER_HEADER` `x-rustcms-user` on every admin response (`guard.rs:198`), `LOCALE_HEADER` `x-rustcms-locale` (`routes.rs:133`), `Cache-Control: private, no-store` on admin API and on cookie-setting responses (`guard.rs:113-117`), `MAX_EMAIL_LEN` / `MAX_PASSWORD_LEN` (`routes.rs:32-34`), token prefix `rcms_` / refresh prefix `rcmsr_` (`tokens.rs:15`; `jwt_session.rs:27`).
  - Behaviour:
    - Unknown `<admin>/api/*` paths answer a JSON 404 inside the session guard (`routes.rs:80-87`; `provider.rs:155-158`).
    - Admin asset paths skip the guard (`guard.rs:103-107`).
    - `forgot-password` and `reset-password` answer 404 when `password_reset = false` (`routes.rs:571,620`).
    - JWT endpoints answer 404 when `jwt.enabled = false` (`routes.rs:450`).
    - `admin_path` validation: not `/`, not under `/api` (`config.rs:120-137`).
    - The JWT config bounds panic at boot (`config.rs:78-89`; `provider.rs:134`).
  - Limits not on the page:
    - `password_min_length` (`routes.rs:319,538`).
    - Token-use throttles of 10/min per IP and 5 per token (`routes.rs:610-611`).
    - Global caps: reset requests at 300/min (`routes.rs:558`), anonymous sessions in total (`routes.rs:~240`).
    - JWT abilities: 1–100 names, 2048 bytes in all (`routes.rs:27-30,463-473`).
  - `[audit] sign_in_failures_per_minute` (600; `crates/rustcms-core/src/audit.rs:50,55`) and the sign-in audit events (`support/audit.rs`).
  - Password hashing: `Passwords` bounds Argon2id concurrency to the CPU count (`support/password.rs:19-32`). The production page mentions this, the authentication page does not.
  - Mail templates `auth.password_reset` / `auth.invitation` (en, vi) registered by the provider (`provider.rs:163-170`). Boot fails when `password_reset` is on without a mail driver (`provider.rs:183-191`); the page says so, which is correct.
  - No link to the `[auth]` table in `getting-started/configuration.md:41-68`. That table holds `cookie_name`, `secure_cookies`, `password_min_length`, `permission_cache_ttl_secs` and the session defaults (120 min).
- **Examples available**
  - `examples/demo/src/routes/api.rs` (`routes`: `Access::public`, `Access::can`).
  - `examples/demo/src/http/controllers/post_controller.rs` (`store`: `Auth` + `Throttle<CREATES_PER_MINUTE>` + `Validated`), in place of the hand-written `create_post`.
  - `crates/rustcms-cli/templates/app/src/providers/auth_service_provider.rs.tmpl` (`provider`).
  - Tests: `examples/demo/tests/feature/health.rs` (`admin_sign_in`).
- **Screenshots needed**
  - The sign-in page with the "Remember me" box and the "Forgot password?" link (`admin-ui/src/pages/login.tsx`).
  - Forgot, reset and invitation pages (`admin-ui/src/pages/recovery.tsx`).
  - The Account page with password change and "sign out everywhere" (`admin-ui/src/pages/account.tsx`).
  - The local `examples/demo` shows all of these, but forgot-password needs `auth.password_reset = true` plus Mailpit. RBlog would show a real branded sign-in. RDockPanel is not needed.
- **Linked anchors**: `#csrf` (from `reference/admin-api.md:9`).

### security/authorization.md

- **Public API covered**: tables `permissions`, `roles`, `role_has_permissions`, `model_has_roles`, `model_has_permissions`; `super_admin`; `AuthProvider::{define, permission, admin_permissions, permission_area, permission_meta, permission_preset}`; `gate.allows`; `permission:sync`; `PermissionArea::new().sort()`, `PermissionMeta::new().label().description().area()`, `PermissionPreset::new().description().permissions()`; `ADMIN_GUARD`, `API_GUARD`; `AdminProvider::register` checks; `lang:missing`; `RbacService::{new, audited, sync_permissions, create_role}`, `Actor::{User, System}`, `rbac::lock`; `AuditContext`, `AuditActor::user`; `AuthState` (`resolver`, `permissions.all()`); `role:assign`, `token:create`.
- **Wrong or outdated**
  - "`gate.allows(principal, "publish")`" does not say how to get the gate. It is `AuthState.gate` (`crates/rustcms-auth/src/state.rs:88`). The page also leaves out the gate's semantics: super admins pass, token abilities still bound it (`state.rs:37-44`), and `Gate::authorize` returns 403 (`state.rs:46`).
  - The refusal table is incomplete, so it reads as exhaustive when it is not. It lacks three rules:
    - Replacing another user's credentials (password, email; `reset_password`, `update_profile`, `check_reset_password`) needs super admin of **every** guard (`rbac/rules.rs:6-27`; `service.rs:647,698,740`).
    - Accounts that hold a protected role are managed (status, profile, deletion) only by super admins of every guard (`rules.rs:114-127`; `service.rs:397,427,642`).
    - A role that carries orphaned permissions can be assigned only by super admins (`rules.rs:93-112`; `service.rs:263`).
  - Lock timeout: PostgreSQL RBAC changes set `lock_timeout = '10s'` (`service.rs:~800`) and MySQL uses the 10 s lock wait. The page does not say so, though it is relevant to "applied one after another".
- **Missing**
  - `AuthProvider::api_permissions(&ApiRoutes)` (`provider.rs:62-70`). Only `admin_permissions` appears in the sample, though both demo and template use `api_permissions`.
  - `BUILT_IN_PERMISSIONS` (users.*, roles.*, permissions.*; `provider.rs:16-31`).
  - The rest of `RbacService` (`rbac/service.rs`): `set_role_permissions[_if]` (162/169), `delete_role` (222), `assign_role` / `revoke_role` (251/282), `grant_permission` / `revoke_permission` (313/347), `set_user_active[_if]` (374/381), `delete_user` (421), `create_user` / `create_user_with_roles` / `invite_user` / `resend_invitation` (454–529), `set_user_roles[_if]` (557/564), `update_profile[_if]` (605/619), `check_reset_password` / `check_new_user` / `covers_all_guards` / `reset_password` (694–733). Also `NewUser` (`:35-45`), `SyncReport` (`:48-53`), and `rbac::begin_as` (`:853`: writes outside RBAC under its lock, used by settings).
  - `PermissionResolver` / `Grants`, `rbac::shared_generation` (`rbac/resolver.rs:10-103`); `PermissionCatalog` and `RESOURCES_AREA` / `SYSTEM_AREA` / `OTHER_AREA` (`catalog.rs:10-14`).
  - `Principal { id, guard, permissions, super_admin, token_abilities, auth_version, locale, credential }` and `Principal::can`, `Credential` (`crates/rustcms-core/src/principal.rs:11-57`).
  - `permission:sync` restores re-declared permissions and regroups them, and records a `permission.synced` audit entry (`service.rs:756-833`).
- **Examples available**
  - `examples/demo/src/providers/auth_service_provider.rs` (`provider`: `permission`, `api_permissions`, `admin_permissions`, `permission_area`, `permission_meta`, `permission_preset`). It can replace the hand-written Roles-page sample whole.
  - `examples/lms/src/providers/auth_service_provider.rs` (`provider`); `examples/lms/src/policies/permissions.rs` (constants).
  - `examples/lms/src/database/seeders/role_seeder.rs` (`RoleSeeder::run`: `sync_permissions` + `create_role`). It can replace the "Creating roles from code" sample, which is an `AppCommand` that exists nowhere.
  - `examples/demo/src/database/seeders/user_seeder.rs` (`UserSeeder::run`: super admin of both guards, `create_role`, `assign_role`; `account`: `create_user`).
- **Screenshots needed**
  - System → Roles: areas, the resource matrix, groups, search, presets, the notice after a preset (`admin-ui/src/pages/system/roles.tsx`). The local `examples/demo` has the "Live" area and the "viewer" preset; RDockPanel shows a real `docker` area (the sample uses docker permissions); RBlog shows a CMS role set.
  - System → Users, with role assignment.
- **Linked anchors**: `#the-roles-page-areas-labels-and-presets` (from `admin/overview.md:101`, `admin/users-roles.md:26`, `release-notes.md:127`).

### security/revocation.md

- **Public API covered**: `RbacService::set_user_active`; `logout-others`; `revoke_tokens_on_password_change`; `POST /api/v1/auth/jwt/logout`; `APP_KEY`; `users.auth_version`; `permission_cache_ttl_secs`; `auth_generations.rbac`.
- **Wrong or outdated**
  - The table matches the code:
    - `revoke_all` bumps `auth_version`, revokes JWT families, and revokes tokens if asked (`crates/rustcms-auth/src/guards/credentials.rs:37-52`).
    - Disabling, admin reset and profile changes revoke tokens (`rbac/service.rs:411,681,749`).
    - Link reset and invitation use `revoke_tokens_on_password_change` (`recovery/mod.rs:288`).
    - JWT and session keys derive from `APP_KEY` only, not `APP_PREVIOUS_KEYS` (`state.rs:120-131`; `guards/jwt.rs:62-64`).
  - One nuance: "Current session: ended" for a disabled user holds. For a credential change made elsewhere, the current request becomes anonymous, but the row is ended by that change's transaction, not by the guard (`guard.rs:145-150`).
- **Missing**
  - Where the "framework's test suite" lives: `crates/rustcms-auth/tests/{auth_flow,jwt,recovery,rbac,instances}.rs`. Linking them backs the claim.
  - `credentials::{lock_unchanged, revoke_all, revoke_tokens, change_password, logout_everywhere, issue_token}` are public (`credentials.rs:24-137`) for apps that build their own credential flows. Not documented anywhere.
  - Audit actions written with each revocation: `auth.password_changed`, `auth.signed_out_everywhere`, `token.revoked` (`credentials.rs:78,91`; `http/routes.rs:417`).
  - The `x-rustcms-user` header that lets the SPA detect an account switch in another tab (`guard.rs:196-198`).
- **Examples available**: none needed (a table page). `examples/demo/tests/feature/api.rs` shows the guards separated (an admin cookie gets 401 on the API).
- **Screenshots needed**: none, or the Account page's "sign out everywhere" button (`examples/demo`).
- **Linked anchors**: none with fragments (the page is linked whole from `api/authentication.md:42,56`).

### security/production.md

- **Public API covered**: `APP_ENV`, `APP_DEBUG`, `APP_KEY`, `APP_PREVIOUS_KEYS`, `APP_URL`, `key:generate`, `key:reencrypt`; `auth.admin_origins`, `http.trusted_proxies`, feature `redis`, `[redis] url`, `redis.prefix`, `storage.root`, `storage.max_upload_bytes`, `media:prune`, `queue.enabled`, `queue:work`, `queue:failed`; permissions `queue.manage`, `schedules.manage`, `settings.mail.update`; `migrate`, `migrate --allow-newer`, `migrate:rollback`, `permission:sync`, `make:user --super-admin`; `audit.retention_days`; `RUSTCMS_ADMIN__BRANDING=static`.
- **Wrong or outdated**
  - Every key named exists:
    - `queue.manage` (`crates/rustcms-admin/src/features/queue.rs:28`); `schedules.manage` (`features/schedules.rs:3`).
    - `audit.retention_days` (`crates/rustcms-core/src/audit.rs:47`, default 365); `storage.max_upload_bytes` (`storage/mod.rs:115`, 10 MiB).
    - `app.previous_keys` / `APP_PREVIOUS_KEYS` (`config.rs:55-59,255`); `redis.prefix` (`redis.rs:21`); `queue.enabled` (`queue/mod.rs:178`).
    - The branding switch (`crates/rustcms-admin/src/panel/provider.rs:185`).
  - Nothing found wrong.
  - "`permission:sync` run after each deploy" appears twice (lines 27 and 30).
- **Missing**
  - `auth.secure_cookies` must not be forced `false` behind https (`crates/rustcms-auth/src/config.rs:15-16`).
  - `auth.jwt.enabled = false` when no client uses JWT (`config.rs:58-60`).
  - `auth.password_reset` needs a mail driver, or boot fails (`provider.rs:183-191`).
  - `http.body_limit` / `timeout_secs` (`crates/rustcms-core/src/config.rs:82-89`).
  - `log.json` (`config.rs:96`); `database.max_connections` sizing (`config.rs:75`).
  - MySQL setup checked at startup (`db/check.rs`; see database.md#mysql).
  - `audit.sign_in_failures_per_minute` (`audit.rs:50`).
  - `_FILE` secret variables: `SecretFiles` (`config.rs:258-273`). Check whether `getting-started/configuration.md` or `deployment.md` covers this, and link it.
  - `[realtime]` limits for SSE and sockets (`crates/rustcms-auth/src/provider.rs:149-151`).
- **Examples available**: none (a checklist).
- **Screenshots needed**: none.
- **Linked anchors**: none.

### api/authentication.md

- **Public API covered**: `Authorization: Bearer rcms_<id>_<secret>`; abilities, `*`; `role:assign --email --role --guard`, `token:create --email --name --abilities --expires-days`; `POST/DELETE /api/v1/auth/token`, `GET /api/v1/auth/me`; `auth.jwt.enabled`, `refresh_ttl_days`, `refresh_family_days`; `POST /api/v1/auth/{jwt, refresh, jwt/logout}`; response `{accessToken, tokenType, expiresIn, refreshToken, refreshExpiresAt, abilities}`; `expires_in_days` 1–3650.
- **Wrong or outdated**
  - The intro calls tokens "personal access token… Bearer rcms_…" only. The guard also accepts JWT access tokens on the same header, and the JWT section says so later. That is fine.
  - The HTTP body `abilities` reads as required. It is optional (`#[serde(default)]`), and empty means `["*"]` (`crates/rustcms-auth/src/http/routes.rs:369-370`; `guards/tokens.rs:48`).
  - `token:create --abilities` defaults to `*` (`console/commands.rs:98`); the page does not say so.
  - JWT `abilities`: "omitted = `*`" is right, but an empty array is refused (422; `routes.rs:463-473`).
- **Missing**
  - The token issuance response `201 {token, abilities, expiresAt}` (`routes.rs:404-409`). Validation: `token_name` 1–255 characters (`:383`), at most 100 abilities of ≤255 bytes (`:386-388`).
  - The `GET /api/v1/auth/me` response (`Me`: `user {id (string), name, email}`, `guard`, `permissions`, `superAdmin`, `abilities`, `locale`, `preferredLocale`, `locales`; `routes.rs:91-131`).
  - Limits:
    - JWT sign-ins: 20 per user per minute (`routes.rs:428`).
    - Refresh and logout: 60/min per IP and 30/min per family (`:424-426`); the authentication page has these, this page does not.
    - JWT abilities: ≤100 names, 2048 bytes in all (`:27-30`).
    - Access token length ≤4096 (`guards/jwt.rs:18`); refresh token `rcmsr_…` ≤80 (`jwt_session.rs:27-30`).
  - `jwt.access_ttl_secs` (60–3600, default 900) is not on this page (`config.rs:61-62,72`).
  - JWT endpoints answer 404 when disabled (`routes.rs:450`).
  - The JWT body is `deny_unknown_fields` (`routes.rs:430-437`).
  - `DELETE /api/v1/auth/token` with a JWT bearer is 401, since it needs `token_id` (`routes.rs:412`).
  - Per-route throttling `Throttle<N>` (`crates/rustcms-auth/src/http/api.rs:266-279`, keyed per route and user, needs a credential). It appears only in `basics/validation.md`; link it.
  - The error format of 401/403/422/429 (`Retry-After`): link `basics/errors.md`.
- **Examples available**
  - `examples/demo/tests/feature/api.rs` (`token_api_lists_and_creates_posts`, `api_access_comes_from_api_guard_permissions_only`): token abilities against API-guard permissions.
  - `examples/demo/src/routes/api.rs` (`routes`); `examples/demo/src/http/controllers/post_controller.rs` (`store`, `Throttle`).
  - No example drives the JWT flow. `crates/rustcms-auth/tests/jwt.rs` is internal; an HTTP transcript stays hand-written.
- **Screenshots needed**: none. An API-token management UI does not exist; tokens come from the CLI or HTTP only.
- **Linked anchors**: none.

---

## Features in this area with no guide page

- **Transactions and named locks**: `rustcms::db::{begin_write, begin_snapshot, set_lock_wait, xact_lock, lock_slot}` (`crates/rustcms-core/src/db/{transactions,locks}.rs`), and the MySQL limit to preseeded lock names. They are only mentioned in passing in `digging-deeper/queues.md:61`, `audit-log.md:151` and `admin/resources.md:163`.
- **Factories**: `rustcms::database::Factory` + `make:factory`, used by seeders and tests. One line in testing.md, nothing in database.md.
- **Credential primitives for custom flows**: `rustcms_auth::credentials::*`, `tokens::{issue, revoke, authenticate}`, `jwt_session::*`, `session::{create_for_user, end, lock_lineage}`, `routes::confirm_password`. All are public; none is documented.
- **Gate**: `AuthProvider::define` / `Gate::{allows, authorize}` / `AuthState.gate`. One bullet only, with no example in the demo or the LMS.
- **Hourly auth housekeeping** (sessions, JWT families, reset tokens, audit prune in `AuthProvider::boot`) and how it relates to `AppBuilder::schedule`: no operations entry. `operations/retention.md` may partly cover it; check.
- **Testing reference**: no API reference for `TestDb`, `TestClient` or `TestResponse` (see testing.md Missing). Consider a `reference/testing.md` table.
