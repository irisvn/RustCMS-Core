# Database schema

The framework's migrations create the tables below. Use this page when you
write reports or support queries, plan a backup, or check what a table holds
before you touch it. Your own tables live next to them in the same database.

`rustcms migrate` runs the framework's migrations before yours: an
application's migrator starts from `rustcms::migration::framework()`:

<<< @/../../../examples/demo/src/database/migrations/mod.rs#migrator [src/database/migrations/mod.rs]

Column types are shown portably:

- **instant**: `TIMESTAMPTZ` on PostgreSQL, `DATETIME(6)` in UTC on MySQL, text
  on SQLite;
- **bigint id**: auto-increment `BIGINT`;
- **uuid**: `UUID` on PostgreSQL, the database's UUID mapping elsewhere;
- **json**: `JSON` (`JSONB` is not used).

Never write the access-control tables yourself: every change to roles,
permissions and their assignments goes through `RbacService`
([Roles & permissions](../security/authorization.md)), which checks the
actor, locks, audits and invalidates the permission cache.

## Users and access

```text
                          ┌──────────────────────────┐
                          │ users                    │
                          │ id, email (unique), …    │
                          │ auth_version, is_active  │
                          └───────────┬──────────────┘
         (model_type = 'user', model_id = users.id as text)
              ┌───────────────────────┼──────────────────────────────┐
              ▼                       ▼                              ▼
┌──────────────────────────┐ ┌───────────────────────────┐ ┌───────────────────────────┐
│ model_has_roles          │ │ model_has_permissions     │ │ sessions / tokens / JWT   │
│ role_id, model_type,     │ │ permission_id, model_type,│ │ personal_access_tokens    │
│ model_id                 │ │ model_id   (direct grants)│ │ jwt_families → jwt_refresh│
└───────────┬──────────────┘ └─────────────┬─────────────┘ │ user_tokens (reset/invite)│
            ▼                              ▼               └───────────────────────────┘
┌──────────────────────────┐ ┌───────────────────────────┐
│ roles                    │ │ permissions               │
│ name + guard_name unique │ │ name + guard_name unique  │
│ is_system, is_privileged │ │ group, orphaned           │
└───────────┬──────────────┘ └─────────────▲─────────────┘
            └──────► role_has_permissions ─┘
                     (role_id, permission_id)
```

A user's permissions in a guard are those of their roles in that guard, plus
their direct grants. Orphaned permissions are left out. `super_admin` passes
every check of its guard.

### `users`

| Column | Type | Notes |
|---|---|---|
| `id` | bigint id | primary key |
| `name` | varchar(255) | |
| `email` | varchar(255) | unique (`users_email_key`); stored trimmed and lowercased |
| `password` | varchar(255) | Argon2id hash, never the password |
| `is_active` | boolean, default true | false: cannot sign in, every session and token stops at once |
| `auth_version` | bigint, default 0 | bumped by account changes (password, email, sign-out everywhere, …): sessions and JWT sign-ins issued for an older version stop working ([revocation](../security/revocation.md)). Role and permission changes do not bump it: they apply from the next request through `auth_generations` |
| `email_verified_at` | instant, null | |
| `locale` | varchar(35), null | admin language chosen by the user; null follows the browser |
| `invitation_pending` | boolean, default false | invited, password not chosen yet: cannot sign in or reset |
| `invited_at` | instant, null | |
| `invited_by` | bigint, null | the inviting user's id, without a foreign key (that user may be deleted) |
| `created_at`, `updated_at` | instant | |

Deleting a user cascades to their tokens, JWT families and one-time tokens.
Their role and permission assignments are removed by `RbacService` in the
same transaction: `model_has_*` hold no foreign key to `users`.

### `roles`

| Column | Type | Notes |
|---|---|---|
| `id` | bigint id | |
| `name` | varchar(255) | unique together with `guard_name` (`roles_name_guard_key`) |
| `guard_name` | varchar(64) | `admin` (the panel) or `api` (tokens and JWT); the guards never mix |
| `is_system` | boolean | made by the migrations: never renamed or deleted |
| `is_privileged` | boolean | managed by super admins only |
| `created_at` | instant | |

The migrations seed `super_admin` in each guard (`is_system` and
`is_privileged`). Other roles are yours: created in the panel (admin guard), by
seeders, or from code (`RbacService::create_role`).

### `permissions`

| Column | Type | Notes |
|---|---|---|
| `id` | bigint id | |
| `name` | varchar(255) | `posts.update`, `users.invite`, …; unique with `guard_name` (`permissions_name_guard_key`) |
| `guard_name` | varchar(64) | as for roles |
| `group` | varchar(255), null | for display, usually the resource slug |
| `orphaned` | boolean, default false | no longer declared by the application: kept, grants nothing |
| `created_at` | instant | |

Rows come from the permissions the application declares (resources,
`AuthProvider::permission`). `rustcms permission:sync` adds the new ones and
marks the undeclared ones orphaned. It never renames or deletes, so a typo
cannot silently drop a grant.

### `role_has_permissions`, `model_has_roles`, `model_has_permissions`

| Table | Primary key | Foreign keys |
|---|---|---|
| `role_has_permissions` | (`role_id`, `permission_id`) | both, `ON DELETE CASCADE` |
| `model_has_roles` | (`role_id`, `model_type`, `model_id`) | `role_id` → `roles`, cascade |
| `model_has_permissions` | (`permission_id`, `model_type`, `model_id`) | `permission_id` → `permissions`, cascade |

- `model_type` is `user` and `model_id` is the user's id as text: the
  Spatie-style shape leaves room for other holders.
- `model_has_*` are indexed on (`model_type`, `model_id`) for "what does this
  user hold".
- Direct grants (`model_has_permissions`) exist for exceptions; roles are the
  usual way.

### `auth_generations`

| Column | Type | Notes |
|---|---|---|
| `name` | varchar(32) | primary key; `rbac` |
| `value` | bigint | advanced inside every RBAC transaction |

Instances cache resolved permissions per generation. A change anywhere
advances it, and every instance reloads ([several instances](../digging-deeper/instances.md)).

### Credentials

| Table | Holds | Stored secret | Notes |
|---|---|---|---|
| `sessions` | admin sign-ins (and anonymous CSRF sessions) | `id` varchar(128) = HMAC-SHA-256 of the cookie's random value, keyed by `APP_KEY` | `data` (JSON text), `expires_at` (indexed, pruned hourly), `lineage` varchar(64), null (the first session of a re-issued chain: sign-out ends all of it; indexed) |
| `personal_access_tokens` | API tokens (`rcms_…`) | `token_hash` varchar(64) = SHA-256 | `id` bigint, `user_id` → `users` (cascade, indexed), `name`, `abilities` (JSON list, `["*"]` = all of the user's permissions), `last_used_at`, `expires_at`, `revoked_at`, `created_at` |
| `jwt_families` | one JWT sign-in and its refresh chain | none | `id` varchar(32), `user_id` → `users` (cascade), `abilities`, `auth_version` at issue, `created_at`, `expires_at`, `revoked_at` |
| `jwt_refresh_tokens` | each refresh token of a family | `token_hash` varchar(64) = HMAC-SHA-256 under a key derived from `APP_KEY` | `id` bigint, `family_id` → `jwt_families` (cascade), `created_at`, `expires_at`, `used_at`: reusing a used token revokes the family |
| `user_tokens` | password-reset and invitation links | `token_hash` varchar(64) under a key derived from `APP_KEY` | see below |

#### `user_tokens`

| Column | Type | Notes |
|---|---|---|
| `id` | uuid | primary key; the public half of the token |
| `user_id` | bigint | → `users`, `ON DELETE CASCADE` |
| `purpose` | varchar(32) | `password_reset` or `invitation`; unique with `user_id` (`user_tokens_user_purpose_key`): one live link per user and purpose |
| `email` | varchar(255) | the user's email when it was issued: a change of email kills the link |
| `auth_version` | bigint | the user's `auth_version` when it was issued: a change kills the link |
| `token_hash` | varchar(64) | hex HMAC-SHA-256 of purpose, id and secret |
| `request_id` | uuid | the request (a queued mail job) the link answers: a retried job sends nothing new, an older request never replaces a newer link |
| `requested_at` | instant | when that request was made |
| `issued_at` | instant | when this secret was made |
| `expires_at` | instant | when it stops working; indexed, expired rows pruned hourly |
| `sent_at` | instant, null | when its mail was sent (a retry then sends nothing) |

A copy of these tables gives nobody a working credential: only hashes are
stored. Restoring a backup still requires ending every sign-in
([backup and restore](../operations/backup-restore.md)).

### Reading them

For reports or support, read-only SQL is fine. For example, everyone holding
a permission in the admin guard:

```sql
SELECT DISTINCT u.id, u.email
FROM users u
JOIN model_has_roles mr ON mr.model_type = 'user' AND mr.model_id = CAST(u.id AS CHAR(20))
JOIN roles r ON r.id = mr.role_id AND r.guard_name = 'admin'
LEFT JOIN role_has_permissions rp ON rp.role_id = r.id
LEFT JOIN permissions p ON p.id = rp.permission_id AND p.orphaned = FALSE
WHERE p.name = 'posts.update' OR r.name = 'super_admin';
```

(Direct grants: the same through `model_has_permissions`. On PostgreSQL, use
`CAST(u.id AS TEXT)`.) Writes go through `RbacService` or the System pages.

## Other framework tables

| Table | For | Page |
|---|---|---|
| `seaql_migrations` | applied migrations (SeaORM's own table) | [Database & migrations](../basics/database.md) |
| `rustcms_locks` | named transaction locks on MySQL ([below](#rustcms_locks)) | [Database & migrations](../basics/database.md#named-locks) |
| `idempotency_keys` | stored results of writes sent with `Idempotency-Key`, kept 24 hours | [Admin API](admin-api.md#retrying-writes-safely), [Retention](../operations/retention.md) |
| `media`, `media_uploads`, `media_deletions` | the media library; uploads being stored; files to delete from storage, with retries | [Media](../admin/media.md), [Retention](../operations/retention.md) |
| `audit_logs` | who changed what | [Audit log](../digging-deeper/audit-log.md) |
| `settings` | settings groups | [Settings](../admin/settings.md#secrets) |
| `jobs`, `job_receipts` | the database queue; receipts of effects applied once | [Queues](../digging-deeper/queues.md) |
| `job_runs` | one row per finished job, kept `queue.history_retention_days` | [Queues](../digging-deeper/queues.md#the-queue-page) |
| `schedule_states`, `schedule_slots` | one state row per schedule (never pruned); each claimed slot, kept 7 days | [Scheduling](../digging-deeper/scheduling.md) |
| `schedule_runs` | one row per schedule run, kept `schedule.history_retention_days` | [Scheduling](../digging-deeper/scheduling.md#the-schedules-page) |
| `exports`, `export_objects` | background CSV exports and every file a run may have written | [Import and export](../admin/import-export.md) |
| `imports`, `import_rows` | CSV imports, their mapping and report; what each row did | [Import and export](../admin/import-export.md) |

Ids made by the framework are UUID v7 (time-ordered) unless noted. Messages
stored as errors (`last_error`, `error`) are safe texts, never the raw error.

### `rustcms_locks`

| Column | Type | Notes |
|---|---|---|
| `name` | varchar(64) | primary key |

The migration inserts every row up front: `rbac`, and `lineage:0` …
`lineage:1023` and `idem:0` … `idem:1023` (names are hashed into 1,024
slots). On MySQL, `rustcms::db::xact_lock(txn, name)` locks the row with
`FOR UPDATE` until the transaction ends; a name without a row is an error, so
on MySQL only these names work. PostgreSQL takes an advisory lock on a hash of
the name and SQLite already holds its write lock: the table exists there only
so every database has the same schema.

### `idempotency_keys`

| Column | Type | Notes |
|---|---|---|
| `id` | varchar(64) | primary key: SHA-256 of the user (guard and id), the resource, the operation and the key |
| `owner` | varchar(255) | `<guard>:<user id>` (indexed): at most 1,000 live keys per owner |
| `request_hash` | varchar(64) | hash of the canonical request: a different request with the same key is refused |
| `result` | text, null | the stored outcome, replayed to a retry; committed with the write |
| `created_at`, `expires_at` | instant | `expires_at` indexed; `idempotency:prune` deletes expired rows |

### Media

| Table | Columns |
|---|---|
| `media` | `id` uuid, `disk` varchar(16), `key` varchar(128), `name` varchar(255), `mime` varchar(64), `size` bigint, `sha256` varchar(64), `width`, `height` (int, null), `alt` varchar(500) null, `owner_id` bigint (indexed, no foreign key), `created_at`, `deleted_at` (null; set: in the trash) |
| `media_uploads` | `id` uuid, `disk`, `key`, `created_at`: an intent committed before the object is written and removed with the media row, so a crash in between leaves a record of what to clean up |
| `media_deletions` | `id` uuid, `disk`, `key`, `attempts` int, `next_at` instant (indexed): objects to delete, retried until done |

### `audit_logs`

| Column | Type | Notes |
|---|---|---|
| `id` | uuid | |
| `at` | instant | |
| `event` | varchar(64) | `created`, `updated`, … |
| `subject_type`, `subject_key` | varchar(64), varchar(255) null | what changed |
| `actor_kind` | varchar(16) | `user`, `system`, `guest` or `job` |
| `actor_user_id` | bigint, null | no foreign key: entries outlive deleted users |
| `actor_guard` | varchar(32), null | |
| `credential` | varchar(96), null | `session`, `token:<id>`, `jwt:<family>` or `console:<command>`: never a secret |
| `ip`, `user_agent`, `request_id` | varchar(45), varchar(255), varchar(64), null | |
| `changes`, `meta` | json, null | encrypted fields are always redacted |

Indexed on (`subject_type`, `subject_key`, `at`), (`actor_user_id`, `at`) and
(`at`, `id`).

### `settings`

| Column | Type | Notes |
|---|---|---|
| `name` | varchar(32) | primary key: the group |
| `value` | json | the group's values, without its secrets |
| `secrets` | json, null | secret fields, encrypted under a key derived from `APP_KEY` |
| `version` | bigint | advanced by every save; a `PUT` sends the version it read, and a stale one is refused |
| `updated_at` | instant | |
| `updated_by` | bigint, null | the user who saved |

### `jobs`

| Column | Type | Notes |
|---|---|---|
| `id` | uuid | |
| `queue` | varchar(64) | queue name |
| `kind` | varchar(64) | `Job::KIND` |
| `version` | int | `Job::VERSION` of the payload |
| `payload` | json | the job, serialized |
| `attempts` | int | claims so far (each run counts when it starts) |
| `max_attempts` | int | most runs |
| `available_at` | instant | not run before this |
| `reserved_by` | varchar(64), null | owner token of the run in progress |
| `reserved_until` | instant, null | the run in progress owns the job until then (indexed) |
| `started_at` | instant, null | when the current or last attempt started |
| `description` | varchar(200), null | `Job::describe`, shown in the panel |
| `last_error` | varchar(500), null | why the last run failed |
| `initiated_by` | bigint, null | the user who asked for it, if any |
| `created_at` | instant | when it was dispatched (indexed with `id`) |
| `failed_at` | instant, null | set when it will not run again |

A job's row is deleted when it finishes; a failed job stays (with
`failed_at`) until it is retried or forgotten, or until the workers delete it
after `queue.failed_retention_days` (30 by default). Indexed on (`queue`,
`failed_at`, `available_at`) for claiming.

`job_receipts`: `key` varchar(191) primary key, `created_at` (indexed). A job
records a receipt in the transaction of its effect (`rustcms::queue::once`), so
the effect applies once even when the job runs again, as long as the receipt
is kept: receipts are deleted after `queue.failed_retention_days` (30 by
default).

### `job_runs`

| Column | Type | Notes |
|---|---|---|
| `id` | uuid | this row |
| `job_id` | uuid | the job (a retried job finishes more than once) |
| `queue`, `kind` | varchar(64) | |
| `description` | varchar(200), null | |
| `status` | varchar(16) | `done`, `failed` or `cancelled` |
| `attempts` | int | claims used |
| `created_at` | instant | when the job was dispatched |
| `started_at` | instant, null | when its last attempt started |
| `finished_at` | instant | |
| `duration_ms` | bigint, null | how long its last attempt ran |
| `last_error` | varchar(500), null | |
| `initiated_by` | bigint, null | |

Indexed on (`finished_at`, `id`), (`job_id`, `finished_at`) and (`kind`,
`finished_at`, `id`).

### `schedule_states`, `schedule_slots`

| Table | Columns |
|---|---|
| `schedule_states` | `name` varchar(64) primary key, `active_job_id` uuid null (the job of the last claimed slot, for `without_overlapping`), `last_slot_at` instant null, `updated_at`. Locked by every claim; never pruned |
| `schedule_slots` | primary key (`name`, `slot_at`): a slot is claimed once, by one instance. `claimed_at` (indexed), `instance` varchar(64) (who claimed it), `job_id` uuid null. Kept 7 days, pruned hourly while the scheduler runs |

### `schedule_runs`

| Column | Type | Notes |
|---|---|---|
| `id` | uuid | also the task's `run_id` |
| `name` | varchar(64) | the schedule |
| `slot_at` | instant, null | the slot it ran for; null for a manual run |
| `trigger` | varchar(16) | `schedule` or `manual` |
| `started_at`, `finished_at` | instant, instant null | |
| `status` | varchar(16) | `running`, `dispatched`, `succeeded`, `failed`, `skipped`, `cancelled` or `unknown` |
| `job_id` | uuid, null | the job that runs it |
| `duration_ms` | bigint, null | |
| `summary` | varchar(200), null | a task's summary |
| `error` | varchar(500), null | |
| `initiated_by` | bigint, null | who ran it, for a manual run |

Indexed on (`name`, `started_at`, `id`), (`started_at`, `id`) and (`job_id`).

### `exports`, `export_objects`

| Column of `exports` | Type | Notes |
|---|---|---|
| `id` | uuid | |
| `user_id` | bigint | who asked for it (indexed) |
| `auth_version` | bigint | their credential version then: the export stops if it changes |
| `resource` | varchar(64) | resource slug |
| `query`, `columns` | json | the list query (canonical, no page) and the fields, fixed at creation |
| `keys` | json, null | keys of the records exported, in order: checked again at download |
| `status` | varchar(16) | `pending`, `running`, `done` or `failed` |
| `rows` | int, default 0 | rows written |
| `disk`, `storage_key` | varchar(16), varchar(128), null | the file |
| `error` | varchar(255), null | a message safe to show its owner |
| `created_at`, `expires_at` | instant | the row and the file are deleted after `expires_at` (indexed) |

`export_objects`: `storage_key` varchar(128) primary key, `export_id` uuid
(indexed), `disk`, `created_at`. Every object an export run may have written,
so pruning finds the files of a run that crashed.

### `imports`, `import_rows`

| Column of `imports` | Type | Notes |
|---|---|---|
| `id` | uuid | |
| `user_id` | bigint | who uploaded it (indexed) |
| `auth_version` | bigint | their credential version then: the import stops if it changes |
| `resource` | varchar(64) | resource slug |
| `disk`, `storage_key` | varchar(16), varchar(128) | the uploaded file |
| `sha256` | varchar(64) | of the file, hex |
| `header` | json | column names of the file |
| `rows` | int | rows of the file, header excluded |
| `mapping` | json, null | column → field, fixed when it runs |
| `status` | varchar(16) | `uploaded`, `running`, `done` or `failed` |
| `created`, `updated`, `failed` | int, default 0 | records created and updated, rows that failed |
| `report` | json, null | the failed rows: `[{row, field?, message, code, params?}]` (no `field`: the whole row), never the values |
| `error` | varchar(255), null | why it stopped, safe to show its owner |
| `created_at` | instant | when it was uploaded |
| `started_at` | instant, null | when it started running |
| `expires_at` | instant | the row and the file are deleted after it (indexed) |

`import_rows`: primary key (`import_id` → `imports` cascade, `row` int),
`outcome` varchar(16), `record_key` varchar(255). One receipt per row written,
committed with the row, so a resumed import never writes a row twice.

## Your tables

Your migrations add tables next to these, in the same database. Keep your
data in your own tables and reference `users.id` (bigint) when you need a
user: framework migrations may change their tables in later releases, and the
access-control and credential tables are written only through the framework
(`RbacService`, the auth routes). Secrets in your
tables use `rustcms::crypto::Encrypted<T>` in a `Text` column
([Encrypted columns](../digging-deeper/encryption.md)).
