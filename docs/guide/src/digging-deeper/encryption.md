# Encrypted columns

A secret in one of your tables (an API token, a service's environment
variables, registry credentials) is stored as an `Encrypted<T>`. The column
holds only ciphertext sealed with `APP_KEY`, and the value never leaves your
code unless you open it. Use it for any value that must stay secret in the
database, in backups and in logs.

## Quick start

1. Declare the column as `Option<Encrypted<T>>` on a `Text` column.
2. Seal the value before you save it, open it when you need it:

```rust
use rustcms::crypto::{Context, Encrypted};

let context = Context::column("integrations", "env");
// To store:
let sealed = Encrypted::seal(state.keyring(), &context, &env)?;
// To use:
let env: BTreeMap<String, String> = row.env.unwrap().open(state.keyring(), &context)?;
```

3. Register the column so `key:reencrypt` finds it after a key rotation
   ([below](#rotating-the-key)). Admin fields declared with
   `Field::encrypted()` or `Field::encrypted_map()` register theirs.

## The model and the table

The demo keeps an API token and a map of environment variables encrypted:

<<< @/../../../examples/demo/src/models/integration.rs#model [src/models/integration.rs]

The migration uses `Text` columns (a sealed value is text, at most about
60,000 bytes):

<<< @/../../../examples/demo/src/database/migrations/m20261101_000001_create_integrations.rs#up [src/database/migrations/m20261101_000001_create_integrations.rs]

`T` is any type that is `Serialize` and `DeserializeOwned`: the value is
stored as its JSON, sealed.

## Sealing and opening

Encryption is explicit. Loading and saving a model only move the ciphertext;
nothing is decrypted on its own. You call `seal` and `open` with the keyring
and a `Context`:

- **The keyring.** `state.keyring()` on `AppState`; `ctx.keyring()?` in
  admin hooks and actions.
- **The context.** `Context::column(table, column)` binds a value to its
  table and column: copied into another column or table, it does not open.
  With a schema, name the table `"schema.table"`.
- **Binding to a row.** Add `.row(id)` when the row's key is known before
  the insert (a UUID, a text key). The value then opens only in that row. The
  key is written in one canonical form: integers in decimal, UUIDs lowercase
  with hyphens, text as is. Admin fields bind table and column only.

`open` returns `Result<T, Unreadable>`. `Unreadable` means a key that is
gone, another context, or an altered or malformed value; which one is not
said. Converted to an `AppError` (with `?`), it is a 500 whose logged message
points at `APP_KEY` and `app.previous_keys`.

### In the admin panel

Declare the field with `Field::encrypted()` (an `Encrypted<String>`) or
`Field::encrypted_map()` (an `Encrypted<BTreeMap<String, String>>`):

<<< @/../../../examples/demo/src/admin/resources/integration_resource.rs#fields [src/admin/resources/integration_resource.rs]

![The Integrations form: the API token is write-only and the environment variables are listed by name](/img/digging-deeper/encrypted-fields.webp)
*The demo application: an integration's form with an encrypted token and environment map.*

The panel never shows the value: a token is replaced or cleared, a map is
listed by name and each entry kept, replaced or removed. See
[Encrypted fields](../admin/forms.md#encrypted-fields).

## What never leaks

- `Debug` shows only a key id: `Encrypted(3f2a…)`.
- Serializing writes `null`, so a model sent as JSON carries nothing.
- An `Encrypted` value is never built from JSON (deserializing always fails),
  so `ActiveModel::from_json` cannot be used on such a model.
- Errors never quote the value or a key.

Do not put an opened value in a record sent to clients, in a log line or in
an [audit entry](audit-log.md).

## Rotating the key

`rustcms key:reencrypt` moves every value to the current `APP_KEY`. It finds
the columns it must move in a list built at boot:

- **Admin fields** (`Field::encrypted*`) register theirs.
- **Other columns**: register them yourself, in a provider's `register`:

  ```rust
  use rustcms::crypto::Binding;

  app.encrypted_column::<integration::Entity>(integration::Column::ApiToken, Binding::Column);
  ```

  Use `Binding::Row` if you seal with `.row(id)`. Registering the same
  column twice with the same binding is fine; with another binding, boot
  panics.

It also moves the [secrets of settings groups](../admin/settings.md#secrets).

The rotation itself is an operations task: see
[Rotating APP_KEY](../operations/app-key.md). In short, the old key goes into
`APP_PREVIOUS_KEYS`, where it only opens encrypted values (never sessions,
tokens or links), then `key:reencrypt` re-seals everything. The command
reads its own configuration: run it with the same `APP_KEY` and
`APP_PREVIOUS_KEYS` as the application. How it works:

- every value is authenticated first, so a forged one is reported, never
  trusted or rewritten;
- each row is written only if it still holds the value read (a row changed
  meanwhile is left for the next run);
- it reads `--batch` rows at a time, in primary-key order, and can be run
  again at any time: values already under the current key are skipped.

| Option | Default | Effect |
|---|---|---|
| `--dry-run` | off | check and count, write nothing |
| `--list` | off | print the registered columns (with their binding) and `settings secrets`, then exit |
| `--batch <n>` | `500` | rows read at a time, 1 to 10000 |

It prints one line per column and one for the settings:

```text
integrations.api_token: 0 current, 12 moved, 0 changed meanwhile, 0 unreadable
settings: 3 current, 1 moved, 0 changed meanwhile, 0 unreadable
```

With `--dry-run` the column reads `to move` instead of `moved`. Unreadable
values are listed one per line (`unreadable: <table.column> <key>`, or the
settings group), and never modified.

**Exit status.** A run fails (non-zero) when a row changed meanwhile or a
value is unreadable. Rows it moved count as done. With `--dry-run`, any row
still to move also fails it, so `key:reencrypt --dry-run` succeeding means
everything is under the `APP_KEY` that command was run with.

## Reference

`rustcms::crypto::Encrypted<T>`:

| Method | Use |
|---|---|
| `Encrypted::seal(keyring, &context, &value)` | seals with the current key; fails over 32 KiB of JSON |
| `.open(keyring, &context)` | the value, or `Unreadable` |
| `.is_current(keyring, &context)` | whether the current key opens it (authenticated) |
| `.resealed(keyring, &context)` | the same value sealed with the current key (the value is never parsed) |
| `.envelope()` | the stored text |
| `.key_id()` | the key id the envelope claims (unauthenticated: a hint) |
| `Encrypted::from_envelope(text)` | wraps stored text, unchecked (`open` checks it) |

`rustcms::crypto::looks_sealed(text)` tells whether a text starts like an
envelope (`rcms1.`), for screens that must mask such values.

The cipher is AES-256-GCM, with a key derived from `APP_KEY`. The stored
text is `rcms1.<key id>.<nonce, ciphertext and tag in base64url>`; the key
id identifies a key without revealing it, so an operator can see which key
sealed a value.

## Pitfalls and limits

- **Size.** At most 32 KiB per value (its JSON). The sealed text then fits a
  MySQL `TEXT` column; use `Text` columns everywhere.
- **No queries.** Encrypted columns cannot be searched, sorted or filtered
  (each seal uses a fresh nonce, so equal values differ).
- **Primary keys.** `app.encrypted_column` panics at boot on a composite
  primary key, or one that is not an integer, text or UUID.
- **The keyring.** Boot fails when `app.previous_keys` lists a key twice, or
  lists `APP_KEY` itself.
- **Lost keys.** A value sealed with a key that is neither `APP_KEY` nor in
  `APP_PREVIOUS_KEYS` cannot be recovered. Keep the keys your
  [backups](../operations/backup-restore.md) need.

## Related

- [Rotating APP_KEY](../operations/app-key.md)
- [Forms: encrypted fields](../admin/forms.md#encrypted-fields)
- [Settings: secrets](../admin/settings.md#secrets)
- [Configuration](../getting-started/configuration.md)
