# ADR 0030: Encrypted columns and the keyring

- Status: accepted (0.4.1, W8)
- Date: 2026-10-18

## Context

Apps keep secrets in their own tables: a service's environment variables,
registry credentials, API tokens. Settings already encrypt their secrets
(ADR 0015), but models had nothing. A secret in an app table must:
- never reach the browser, the logs or the audit log;
- survive a rotation of `APP_KEY`;
- be editable in the panel without ever being shown.

## Decision

- **Keyring:**
  - `APP_KEY` seals and opens. `app.previous_keys` (`APP_PREVIOUS_KEYS`,
    comma-separated, or a TOML array) only open.
  - Boot refuses a bad key, a duplicate, `APP_KEY` itself, and colliding key
    ids, without quoting a key.
  - Sessions, JWTs, refresh tokens and links never use previous keys: they
    end at a rotation, as before.
  - `state.keyring()`, and `Ctx::keyring()` in the admin.
- **`Encrypted<T>`** is a SeaORM value type holding only the envelope text:
  - `seal(&keyring, &context, &value)`, `open(&keyring, &context)`,
    `is_current`, `resealed`;
  - never transparent: loading and saving a model only move the envelope.
  - **Redaction:** `Debug` shows the key id, `Serialize` writes `null`, and
    `Deserialize` always fails, so no `ActiveModel::from_json`.
  - **Damaged envelopes:** one still loads; only `open` fails.
- **Format:** `rcms1.<kid>.<payload>`.
  - `kid`: 16 hex characters, the first 8 bytes of
    `HMAC(APP_KEY, "rustcms/key-id/v1")`.
  - `payload`: base64url, no padding, of `nonce (12) ‖ ciphertext ‖ tag (16)`.
  - The encryption key is `HMAC(APP_KEY, "rustcms/encrypted/v1")`, separate
    from settings, sessions and JWTs.
  - **Strict parsing:** exact prefix, canonical base64, at most 60 000 bytes
    read before decoding.
  - The plaintext (the JSON of `T`) is at most 32 KiB, so a sealed value fits
    a MySQL `TEXT` column.
- **AAD:** length-prefixed parts:
  1. the domain;
  2. the header (`rcms1.<kid>`);
  3. the table;
  4. the column;
  5. a row tag (none, or the canonical primary key).

  No two contexts share bytes. A ciphertext moved to another table, column or
  (when bound) row does not open.
- **One failure:** `Unreadable` does not distinguish a wrong key, another
  context or a forgery. Errors are static and never quote values.
- **Admin fields bind no row:** `Field::encrypted` and
  `Field::encrypted_map` bind table and column only, because autoincrement
  keys are unknown before the insert. Someone who can write the database can
  therefore swap ciphertexts between rows of one column. App code that knows
  its key first (UUIDs) uses `Context::row`.
- **Rotation (`key:reencrypt`):**
  - It walks each registered column (`AppBuilder::encrypted_column`, plus
    the admin's fields) by keyset pagination, and **authenticates every
    envelope**, so a forged current-key envelope counts as unreadable.
  - It writes with a bytewise compare-and-swap (`BINARY` on MySQL): a row
    changed meanwhile is counted, never overwritten.
  - Settings secrets reseal under the group's lock, at the same version. A
    secret already under `APP_KEY` stays byte for byte, so a second run
    writes nothing.
  - It exits 1 until nothing is unreadable, changed or left; `--dry-run`
    writes nothing and still authenticates.
  - Only single-column primary keys are supported.
- **Admin read contract:** `record()` masks encrypted fields, and any other
  column holding an envelope, as `null`. Policies, hooks, titles, exports,
  dashboards and custom pages never see plaintext or ciphertext. The wire
  gets:
  - for a string, `{"state": "set" | "unset" | "unreadable"}`;
  - for a map, the same `state` plus a `revision`, and its key names when the
    user may read the field.

  `revision` is an HMAC of the envelope under the current key (8 bytes);
  `"none"` when unset.
- **Admin write contract:**
  - A string takes `{"replace": "…"}` or `{"clear": true}` (only when
    nullable and not required); a field left out is unchanged.
  - A map takes `{"revision", "set": {…}, "keep": […]}` or
    `{"revision", "clear": true}`:
    - keys not kept nor set are removed;
    - on an existing record every change needs the current revision, or it
      gets 409 (no key added in between is ever lost);
    - `keep` decrypts the stored map (422 when it cannot); replacing
      everything and clearing never decrypt, which is how an unreadable map
      recovers.
  - Operations resolve in the write transaction, on the locked record, after
    the hooks, in `apply()`. The bounds hold on the merged map.
- **Leaks closed:**
  - The audit diff compares envelope digests privately and records
    `[redacted]`.
  - Idempotency request hashes key secret operations with the keyring, so no
    unsalted hash of a secret is stored.
  - Boot refuses encrypted fields that are sortable, searchable, filterable,
    a title, a key, a relation, media, rich text, in a constraint, in a table
    column, or given a column default.
  - Generated tables, imports and exports leave them out.

## Consequences

- Secrets in app tables have one supported path. RDockPanel (service
  environments, registry credentials) uses it.
- What stays visible:
  - map key names (to readers);
  - ciphertext lengths;
  - key ids, which are linkable across rows sealed with the same key.
- AES-GCM with random nonces stays far below its limit of about 2³² seals
  per key; a rotation resets it.
- Trusted escape hatches remain: raw SQL, and app code that calls `open`.
- A rotation is stop-all, start-all: a process still on the old key cannot
  read what the new ones seal.
