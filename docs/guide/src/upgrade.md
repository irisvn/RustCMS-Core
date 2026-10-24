# Upgrade guide

## Upgrading the command and an application

1. The command: `rustcms self-update` replaces it with the latest release
   (`--version v0.4.0` for a given one, older included). It works when the
   [install script](getting-started/installation.md#installing-the-rustcms-command)
   installed it; a command built with `cargo install` is updated the same way
   it was installed. `rustcms --version` shows the version and the platform.
2. An application: in its directory, `rustcms upgrade` moves it to the
   command's version:

   ```sh
   rustcms upgrade --dry-run   # what changes in Cargo.toml
   rustcms upgrade             # writes it, then updates Cargo.lock
   rustcms migrate
   cargo test
   ```

   It sets the tag of `rustcms` and `rustcms-testing` (always the same), the
   `sea-orm` pins of that version and `[package.metadata.rustcms] version`,
   keeping your comments and features. If Cargo cannot resolve the result,
   `Cargo.toml` and `Cargo.lock` are put back. It refuses to go back to an
   older version (unless `--force`), and leaves alone a dependency on a local
   `path`, inherited from a workspace, renamed, or from another repository:
   change those by hand. Then follow the section of that version below, and
   commit `Cargo.toml` and `Cargo.lock` together.

   To upgrade to another version than the command's, install that command
   first (`rustcms self-update --version vX.Y.Z`).

## From 0.10.0 to 0.10.1

- **Run `rustcms migrate`** (`m20261201_000022_add_alt_to_media`, a nullable
  column): `serve` does not wait for it, and media queries fail until it
  has run. 0.10.0 binaries read the migrated database: to roll back, run
  them with `migrate --allow-newer` (or `[database] allow_newer_migrations`);
  leave the column in place (`migrate:rollback` would undo the application's
  own latest migrations first).
- **`media.update` now grants something**: renaming one's own files and
  editing their alternative text (before, updates were always refused); with
  `media.manage_all`, everyone's files. Review roles that hold `media.*` as a
  whole.
- The navigation and the dashboard's resource cards follow registration
  order, not slugs: use `ResourceMeta::sort` to keep an order.
- Every `.bulk()` action is also offered on rows where the record's
  abilities allow it; an action offered only for a selection is not
  supported (`visible_when` also applies when the action runs, so hiding
  it on every row would leave a bulk run nothing to process).
- Action dialogs no longer say "Run": their button is the action's label
  without "…" (or `submit_label`); empty values read "Not set". Update
  end-to-end selectors.
- Vocabulary 10: `ActionSchema.primary` and `submitLabel`,
  `ColumnSchema.hideBelow`; per-record `abilities` include `visible_when`.
  An older admin SPA shows the banner and every offered action as a button;
  extensions that check the vocabulary version should accept 10.
- Struct literals: `ActionOutcome` has `message` and `open` (add
  `..Default::default()`); `media::entity::Model` / `ActiveModel` have `alt`.
  Clients that build wire types by hand gain `MediaItem.alt`,
  `RecentColumn.valueType`, `options` and `column`, and `RecentRecord.data`.
- `Action::new` is unchanged; handlers that return an outcome use
  `Action::with_outcome`.

## From 0.9.0 to 0.10.0

- Rich text documents have a version 2 (tables, embeds, callouts, image
  captions and widths). Stored version 1 documents stay valid and are read
  as they are, and the panel keeps writing version 1 unless a document uses
  a version 2 part. Nothing to migrate. To rewrite stored documents anyway,
  map each value through `rustcms::admin::rich_text::upgrade_value` (only
  `version` changes).
- **Rolling back to 0.9, or a 0.9 reader of the same database** (another
  service, a replica during a rolling deploy): documents saved with a
  version 2 part (table, embed, callout, image caption or width) are refused
  by 0.9's `rich_text::parse`, and the 0.9 panel shows them read-only. Deploy
  every reader first; before rolling back, remove those parts or keep 0.10.
  A headless consumer that parses documents itself should accept
  `"version": 2` and the new nodes (see [Rich text](admin/media.md#rich-text)).
- `rich_text::Node` is `#[non_exhaustive]` and has new variants (`Table`,
  `Embed`, `Callout`); `Node::Image` now holds an `Image { media, alt,
  caption, width }`; `Document` has a `version` field. Exhaustive matches,
  patterns on `Node::Image { media, alt }` and struct literals need updating.
- `rich_text::to_html` keeps its signature but writes images as
  `<figure><img …></figure>` and headings with an `id`. Use
  `to_html_with(&doc, &HashMap<Uuid, ImageInfo>)` to write image sizes, and
  `outline` for a table of contents.
- `PanelResponse` has an optional `uploads` field (the upload limits);
  admin clients that build `PanelResponse` values by hand need it.

## From 0.8.2 to 0.9.0

- New permission `media.view_all` (run `permission:sync` to list it). Roles
  that held `media.manage_all` only to see other users' files can hold
  `media.view_all` instead.
- `InputType` has a `BelongsToMany` variant, `InputSchema` a `links` field and
  `DynResource` a `link_options_new` method (exhaustive matches, struct
  literals and hand-written implementations need them). Admin clients see
  `vocabularyVersion` 9: an older admin SPA shows these inputs read-only and
  never sends them, and an extension
  that checks the vocabulary version should accept 9.
- `Resource::after_relation_change` is new: a link write never updates the
  parent row, so touch `updated_at` there if your application needs it.
- Restores now call `after_restore`, whose default calls `after_save`:
  nothing changes unless you override it (then call `Self::after_save` from
  it if restores should still run your `after_save`).

## From 0.8.1 to 0.8.2

Nothing needs to change (no rebuild of React pages either). A workaround that
hid `session_changed` from reads may stay or go.

## From 0.8.0 to 0.8.1

Nothing needs to change. To survive automatic rollbacks, keep migrations
additive and start the image with `migrate --allow-newer`. Deploy that image
before adding migrations: a rollback only accepts newer migrations when the
image it returns to is 0.8.1 or later with the flag (a 0.8.0 image still
refuses). Run migrations once per deployment, not in every replica
([Deployment](getting-started/deployment.md#upgrading)).

## From 0.7.1 to 0.8.0

Run `rustcms migrate`: it adds `jobs.started_at`, `jobs.description` and the
`job_runs` and `schedule_runs` tables. Then:

- Grant `schedules.view` (and `schedules.manage` for Run now) to the roles
  that should see System → Schedules (`rustcms permission:sync` declares them).
- `.run` schedules keep working; switch to `.task` to show a summary.
- Job kinds starting with `rustcms.` are now refused (they are the
  framework's).
- The Queue's *Scheduled* status is labelled *Delayed* (the API value stays
  `scheduled`).

## From 0.7.0 to 0.7.1

Nothing needs to change. React pages' dialogs get the close button without a
rebuild; `rustcms make:sdk` adds `closeButton` and `closeDisabled` to the
SDK's types.

## From 0.6.1 to 0.7.0

Most applications need no change; update links of your own to theme files,
and `PermissionInfo` struct literals, if you have any.
- The Roles page now groups permissions by area: resource matrix cells under
  "Resources", the built-in users, roles and permissions permissions under
  "System", the rest under "Other" until you declare areas
  (`permission_area`, `permission_meta`).
- Theme files moved from `{admin}/assets/ext/` to `{admin}/assets/brand/`
  (`theme.css`, `logo`, `favicon`). Change any link of your own to the old
  paths; the panel's pages need nothing.
- The sign-in pages now show your `brand(…)` name and logo instead of
  "rustcms".
- `PermissionInfo` (from `GET {admin}/api/system/permissions`) has new fields;
  code that builds it with a struct literal must add them.

## From 0.6.0 to 0.6.1

Nothing needs to change. Users can now pick Light, Dark or System; your
`Theme::color_scheme` is the default until they do. To use `useColorScheme()`
in a React page, run `rustcms make:sdk`, then `npm run build` in
`resources/admin`.

## From 0.5.1 to 0.6.0

Nothing needs to change: React pages built with `ui: 1` keep working.

To use `props.resources`, `props.toast` and the new components:
1. Run `rustcms make:sdk`: it updates `resources/admin/sdk` (keeping files
   you edited) and lists the registrations to change.
2. Set `ui: 2` in those `Requires { … }`. A bundle built with the new SDK
   is refused while its provider still declares `ui: 1`.
3. Run `npm install && npm run build` in `resources/admin`, then commit.

## From 0.5.0 to 0.5.1

Nothing needs to change. `PanelResponse` (`home`), `ResourceInfo` (`sort`,
`nav`) and `PageLink` (`sort`) have new fields: code that builds these
structs with a literal must add them (they are normally only read).

## From 0.4.1 to 0.5.0

Nothing needs to change: v1 scripts (`extension_script`,
`window.rustcms.registerPage` with `apiVersion: 1`) work as before.

- New, optional: [React pages](admin/react-pages.md)
  (`rustcms make:page <Name> --react`, `extension_script_v2`). They need Node
  20.19+ or 22.12+ to build, not to compile or deploy the app.
- `GET {admin}/api/panel` has a new `permissions` list; a test that compares
  the whole response should expect it, and Rust code building a
  `PanelResponse` literal must add `permissions`.
- `{admin}/api/pages/{slug}/{*path}` is new: a page now answers its
  sub-paths too (with the page's permissions; read `ctx.page_path()` in
  `data` to tell them apart).

## From 0.4.0 to 0.4.1

- `rustcms migrate`: two new tables, `schedule_states` and `schedule_slots`.
- Periodic work can move from your own `tokio::spawn` loops to
  [schedules](digging-deeper/scheduling.md), which run once per slot across
  instances and only in `serve`.
- `rustcms::settings::{read, load, write}` take the keyring
  (`state.keyring()`) instead of `&state.config().app.key`.
  `settings::get(&state)` is unchanged.
- New, optional: `APP_PREVIOUS_KEYS` (see
  [Rotating APP_KEY](operations/app-key.md)), and
  [encrypted columns](digging-deeper/encryption.md).
- The admin vocabulary is 8 (`secret_map` inputs). An extension that checks
  the vocabulary version should accept 8.

## From 0.3.2 to 0.4.0

`rustcms self-update`, then `rustcms upgrade` in each application. Nothing
needs to change in your code.

- New `[realtime]` keys, all optional: `socket_revalidate_secs`,
  `max_message_bytes`, `send_timeout_secs`
  ([Configuration](getting-started/configuration.md)). The other keys already
  existed.
- The admin page's CSP now also lists `ws(s)://` for the panel's origins in
  `connect-src`. A test that compares the header with
  `rustcms::admin::spa::CONTENT_SECURITY_POLICY` should use
  `content_security_policy(&origins)` instead.
- `{admin}/api/x` belongs to your `AdminRoutes`: nothing built in lives
  there.
- Behind a proxy, if you add WebSocket routes, see the
  [production checklist](security/production.md).

## From 0.3.1 to 0.3.2

Only the command changed (new `doctor`, `upgrade`, `self-update`, `docs`).
Install it again with the install script (0.3.1 has no `self-update`), then
`rustcms upgrade` in each application.

## Applications made before 0.3.1

They depend on `rustcms = "0.3.0"` (not on crates.io) or on a local `path`.
`rustcms upgrade` moves a version dependency to the Git tag; a `path` is
changed by hand, keeping your features:

```toml
[dependencies]
rustcms = { git = "https://github.com/irisvn/RustCMS-Core", tag = "v0.3.1" }

[dev-dependencies]
rustcms-testing = { git = "https://github.com/irisvn/RustCMS-Core", tag = "v0.3.1" }
```

## From 0.1.0-alpha.0 (or a 0.2 build) to 0.3.0

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

