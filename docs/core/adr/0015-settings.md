# 0015 — Settings

- Status: accepted
- Date: 2026-10-18

## Context

Back-offices need values that administrators change without a deploy (site
name, enrolment rules, SMTP credentials), visible to every instance at once,
validated like any input, audited, and some of them secret.

## Decision

- **A group is a Rust struct** (`Settings` in core: `GROUP`, serde,
  `Default`), read with `settings::get` / `load`; the panel edits it through
  `SettingsPage` (admin: `fields`, optional `form` and `validate`). Core does
  not depend on the panel, so mail and jobs read settings too.
- **One row per group** (`settings`: `name`, `value` JSON, `secrets` JSON,
  `version`, `updated_at`, `updated_by`). Reads are one query by primary key,
  with no cache: every instance is current without Redis or invalidation, and
  there is no cache entry that could be labelled with the wrong version.
- **Kinds limited to those whose wire form is their serde form** (text,
  integer `i32`, boolean, select, key/value, date, secret), so there is no
  codec between stored JSON and the struct. Boot checks the declaration
  against the struct by round trips: a value of the field's kind must come
  back unchanged (a `Secret` hides it, so it must be declared `secret`; an
  `i64` keeps 2^31 so `integer` refuses it), `null` must pass exactly for
  `.nullable()` fields, secret defaults must be unset (checked with an
  internal serialization that exposes secrets), form inputs name fields and
  offer the field's option values.
- **Secrets**: AES-256-GCM with `HMAC(APP_KEY, "rustcms/settings/v1")`, a
  random 96-bit nonce per write, additional data `group/field/version`, so a
  ciphertext moved to another field, group or version does not decrypt.
  Every save re-encrypts all secrets for the new version. `Secret` serializes
  as `null` and debug-prints `Secret(set)`; serde errors are never passed on
  (they may quote values), and the application's `validate` messages have
  any secret replaced by `[redacted]`. A non-object `secrets` column fails
  closed. A secret that no current or previous key decrypts (`APP_KEY`
  changed without `APP_PREVIOUS_KEYS`, ADR 0030) is reported, never dropped: the group cannot be saved until each
  is replaced or cleared. Writes are three-state: absent keeps, string
  replaces, `null` clears; an empty string is refused.
- **Writes** are authorized from the request first (403 before the body is
  read: the handler takes the raw request and reads it after admission), then under the RBAC lock with the actor's grants reloaded
  (`rbac::begin_as`): a revocation committed while waiting is honoured, and
  settings writes serialize with role changes. Optimistic version check (a
  missing row is version 0; simultaneous first writes: one wins, the other
  is stale), validation, the application's `validate` (inside the
  transaction, with the reloaded principal), then the row and a
  `settings.updated` audit entry in one transaction. A save that changes
  nothing writes nothing. `update` requires `view` too.
- Unknown groups answer 404 only to users who may view some settings group
  (403 otherwise), so group names do not leak.

## Consequences

- Rotating `APP_KEY` without keeping the old key in `APP_PREVIOUS_KEYS`
  makes secrets unreadable until re-entered; with it, `key:reencrypt` moves
  them to the new key (amended in 0.4.1, ADR 0030).
- Settings writes wait for the RBAC lock (rare writes; acceptable).
- Wider numeric kinds need a codec; deferred.
