# Docs v2 inventory: batches 12–15

Audited at commit `d4e87e3` (main). Paths are relative to the repository root. Source references are `file:line`.

Only one snippet region exists in the examples today: `examples/demo/src/bootstrap/providers.rs:7` (`register`). Every "Examples available" item below names a file and an item that would need a new `// #region`.

---

## Batch 12: digging-deeper

### digging-deeper/encryption.md
- **Public API covered**:
  - `rustcms::crypto::{Encrypted, Context, Binding}`
  - `Encrypted::seal` / `open`, `Context::column(..).row(..)`, `Unreadable`
  - `state.keyring()`, `ctx.keyring()?`
  - `app.encrypted_column::<E>(col, Binding)`
  - `Field::encrypted` / `encrypted_map`
  - `APP_PREVIOUS_KEYS`
  - `key:reencrypt --dry-run|--list`
- **Wrong or outdated**:
  - l.73 says the command "exits non-zero until it reports 0 to move, 0 changed and 0 unreadable". In a real run (not `--dry-run`), moved rows still count as finished. The command fails only when a row changed meanwhile or a value is unreadable; "to move" blocks success only with `--dry-run` (`crates/rustcms-core/src/crypto/columns.rs:184-186`). A real run also prints "moved", never "to move" (`columns.rs:313`).
  - Otherwise the page matches the code: the 32 KiB limit is `MAX_PLAINTEXT` (`crypto/mod.rs:64`), Debug/Serialize behave as stated (`mod.rs:275-281`), and `Binding` is at `columns.rs:18`.
- **Missing**:
  - The `--batch <1..10000>` option, default 500 (`columns.rs:293-299`).
  - `key:reencrypt` also re-seals the **settings secrets** (`columns.rs:309`; `--list` prints "settings secrets", `columns.rs:303`).
  - `APP_PREVIOUS_KEYS` maps to the config key `app.previous_keys`. It accepts a list in `config/app.toml` or a comma-separated string (`crates/rustcms-core/src/config.rs:55-59,129-144,255`). Boot fails when the keyring is invalid (`config.rs:293`).
  - `RowIdentity` canonical forms: integers in decimal, UUIDs lowercase hyphenated, text as is (`crypto/mod.rs:183-216`).
  - `EncryptedColumn::of` panics on a composite or unsupported primary key (`columns.rs:51-56`).
  - The cipher is AES-256-GCM and the envelope is `rcms1.<key id>.<…>` (`mod.rs:3,275`). Useful for operators.
  - `Encrypted::is_current`, `resealed`, `envelope`, `key_id`, `from_envelope` (`mod.rs:339-377`) are public but not described.
- **Examples available**:
  - `examples/demo/src/models/integration.rs`: `Model` is identical to the page's first sample, so import it.
  - `examples/demo/src/database/migrations/m20261101_000001_create_integrations.rs`: the `Text` columns.
  - `examples/demo/src/admin/resources/integration_resource.rs:22-23`: `Field::encrypted()` / `encrypted_map().max_items(20)`.
  - No example calls `seal`/`open` or `app.encrypted_column` (both still need hand-written samples, or a new demo item).
- **Screenshots needed**: the demo's **Integrations** record form with an encrypted token and env map (local `examples/demo`). RDockPanel (service env vars) would show the same.
- **Linked anchors**: none.

### digging-deeper/storage.md
- **Public API covered**:
  - `rustcms::storage::StorageProvider`
  - `[storage]` keys: `disk`, `root`, `max_upload_bytes`, `max_pixels`, `max_concurrent_uploads`, `allowed_types`, and `s3.bucket` / `region` / `endpoint` / `allow_http` / `timeout_secs` / `transfer_timeout_secs`
  - `AWS_*` variables, `media:prune`
- **Wrong or outdated**: no errors found. The defaults match `crates/rustcms-core/src/storage/mod.rs:158-171,145-155`, and the S3 ranges match `storage/s3.rs:46-51`.
- **Missing**:
  - `s3.region` defaults to `us-east-1` (`storage/mod.rs:132`).
  - SVG, HTML, XML and scripts are always refused, whatever `allowed_types` says (`storage/mod.rs:120`).
  - The `Storage` trait for application code: `put_file`, `open`, `delete`, `exists`, `disk`, `staging_dir`, `sweep` (`storage/mod.rs:84-101`).
  - `rustcms::storage::of(&state)` (`mod.rs:189`), `storage::connect` (`mod.rs:194`), and `StorageKey::generate` / `parse` (`mod.rs:56-74`).
  - Boot panics on a bad `[storage]` section or an unknown disk (`mod.rs:179-185,194-201`).
  - Environment overrides use the form `RUSTCMS_STORAGE__DISK` and so on (`config.rs:267-271`).
  - Exports and imports also live on the storage disk and are pruned every 10 minutes (`crates/rustcms-admin/src/panel/provider.rs:420-438,456`).
- **Examples available**:
  - `examples/demo/src/bootstrap/providers.rs`: region `register` (already present; it contains `app.provider(rustcms::storage::StorageProvider)`).
  - Same line in `examples/lms/src/bootstrap/providers.rs:9`.
  - No example has a `config/storage.toml`.
- **Screenshots needed**: the Media library grid and an upload in progress (local demo, or RBlog). The page is mostly configuration, so a screenshot is optional.
- **Linked anchors**: none.

### digging-deeper/instances.md
- **Public API covered**:
  - `redis` feature, `REDIS_URL` / `[redis] url`, `redis.prefix`
  - `AdminBranding::invalidate()`, `[admin] branding_ttl_secs`
  - `app.schedule(Schedule::every(..).job(..))`, `rustcms::jobs::exclusive(&state, name, ttl, f)`
- **Wrong or outdated**:
  - l.18 lists the periodic jobs as "(pruning sessions, media deletions, idempotency receipts)". The provider loops also prune exports and imports, JWT sign-ins, reset and invitation tokens, and audit entries (`crates/rustcms-auth/src/provider.rs:200-225`; `crates/rustcms-admin/src/panel/provider.rs:420-438`).
  - The redis prefix default `rustcms:<app.name>` is correct (`crates/rustcms-core/src/redis.rs:113`).
- **Missing**:
  - `redis.prefix` may not be empty or contain whitespace (`redis.rs:114-117`).
  - Boot fails when Redis cannot be reached (`redis.rs:119`).
  - The lease names (`auth-prune`, `media-prune:<disk>`, `exports-prune:<disk>`, `idempotency-prune`) help when debugging (`panel/provider.rs:408,424,447`; `auth/provider.rs:207`).
  - The queue lease and clock-skew settings, which matter for several instances, only link out (`queue.lease_secs`, `clock_skew_secs`).
- **Examples available**: none (no example builds with `redis`). `examples/demo/src/jobs/post_digest.rs` has the `schedule` function and could replace l.48.
- **Screenshots needed**: none (an architecture diagram would help more than a screenshot).
- **Linked anchors**: none.

### digging-deeper/audit-log.md
- **Public API covered**:
  - `rustcms::audit::{AuditEntry, AuditActor, AuditContext, RequestMeta, record, diff}`
  - `ctx.audit(txn, entry)`, `.audit_redact()`, `ResourceMeta::audit(false)`
  - `[audit] retention_days`, `sign_in_failures_per_minute`
  - `audit:prune`
  - permissions `audit.view_all`, `audit.view_auth`, `audit.view`
  - API `GET /admin/api/audit`, `…/history`
- **Wrong or outdated**:
  - l.153: `AuditActor::user(&auth.0.principal)` compiles, but `auth.principal()` is the accessor (`crates/rustcms-auth/src/guards/guard.rs:48-50`).
  - The event list matches the code; every listed `AuditEntry::new(..)` event exists in `crates/*`.
  - The defaults and limits match: `crates/rustcms-core/src/audit.rs:31-40,55` (365 days, 600 per minute, `MAX_TEXT` 1000, `MAX_BYTES` 64 KiB).
- **Missing**:
  - The other public constructors: `AuditActor::job` / `system` / `guest` / `signed_in` (`audit.rs:143-182`) and `AuditContext::system` (`audit.rs:199`).
  - `audit::record_all` (`audit.rs:274`), `record_detached` (best effort, outside a transaction; `audit.rs:288`), `audit::bounded`, `summary`, `changes_of`, `subject_key` (`audit.rs:327-438`).
  - `ctx.audit_context` is a public field. The LMS uses it as `rustcms::audit::record(txn, &ctx.audit_context, entry)` (`examples/lms/src/admin/resources/lesson_resource.rs:92-95`).
  - `MAX_DEPTH` 8 and `MAX_ITEMS` 100 (`audit.rs:33-35`); the page says only "long lists and deep values are cut".
  - The `retention_days` range 1..36500 is in the table, but boot validation goes through `AuditConfig::of` (`audit.rs:61`).
- **Examples available**:
  - `examples/lms/src/admin/resources/lesson_resource.rs`: fn `reorder` (records `updated` with `changes` for other rows, in the action's transaction).
  - No example uses `.audit_redact()` or `audit(false)`.
- **Screenshots needed**:
  - **System → Audit log** with filters.
  - A record's **History** tab.
  - Both are available in local `examples/lms` (lesson Move up/down entries) or the demo. RBlog shows realistic editorial history.
- **Linked anchors**: `#viewing-the-log` (from `admin/overview.md:32`, `reference/admin-api.md:43`).

### digging-deeper/queues.md
- **Public API covered**:
  - `rustcms::queue::{Job, JobCtx, Failure, dispatch, dispatch_with, enqueue, once, manage::{counts, retry, forget, cancel}, Clock}`
  - `Job::{KIND, VERSION, QUEUE, AT_MOST_ONCE, max_attempts, backoff, timeout, describe, upgrade, handle}`
  - `app.job::<J>()`, `ctx.dispatch`, `ctx.audit_context()`
  - commands `queue:work [--queue] [--once]`, `queue:failed`, `queue:retry`, `queue:forget [--all]`
  - every `[queue]` key
  - permissions `queue.view`, `queue.manage`
- **Wrong or outdated**:
  - l.33-37: `make:job` registers `app.job::<crate::jobs::<module>::<Name>>();` at the `// rustcms:jobs` marker (`crates/rustcms-cli/src/make.rs:87`), not `app.job::<SendWelcome>();`.
  - The config table omits several validated ranges: `max_timeout_secs` 1..86400, `unknown_kind_grace_secs` 0..604800, `failed_retention_days` 1..3650 (`crates/rustcms-core/src/queue/mod.rs:243-251`).
- **Missing**:
  - `queue:work --workers <1..64>` (`queue/commands.rs:41-45`; only mentioned in `operations/queue.md`).
  - `--once` runs at most 1,000 jobs (`commands.rs:39,52`).
  - The `--queue` option is repeatable (`commands.rs:30-33`).
  - Queue names are 1–64 of `a-z 0-9 _ . -`; `KIND` allows no `-` (`queue/mod.rs:220-228`).
  - The payload limit `MAX_PAYLOAD_BYTES` (`mod.rs:41`) is stated, but the constant is not named.
  - `once` keys are 1..191 bytes (`mod.rs:465-467`).
  - `dispatch`, `dispatch_with` and `enqueue` return the job's `Uuid` (`mod.rs:442-459`).
  - `JobCtx` fields `attempt` and `initiated_by` (`mod.rs:153-163`).
  - `Failure` has public `code` / `message` fields and `Permanent` (`mod.rs:111-133`).
  - `queue::run_until_idle(state, queues, max)` and `Workers::start` (`queue/worker.rs:418,450`); `run_until_idle` is only shown in mail.md and testing.md.
  - `JobStatus` / `QueueCounts` (`queue/manage.rs:16,114`).
- **Examples available**:
  - `examples/lms/src/jobs/send_welcome.rs`: `SendWelcome` impl (`Failure::from(err).permanent()`, receipt via `queue::once`).
  - `examples/lms/src/admin/resources/enrollment_resource.rs:107`: `ctx.dispatch` in a hook.
  - `examples/lms/src/providers/app_service_provider.rs`: `register` (`app.job::<…>()`).
  - `examples/demo/src/jobs/post_digest.rs`: `PingPosts` (`describe`).
- **Screenshots needed**:
  - **System → Queue**: counts, Jobs tab, History tab, job detail dialog, a failed job with Retry/Forget.
  - Local demo or LMS (enrol a learner, or Run now `posts-ping`).
  - RDockPanel's deploy jobs (`describe` like "deploy demo/web #12") are the best live illustration.
- **Linked anchors**:
  - `#the-queue-page` (`release-notes.md:105`, `admin/overview.md:33`, `scheduling.md:120`, `reference/database-schema.md:167`, `reference/admin-api.md:54`)
  - `#failures` (`scheduling.md:52`)
  - `#configuration` (`getting-started/configuration.md:97`)
  - `#changing-a-job` (same-page link, l.43)

### digging-deeper/scheduling.md
- **Public API covered**:
  - `rustcms::schedule::Schedule::{every, cron, job, run, task, timeout, without_overlapping}`
  - `TaskCtx {state, run_id, trigger, initiated_by}`, `Trigger`
  - `app.schedule`
  - `[schedule] enabled`, `grace_secs`, `history_retention_days`
  - `schedule:list`, `schedule:run <name>`
  - permissions `schedules.view`, `schedules.manage`
  - job `rustcms.schedule.run`, audit event `schedule.run`
- **Wrong or outdated**:
  - l.11 and l.45 use `Schedule::cron(..)?` inside a provider's `register`. `ServiceProvider::register` returns `()`, so `?` does not compile there. The demo uses `.expect("a valid cron expression")` (`examples/demo/src/jobs/post_digest.rs:37-38`).
  - l.7 places schedules in `app_service_provider.rs (register)`. The demo keeps them in `src/jobs/post_digest.rs::schedule`, called from `bootstrap/mod.rs:13`. That is valid, but the page should show the real layout.
  - l.154-155 ("cleanups … still run as provider loops") is still true (`crates/rustcms-admin/src/panel/provider.rs:403-456`), but it omits the auth loop: sessions, JWT, tokens, audit (`crates/rustcms-auth/src/provider.rs:200`).
- **Missing**:
  - No `[schedule]` config table. Ranges: `grace_secs` 1..300, `history_retention_days` 1..3650 (unlike the queue's 0..3650) (`crates/rustcms-core/src/schedule/mod.rs:419-446`).
  - Boot panics:
    - a schedule without an action (`mod.rs:390`);
    - `without_overlapping` on a `.run` or `.task` schedule (`mod.rs:392-396`);
    - a duplicate name (`mod.rs:389`);
    - a period that is out of range or not whole seconds (`mod.rs:172-176`);
    - a `timeout` of 0 or more than 1 day (`mod.rs:281-287`).
  - `ScheduleError::{Cron, Never}` (`mod.rs:47`).
  - Accessors: `Schedule::name`, `describe`, `next_after`, `action`, `run_timeout`; `Schedules::all` / `get`; constants `MIN_PERIOD` / `MAX_PERIOD` / `DEFAULT_TIMEOUT` (`mod.rs:37-43,301-330,406-411`).
  - `schedule_slots` claims are kept 7 days (`schedule/runner.rs:24`); this is in retention.md only.
- **Examples available**:
  - `examples/demo/src/jobs/post_digest.rs`: fn `schedule` (`every` + `.job` + `without_overlapping`; `cron` + `.task` + `.timeout`) and `PingPosts`.
- **Screenshots needed**:
  - **System → Schedules**: list, a schedule's runs dialog, Run now confirmation.
  - The local demo has `posts-ping` and `posts-digest` with a summary ("N posts").
  - RDockPanel (docker-prune, database-backups task summaries) is the live counterpart.
- **Linked anchors**: `#the-schedules-page` (`release-notes.md:108`, `queues.md:188`, `reference/database-schema.md:168`, `admin/overview.md:34`, `reference/admin-api.md:48`; same-page l.42).

### digging-deeper/mail.md
- **Public API covered**:
  - `rustcms::mail::{MailProvider, MailJob, Message, MailError, send_later, render, send}`
  - `rustcms::mail_template!`, `app.mail_template`
  - `AdminProvider::mail_settings()`
  - `[mail]` keys `driver`, `source`, `allowed_hosts`, `allowed_ports`, `allow_insecure`, `timeout_secs`, `max_per_minute`, `log_body`, `tls_ca_file`
  - `[mail.smtp]` keys `host`, `port`, `encryption`, `username`, `from_address`, `from_name`, password via `RUSTCMS_MAIL__SMTP__PASSWORD`
  - `TestApp::mails` / `take_mails`, `queue::run_until_idle`
- **Wrong or outdated**: none found. The `mail.send` job has 8 attempts and a 120 s timeout (`crates/rustcms-core/src/mail/mod.rs:401-413`). The Mailpit ports match `docker-compose.yml:67-68`.
- **Missing**:
  - `[mail.smtp] reply_to` (`mail/mod.rs:86`). `SmtpConfig` uses `deny_unknown_fields`, so a typo fails boot (`mod.rs:68-69`).
  - Ranges: `timeout_secs` 1..300, `max_per_minute` 1..10000 (`mod.rs:152-153`).
  - `allowed_ports` defaults to `[465, 587]` (`mod.rs:133`).
  - `send_later` accepts 1–50 recipients (`MAX_RECIPIENTS`, `mod.rs:38,399-401`).
  - `MAX_DATA_BYTES` 64 KiB (`mod.rs:36`).
  - `mail::mailer(&state)` / `Mailer::sent` / `take_sent` (`mod.rs:335-370`).
  - `MailError::is_permanent()` and `message()` (`mod.rs:258-273`).
  - `Failure::from(MailError)` is the pattern for custom mail jobs (`examples/lms/src/jobs/send_welcome.rs:50-55`).
  - `MailSettings` (the settings group, `mod.rs:187`).
  - `mail::valid_address` (`mod.rs:452`).
  - `mail_template!` without `html` (`mail/template.rs:45-46`).
  - The `log` driver logs the template name too (`mod.rs:50`).
- **Examples available**:
  - `examples/lms/src/providers/app_service_provider.rs:24-25`: `mail_template!` registration.
  - `examples/lms/src/jobs/send_welcome.rs`: `SendWelcome::handle` (render + send in a job of its own, permanent/temporary mapping).
  - `examples/lms/resources/views/mail/{en,vi}/welcome.*`: templates.
  - `examples/lms/config/mail.toml`, `examples/demo/config/mail.toml`.
  - `examples/demo/src/bootstrap/providers.rs` region `register` (`MailProvider`).
  - No example calls `send_later`.
- **Screenshots needed**: **Settings → Mail** with *Send test mail* and an error code (local demo or LMS; RBlog live). A Mailpit inbox shot is optional.
- **Linked anchors**:
  - `#templates` (`architecture/providers.md:78`)
  - `#configuration` (`getting-started/configuration.md:93`)
  - The page links out to `../reference/error-codes.md#mail`.

---

## Batch 13: operations

### operations/queue.md
- **Public API covered**:
  - `queue.workers`, `queue.enabled`, `queue:work [--queue] [--workers]`
  - `queue.shutdown_secs`, `queue:failed`, `queue:retry`, `queue:forget`
  - `queue.unknown_kind_grace_secs`, `queue.failed_retention_days`, `queue::once`
- **Wrong or outdated**: none found. `--workers` takes 1..64 (`crates/rustcms-core/src/queue/commands.rs:42-45`).
- **Missing**:
  - `queue:work --once` (cron-style; at most 1,000 jobs; prunes first) (`commands.rs:36-39,51-54`).
  - Built-in job kinds an operator sees: `mail.send`, `auth.password_reset` (5 attempts), `auth.invitation` (8), `rustcms.schedule.run`, and the export/import jobs (`crates/rustcms-auth/src/recovery/jobs.rs:86-89,132-135`).
  - The History tab and `queue.history_retention_days`.
  - The Cancel action.
- **Examples available**: none (operational page).
- **Screenshots needed**: the Queue page's failed filter with Retry/Forget (local demo/LMS; RDockPanel live).
- **Linked anchors**: none.

### operations/retention.md
- **Public API covered**:
  - `session_lifetime_minutes`, `remember_days`, `refresh_family_days`, `audit.retention_days`
  - `queue.failed_retention_days`, `queue.history_retention_days`, `schedule.history_retention_days`
  - commands `session:prune`, `audit:prune`, `idempotency:prune`, `media:prune`
- **Wrong or outdated**:
  - l.11: JWT sign-ins are pruned by "`serve` hourly" only, but `session:prune` also deletes ended JWT sign-ins (`crates/rustcms-auth/src/console/commands.rs:164-171`).
  - l.12: no command prunes reset and invitation links (`recovery::prune` runs only in the serve loop, `crates/rustcms-auth/src/provider.rs:217`). That matches the page, but the cron section should say so.
  - l.19-20: exports and imports are pruned every 10 minutes (`EXPORTS_PRUNE_EVERY`, `crates/rustcms-admin/src/panel/provider.rs:456`), and no command does it. The cron list (l.27-32) cannot replace that when `serve` is not running.
- **Missing**: `queue:work --once` also prunes failed jobs and receipts (`crates/rustcms-core/src/queue/worker.rs:264,284`), so it belongs in the cron list.
- **Examples available**: none.
- **Screenshots needed**: none.
- **Linked anchors**: none.

### operations/backup-restore.md
- **Public API covered**: `storage.root`, `APP_KEY`, `rustcms migrate:status`, `rustcms migrate`; tables `sessions`, `jwt_refresh_tokens`, `jwt_families`, `user_tokens`, `jobs`.
- **Wrong or outdated**:
  - None in the table and job names (they exist: `crates/rustcms-auth/src/recovery/jobs.rs:86,132`; `crates/rustcms-core/src/mail/mod.rs:403`).
  - l.9-10: "without it, saved secrets … cannot be decrypted" also applies to **encrypted columns** (ADR 0030), which the page does not mention.
- **Missing**:
  - Encrypted app columns (`Encrypted<T>`) need the same `APP_KEY`, or a key in `APP_PREVIOUS_KEYS`. That lets a backup made before a rotation still be restored (`crates/rustcms-core/src/config.rs:55-59`).
  - `migrate --allow-newer` when restoring an older binary over a newer schema (`crates/rustcms-core/src/console.rs:30-35`).
  - Revoking API tokens: the SQL for `personal_access_tokens.revoked_at`.
  - The `schedule_states` / `schedule_slots` / `job_runs` / `schedule_runs` tables need no special handling.
- **Examples available**: none.
- **Screenshots needed**: none.
- **Linked anchors**: none (`reference/database-schema.md:135` links the page without an anchor).

### operations/app-key.md
- **Public API covered**: `APP_KEY`, `APP_PREVIOUS_KEYS`, `rustcms key:generate --show`, `key:reencrypt [--dry-run] [--list]`, `personal_access_tokens.revoked_at`, *Sign out other sessions*.
- **Wrong or outdated**:
  - l.31-32: "until it reports 0 to move, 0 changed and 0 unreadable. It exits non-zero until then." A real run succeeds when it moved rows and nothing changed or was unreadable. Only `--dry-run` fails while rows remain to move (`crates/rustcms-core/src/crypto/columns.rs:184-186`).
  - l.39: *Sign out other sessions* revokes the API tokens (`logout_everywhere` → `revoke_all(.., true)`, `crates/rustcms-auth/src/guards/credentials.rs:85-97`). Correct, and the route is `POST {admin}/api/auth/logout-others` (`crates/rustcms-auth/src/http/routes.rs:45`).
- **Missing**:
  - `key:generate --force` (`crates/rustcms-core/src/console.rs:86-89`).
  - `key:reencrypt --batch` (`columns.rs:293-299`).
  - `app.previous_keys` can also be set in `config/app.toml` (`config.rs:129-144`).
  - The `_FILE` secret variants (`APP_KEY_FILE`, …; `config.rs:259-264`).
  - A key given twice, or equal to the current key, fails boot (`config.rs:485-492` tests).
- **Examples available**: none.
- **Screenshots needed**: none.
- **Linked anchors**: none (`encryption.md:67` links without an anchor).

### operations/mail.md
- **Public API covered**: *Send test mail*; codes `smtp_auth`, `smtp_tls`, `mail_policy`, `mail_settings_unreadable`; `mail.send`; `queue:retry --all`; `auth.password_reset_minutes` (60); `auth.invitation_days` (7); `mail.max_per_minute`; `RUSTCMS_MAIL__LOG_BODY`.
- **Wrong or outdated**: none. The attempts (reset 5, invitation 8, `mail.send` 8) match `crates/rustcms-auth/src/recovery/jobs.rs:88-90,134-136` and `crates/rustcms-core/src/mail/mod.rs:405-407`. The defaults match `crates/rustcms-auth/src/config.rs:110-111`.
- **Missing**:
  - Invitation links live at most 30 days (`MAX_INVITATION_DAYS`, `crates/rustcms-auth/src/recovery/mod.rs:44-45`).
  - A reset job older than its window fails as `expired_before_sent` (`recovery/jobs.rs:101-103`).
  - The reset cooldown is `auth.reset_cooldown_secs` (`recovery/jobs.rs:110`).
  - In production, `allowed_hosts` must be set or boot fails (`mail/mod.rs:159-163`), which is an operations concern.
  - The TLS Mailpit runs on port 51026 (`docker-compose.yml:81`).
- **Examples available**: none.
- **Screenshots needed**: **Settings → Mail**, test mail result with an error code (local demo or LMS).
- **Linked anchors**: none.

---

## Batch 14

### examples/lms.md
- **Public API covered**:
  - `ResourceMeta::lock_parent`, `ResourceMeta::unique`, `scope()`, `ctx.dispatch`, `ctx.records`, `ctx.t`
  - `view_depends_on_record`, `admits`
  - `lang:missing`, `db:seed`, `key:generate`
  - `RUSTCMS_TEST_PG`, `RUSTCMS_TEST_MYSQL`
  - permissions `lms.all_courses`, `courses.assign_instructor`, `enrollments.update_progress`
- **Wrong or outdated**:
  - l.15-18 lists four accounts. The seeder also creates `admin@example.test` (super admin) and `tutor@example.test` (a second instructor) (`examples/lms/src/database/seeders/user_seeder.rs:17-22`). The seeder's own hint names admin@ (`seeders/mod.rs:39`).
  - l.60 gives the `check` signature as untyped pseudo-code. The real one starts at `examples/lms/src/services/enrollment_service.rs:58`; import it instead.
  - l.50: `tests/feature/` also has `customizing.rs`, which the page does not list.
- **Missing**:
  - `src/routes/api.rs`: `ApiRoutes::new("/api/v1").get("/ping", Access::public(), ping)` (`examples/lms/src/routes/api.rs:11-13`).
  - `enrollment_service::lock_course` / `active_count` (`enrollment_service.rs:49,89`); `Refusal::reason` / `errors` (`:31,41`).
  - `enrollments.enrolled_at` (used by the weekly chart, `examples/lms/src/models/enrollment.rs:22`; `providers/admin_panel_provider.rs:46`).
  - `resources/views/admin/course_seats.html` (the slot template).
  - The panel's `.theme(..)` and `.mail_settings()` (`admin_panel_provider.rs:17,36`).
  - `config/mail.toml` uses `driver = "log"`, `source = "config"`.
- **Examples available** (the whole page could import these):
  - `services/enrollment_service.rs`: `check`, `enroll_in`, `enroll`, `Refusal`.
  - `admin/resources/lesson_resource.rs`: `scope`, `reorder`.
  - `admin/resources/enrollment_resource.rs`: `meta` with `lock_parent` + `unique`; `scope`; the hook at l.107.
  - `models/course.rs`: `taught_by`.
  - `policies/enrollment_policy.rs`.
  - `policies/permissions.rs`.
  - `jobs/send_welcome.rs`.
  - `providers/admin_panel_provider.rs`: widgets.
  - `admin/pages/course_overview_page.rs`: `data`.
  - `admin/slots/course_seats_slot.rs`.
  - `resources/admin/js/lms/progress.js`.
  - `tests/feature/enrollments.rs`.
- **Screenshots needed** (all local `examples/lms`):
  - Courses list as staff vs teacher.
  - The *Enroll a learner…* dialog with a 422 "no seat".
  - The course page with the Seats slot.
  - The Course overview custom page.
  - The enrollments table progress bars.
  - Lessons with Move up/down.
  - News editor with cover and tags.
  - The dashboard widgets.
  - The theme (blue, logo).
- **Linked anchors**: none (the page links out to `../admin/resources.md#parent-locks`).

---

## Batch 15: reference

### reference/database-schema.md
- **Public API covered**:
  - Framework tables: `users`, `roles`, `permissions`, `role_has_permissions`, `model_has_roles`, `model_has_permissions`, `auth_generations`, `sessions`, `personal_access_tokens`, `jwt_families`, `jwt_refresh_tokens`, `user_tokens`.
  - Other tables: `seaql_migrations`, `rustcms_locks`, `idempotency_keys`, `media*`, `audit_logs`, `settings`, `schedule_states`, `schedule_slots`, `jobs`, `job_receipts`, `job_runs`, `schedule_runs`, `exports`, `export_objects`, `imports`, `import_rows`.
  - `RbacService`, `permission:sync`.
- **Wrong or outdated**:
  - The columns checked against the migrations match: users (`crates/rustcms-migration/src/m20261011_000001_create_users_table.rs:21-29`, `m…000011`, `m…000018`), rbac (`m20261012_000004_create_rbac_tables.rs`), media incl. `sha256` and `alt` (`m20261014_000007…:26`, `m20261201_000022`).
  - l.131 `user_tokens` omits `request_id`, `requested_at` and `issued_at` (`m20261025_000019_create_user_tokens_table.rs`), and `purpose` varchar(32).
  - l.166 `jobs` omits `version`, `max_attempts`, `reserved_by`, `initiated_by`, `failed_at` (`m20261019_000014_create_jobs_tables.rs`; `m20261120_000021_add_run_history.rs`).
- **Missing**:
  - Key lengths/types: `sessions.id` varchar(128), `lineage` varchar(64); `jwt_families.id` varchar(32); `personal_access_tokens.token_hash` varchar(64) (migrations 000002, 000005, 000010, 000003).
  - Schedule columns: `schedule_states` `active_job_id` / `last_slot_at`; `schedule_slots` `instance` / `job_id` (`m20261101_000020`).
  - `job_runs` / `schedule_runs` columns (`m20261120_000021`).
  - `imports.sha256`, `imports.started_at` (`m20261023_000016`, `m20261024_000017`).
  - `rustcms_locks` rows: names `rbac`, `lineage:<slot>`, `idem:<slot>`, 1024 slots (`crates/rustcms-core/src/db/locks.rs:6,11-13`).
  - App tables in the examples (`integrations` with encrypted `Text` columns), if the page should show how app tables sit beside these.
- **Examples available**: none (reference). A migration-helper sample could import `examples/demo/src/database/migrations/m20261101_000001_create_integrations.rs`.
- **Screenshots needed**: none (an ER diagram exists as ASCII; consider SVG).
- **Linked anchors**: none.

### reference/admin-api.md
- **Public API covered**: every route in `rustcms_admin::http::routes` (`crates/rustcms-admin/src/http/mod.rs:101-137`) plus the feature routes:
  - system (`features/system/mod.rs`)
  - settings (`features/settings/http.rs`)
  - audit (`features/audit.rs`)
  - queue (`features/queue.rs`)
  - schedules (`features/schedules.rs`)
  - pages/slots (`features/custom/http.rs`)
  - exports (`features/exports/mod.rs`)
  - imports (`features/imports/mod.rs`)
  - dashboard (`features/dashboard/mod.rs`)

  All are present. Also covered: `Idempotency-Key`, the list query (`filter[f][op]`, `trashed`), the `Envelope` shape, and limits.
- **Wrong or outdated**: none in the route table. The `PanelResponse` fields match `http/mod.rs:36-61`. The limits match `resources/query.rs:107-110`, `engine/idempotency.rs:21-24,112` and `engine/adapter/mod.rs:619`.
- **Missing**:
  - `/system/…` is only "…". The routes are `GET/POST /system/users`, `GET/PATCH/DELETE /system/users/{id}`, `PUT …/roles|active|password`, `POST /system/users/invitations`, `POST /system/users/{id}/invitation`, `GET/POST /system/roles`, `GET/DELETE /system/roles/{id}`, `PUT /system/roles/{id}/permissions`, `GET /system/permissions` (`features/system/mod.rs` routes fn).
  - Auth routes under the same prefix, `{admin}/api/auth/{csrf,login,logout,me,password,logout-others,locale,forgot-password,reset-password,invitation}` (`crates/rustcms-auth/src/http/routes.rs:37-49`). These belong in a link to security/authentication.
  - Application `AdminRoutes` share `{admin}/api/x`, including `events` (SSE) and `socket` (ADR 0026/0027/0029). Link `admin/admin-routes.md` and `admin/realtime.md`.
  - Attach/detach take at most 100 keys (`MAX_LINK_KEYS`, `engine/adapter/many.rs:17,160`). The Limits table says 500 keys for actions and lookups only.
  - A list that checks the policy record by record scans at most 10,000 rows (`MAX_LIST_SCAN`, `engine/adapter/mod.rs:631`). That is the `too_many_matches` error.
  - Page sizes offered: 10/25/50/100 (`engine/adapter/mod.rs:617`).
  - After a commit, the record envelope may be replaced by the key alone after 2 s (`POST_COMMIT_HYDRATION`, `engine/adapter/mod.rs:623-626`).
- **Examples available**: none (API reference). The fixture `fixtures/authz/admin.toml` lists every route with access and could feed a generated table.
- **Screenshots needed**: none.
- **Linked anchors**: none. The page links out to `../admin/relationships.md#belongs-to-many-pivot-tables`, `#choosing-links-in-the-records-form`, `../admin/actions.md#actions-with-a-form`, `../admin/import-export.md#exporting`/`#importing`, `../admin/dashboard.md#api`, `../admin/settings.md#api`/`#mail`, `../admin/customizing.md#custom-pages`, `../security/authentication.md#csrf`, and the audit/queue/scheduling anchors above.

### reference/error-codes.md
- **Public API covered**:
  - All 108 validation codes of `rustcms_core::i18n::TEMPLATES` (`crates/rustcms-core/src/i18n/issues.rs:41`). Checked by script: every code in the doc exists, every template code is in the doc, and every English template text appears except four `document_needs_version` variants, which l.101 summarises.
  - Top-level `AppError` codes and `MailError` codes.
- **Wrong or outdated**: none found.
- **Missing**:
  - **408** from the request timeout layer (`http.timeout_secs`, `crates/rustcms-core/src/http/mod.rs:213`). It has an empty body and no JSON `code`. storage.md mentions it; the table should too.
  - **503** `unavailable` from realtime streams/sockets during shutdown (`crates/rustcms-auth/src/realtime/mod.rs:306-307`).
  - **503** from `/health` when the database is unavailable (`crates/rustcms-core/src/http/mod.rs:184`).
  - The internal conflicts `import_row_written` / `import_stopped` (`crates/rustcms-admin/src/features/imports/mod.rs:693-696`) never reach clients. List them as internal only if they can surface.
  - The `retry` hint: `TooManyRequests` sets `Retry-After` (`crates/rustcms-core/src/error.rs:108-112`).
  - `custom` code: `rustcms_core::i18n::CUSTOM` (`i18n/mod.rs:15`).
- **Examples available**: none (could be generated from `TEMPLATES`, which already has a consistency test in `crates/rustcms-admin/tests/issues.rs`).
- **Screenshots needed**: none.
- **Linked anchors**: `#mail` (`digging-deeper/mail.md:117`).

### reference/wire-format.md
- **Public API covered**: `FieldKind` String / I32 / I64 / F64 / Bool / Decimal / Uuid / Date / LocalDateTime / Instant / Json and their column types (`crates/rustcms-admin/src/engine/wire.rs:26-79`).
- **Wrong or outdated**: none. The mapping matches `from_column_type` (`wire.rs:64-79`); decimals are rescaled to the column scale (`wire.rs:219-231`).
- **Missing**:
  - Column types the admin does **not** support, and which therefore cannot be fields: `SmallInteger`, `TinyInteger`, unsigned, binary, enums, arrays (`wire.rs:77` `_ => return None`).
  - The schema's `valueType` names `string`, `i32`, `i64`, `f64`, `bool`, `decimal`, `uuid`, `date`, `local_datetime`, `instant`, `json` (`wire.rs:82-96`), which custom widgets and the TS types use.
  - Query-string filters are parsed by kind (`from_query`, `wire.rs:98-108`; covered in admin-api.md).
  - Key encoding: `encode_key`, keys stringified (`wire.rs:234-240`).
  - `BigUnsigned` encodes as a string (`wire.rs:198`).
  - Encrypted fields never travel (`null`), with a link to encryption.md.
  - Rich-text documents are `Json` with a closed schema (ADR 0008).
- **Examples available**: `fixtures/contract/*.json` shows the real wire values; `crates/rustcms-admin/bindings` / TS types if generated.
- **Screenshots needed**: none.
- **Linked anchors**: none.

### reference/limitations.md
- **Public API covered**: `BelongsToManySelect`, `lock_parent`, `max_per_minute`, the settings kinds, `role:assign`, MariaDB, Redis Cluster.
- **Wrong or outdated**:
  - l.54-55, the size: the committed panel bundle is now **1.36 MB of JS (≈417 KB gzipped) + 42 KB CSS** (`crates/rustcms-admin/assets/assets/index-BjGK1QfM.js`, `index-DChiodaT.css`), not "about 0.55 MB (175 KB gzipped)". The binary size was not re-measured, so it needs a release build to confirm the 29/23 MB figures.
  - l.15-16, settings kinds: these match `SettingKind` (`crates/rustcms-admin/src/features/settings/mod.rs:34-45`).
  - l.25, API-guard roles: still panel-admin-only (`features/system/roles.rs:112,129,153` filter `ADMIN_GUARD`). Correct.
- **Missing**:
  - Unsupported column types: smallint, tinyint, unsigned, binary, enum (`engine/wire.rs:77`).
  - The 10,000-row scan limit for per-record policies (`MAX_LIST_SCAN`).
  - Attach/detach take at most 100 keys.
  - Mail allows at most 50 recipients (`MAX_RECIPIENTS`).
  - Encrypted values are at most 32 KiB and cannot be searched, sorted or filtered (encryption.md).
  - Cron is UTC only.
  - Schedules run only in `serve`.
  - Provider cleanup loops are not yet schedules.
  - Branding and realtime are per instance (instances.md).
  - No presigned or public media URLs (storage.md says so; limitations l.21 only says "public media URLs").
- **Examples available**: none.
- **Screenshots needed**: none.
- **Linked anchors**: none (the page links out to `../basics/database.md#mysql`).

---

## Features in this area without a guide page

- **Redis configuration** (`[redis] url` / `prefix`, feature `redis`, `crates/rustcms-core/src/redis.rs`): only a few lines in instances.md, with no config table.
- **Storage trait for application code** (`rustcms::storage::{Storage, StorageKey, of, connect}`, `crates/rustcms-core/src/storage/mod.rs:52-201`): no page shows storing or serving your own files.
- **`rustcms::jobs::exclusive`** has two paragraphs in instances.md; there is no reference.
- **Queue worker API for tests and tools** (`Workers::start`, `run_until_idle`, `queue::manage::*`, `JobStatus`, `QueueCounts`): scattered over queues.md and testing.md, with no reference section.
- **Built-in job kinds** (`mail.send`, `auth.password_reset`, `auth.invitation`, `rustcms.schedule.run`, the export/import jobs) and **built-in background loops** (auth-prune, media-prune, exports-prune, idempotency-prune with their periods): no single operations page lists them.
- **Health endpoint** (`/health`, 503 "degraded", `crates/rustcms-core/src/http/mod.rs:184`): only in deployment/lifecycle, not in the operations section.
- **`app.previous_keys` / `_FILE` secret variables**: only configuration.md, with no operations note.
- **Settings secrets re-encryption** (`reencrypt_settings`, `crates/rustcms-core/src/crypto/columns.rs`): mentioned nowhere on the encryption pages.
- `admin:branding` (CLI, `crates/rustcms-admin/src/panel/provider.rs:398`) appears only in the generated cli.md.
