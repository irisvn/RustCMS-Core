# Upgrade guide

## From 0.1.0-alpha.0 to the next release

1. Update the `rustcms` dependency, then run `rustcms migrate` (new tables for
   media, JWT sign-ins, idempotency keys, locks, the audit log, settings,
   jobs, and `users.locale`). `serve` now also runs queue workers (set
   `queue.enabled = false` to keep them out of web instances).
2. Rust API changes:
   - API routes: move them to `ApiRoutes` with an explicit `Access` per route
     ([Routing](basics/routing.md)); register them with
     `AuthProvider::new().api_permissions(&api)` and `api.register(&mut app)`.
   - `RateLimiter::reserve`, `release` and `clear` are async (`.await` them).
   - `Principal` and `rustcms::models::user::Model` have new fields
     (`locale`, `credential`): struct literals need them, or `..Default::default()`.
   - `AppError::Conflict` has a new `reason` field.
   - `SettingsAbilities` has a new `test` field; `SettingsPage::validate_in`
     (with the application) runs `validate` by default.
   - `rustcms::models::user::Model` has `invitation_pending`, `invited_at`,
     `invited_by` (migrations `…000018`, `…000019`): struct literals need them
     (`..Default::default()` on `ActiveModel`s is unaffected). `rustcms-auth`
     now enables the core `mail` feature.
   - Mail: register `rustcms::mail::MailProvider` (and
     `AdminProvider::mail_settings()` for the page); in production with
     `driver = "smtp"` and `source = "settings"`, set `[mail] allowed_hosts`.
   - Functions that change credentials record an audit entry and take one more
     argument: `credentials::change_password` and `credentials::logout_everywhere`
     an `&AuditContext`; `credentials::issue_token` and
     `jwt_session::{issue, rotate, logout}` a `&RequestMeta` (both in
     `rustcms::audit`; `RequestMeta` is also an extractor).
   - `InputType` has a `Secret` variant and `AdminPanel` a `settings` field
     (exhaustive matches and struct literals need them); admin clients see
     `vocabularyVersion` 5.
   - Resource slugs are limited to 64 bytes and action names to 57, and
     `user`, `role`, `permission`, `settings`, `session` (and `job`, `queue`,
     see below) are reserved (boot fails); `Ctx::new` records writes as its principal.
   - `ActorKind` has a `Job` variant (exhaustive matches need it).
   - `AdminPanel` has a `custom` field (`CustomPanel::empty(base)` in hand-built
     panels), `PanelResponse` a `pages` field and `ColumnSchema` `cell` /
     `props`; admin clients see `vocabularyVersion` 6.
   - `SystemLinks` has `audit` and `queue` fields; resource schemas gain the
     `history` ability.
   - Resource slugs `job` and `queue` are reserved too (subjects of the
     Queue page's audit entries).
   - Actions: `data` sent to an action without a form is refused (422), and
     the request body may only hold `keys` and `data`. Declare what an action
     needs with `Action::form` ([Actions](admin/actions.md#actions-with-a-form)).
     Actions are admitted before the body is read: a policy deciding per record
     should override `Policy::admits` for abilities that need a permission.
     Actions the policy does not admit are no longer listed in the schema.
   - Run `rustcms migrate` (new `exports` and `imports` tables). `ResourceSchema.abilities`
     may hold `export`; `DynResource` has new methods (implementations outside
     the crate are not expected).
   - `Relation` has new fields and `RelationSchema` a `kind`; `ActionSchema`
     a `form`; `ListQuery` a hidden `through` field (struct literals need
     `..Default::default()`). Admin clients see `schemaVersion` 2 and
     `vocabularyVersion` 7: an older admin SPA turns read-only.
   - Tests that build tables from entities instead of running migrations also
     need the `audit_logs` table ([Testing](basics/testing.md)).
3. Optional: translate your labels. Add `lang/vi.toml`, register it with
   `AdminProvider::new().translations("vi", include_str!("../lang/vi.toml"))`,
   and fill in what `rustcms lang:missing vi` lists.
4. API clients: error bodies gain `issues` and `formIssues` (and `reason` on
   409); existing fields are unchanged.
5. Several instances: see [Several instances](digging-deeper/instances.md).

## Application layout (0.3)

Applications are laid out like Laravel applications, inside `src/`
([Directory structure](getting-started/directory-structure.md)). Generators
refuse an application in the old layout; move it once:

| Before | Now |
|---|---|
| `src/app.rs` (`build`) | `src/bootstrap/mod.rs` (`build`) + `src/bootstrap/providers.rs` |
| providers set up in `build` | `src/providers/{auth_service_provider,admin_panel_provider,app_service_provider}.rs` |
| `src/migrations/` | `src/database/migrations/` (keep the file names: they are recorded in `seaql_migrations`) |
| `src/admin/post.rs` | `src/admin/resources/post_resource.rs` |
| `src/http/api/mod.rs` (`routes`) | `src/routes/api.rs` |
| `src/http/api/posts/handlers.rs` | `src/http/controllers/post_controller.rs` |
| `src/http/api/posts/requests.rs` | `src/http/requests/post_request.rs` |
| `src/http/api/posts/json.rs` | `src/http/resources/post_resource.rs` |
| a settings struct with its page | `src/settings/<x>_settings.rs` + `src/admin/pages/<x>_settings_page.rs` |
| a `<app>:seed` command | `src/database/seeders/` + `rustcms db:seed` |
| `tests/*.rs` | `tests/feature/*.rs` listed in `tests/feature/main.rs`, helpers in `tests/support/mod.rs` |
| `assets/admin/<vendor>/*.js`, `*.css` | `resources/admin/js/<vendor>/`, `resources/admin/css/<vendor>/` (update the `include_str!` paths) |

Then:

1. `src/main.rs`: `Console::<my_app::database::migrations::Migrator>::new(my_app::bootstrap::build)`.
2. Add the generator anchors where generated lines go: `// rustcms:mods`
   (`src/lib.rs`), `// rustcms:resources` and `// rustcms:pages`
   (admin panel provider), `// rustcms:seeders`, `// rustcms:jobs`,
   `// rustcms:commands` (app service provider), `// rustcms:api-routes`
   (`src/routes/api.rs`), `// rustcms:tests` (`tests/feature/main.rs`),
   `// rustcms:extensions` (admin panel provider, for `make:page --script`). A new
   application from `rustcms new` shows every one.
3. `rustcms migrate:status` lists every migration as applied: nothing to run.

