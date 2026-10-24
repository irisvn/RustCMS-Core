# Database schema

The framework's migrations (`rustcms::migration`, run by `rustcms migrate`
before your own) create the tables below. Your models live next to them in the
same database. Column types are shown portably:

- **instant**: `TIMESTAMPTZ` on PostgreSQL, `DATETIME(6)` in UTC on MySQL, text
  on SQLite;
- **bigint id**: auto-increment `BIGINT`.

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
| `sessions` | admin sign-ins (and anonymous CSRF sessions) | `id` = HMAC-SHA-256 of the cookie's random value, keyed by `APP_KEY` | `data` (JSON), `expires_at` (indexed, pruned hourly), `lineage` (the first session of a re-issued chain: sign-out ends all of it) |
| `personal_access_tokens` | API tokens (`rcms_…`) | `token_hash` = SHA-256 | `user_id` → `users` (cascade), `name`, `abilities` (JSON list, `["*"]` = all of the user's permissions), `last_used_at`, `expires_at`, `revoked_at` |
| `jwt_families` | one JWT sign-in and its refresh chain | none | `id`, `user_id` → `users` (cascade), `abilities`, `auth_version` at issue, `expires_at`, `revoked_at` |
| `jwt_refresh_tokens` | each refresh token of a family | `token_hash` = HMAC-SHA-256 under a key derived from `APP_KEY` | `family_id` → `jwt_families` (cascade), `used_at`: reusing a used token revokes the family |
| `user_tokens` | password-reset and invitation links | `token_hash` under a key derived from `APP_KEY` | unique (`user_id`, `purpose`): one live link per user and purpose; `email` and `auth_version` at request, `expires_at`, `sent_at` |

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
| `seaql_migrations` | applied migrations | [Database & migrations](../basics/database.md) |
| `rustcms_locks` | one row per lock name, locked first by a transaction to run after the others holding it; used on MySQL only (PostgreSQL takes an advisory lock, SQLite already holds its write lock), created on every database | [Database & migrations](../basics/database.md) |
| `idempotency_keys` | stored results of writes sent with `Idempotency-Key` (`owner`, `request_hash`, `result`, `expires_at`, kept 24 hours) | [Actions](../admin/actions.md), [Retention](../operations/retention.md) |
| `media`, `media_uploads`, `media_deletions` | the media library (`disk`, `key`, `name`, `mime`, `size`, `sha256`, `width`, `height`, `alt`, `owner_id`, `deleted_at`); uploads not yet attached; files to delete from storage, with retries | [Media](../admin/media.md), [Retention](../operations/retention.md) |
| `audit_logs` | who changed what (`event`, `subject_type`/`subject_key`, actor, `credential`, `ip`, `request_id`, `changes`) | [Audit log](../digging-deeper/audit-log.md) |
| `settings` | settings groups (`name`, `value`, `secrets` encrypted under a key derived from `APP_KEY`, `version`, `updated_by`) | [Settings](../admin/settings.md#secrets) |
| `schedule_states`, `schedule_slots` | one state row per schedule (never pruned); each claimed slot, kept 7 days and pruned hourly while the scheduler runs | [Scheduling](../digging-deeper/scheduling.md) |
| `jobs`, `job_receipts` | the database queue (`queue`, `kind`, `payload`, `attempts`, `available_at`, `reserved_until`, `started_at`, `description`, `last_error`); jobs already run once | [Queues](../digging-deeper/queues.md) |
| `job_runs` | one row per finished job (`job_id`, `status` done/failed/cancelled, times, `duration_ms`, safe error), kept `queue.history_retention_days` | [Queues](../digging-deeper/queues.md#the-queue-page) |
| `schedule_runs` | one row per schedule run (`trigger`, `status`, `job_id`, times, `summary`, safe error), kept `schedule.history_retention_days` | [Scheduling](../digging-deeper/scheduling.md#the-schedules-page) |
| `exports`, `export_objects` | CSV exports and their files | [Import and export](../admin/import-export.md) |
| `imports`, `import_rows` | CSV imports, their mapping and report; what each row did | [Import and export](../admin/import-export.md) |
