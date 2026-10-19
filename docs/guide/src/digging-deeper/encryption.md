# Encrypted columns

A secret in one of your tables (an API token, a service's environment
variables, registry credentials) is stored as an `Encrypted<T>`. The column
holds only ciphertext sealed with `APP_KEY`, and the value never leaves your
code unless you open it.

```rust,ignore
// src/models/integration.rs
use rustcms::crypto::Encrypted;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "integrations")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub name: String,
    #[sea_orm(column_type = "Text", nullable)]
    pub api_token: Option<Encrypted<String>>,
    #[sea_orm(column_type = "Text", nullable)]
    pub env: Option<Encrypted<BTreeMap<String, String>>>,
}
```

```rust,ignore
use rustcms::crypto::{Context, Encrypted};

let context = Context::column("integrations", "env");
let sealed = Encrypted::seal(state.keyring(), &context, &env)?;        // to store
let env: BTreeMap<String, String> = row.env.unwrap().open(state.keyring(), &context)?; // to use
```

- **Explicit.** Loading and saving a model only move the ciphertext. You
  call `seal` and `open` with the keyring (`state.keyring()`, or
  `ctx.keyring()?` in admin hooks and actions) and a `Context`.
- **The context.** `Context::column(table, column)` binds a value to its
  table and column, so moved elsewhere it does not open. Add `.row(id)` when
  the row's key is known before the insert (a UUID): the value is then bound
  to that row too. Admin fields bind table and column only.
- **Never leaked.**
  - `Debug` shows only a key id.
  - Serializing writes `null`, so a model sent as JSON carries nothing.
  - An `Encrypted` value is never built from JSON, so
    `ActiveModel::from_json` cannot be used on such a model.
  - Errors never quote the value.
- **Limits.**
  - At most 32 KiB per value (its JSON). A sealed value then fits a MySQL
    `TEXT` column; use `Text` columns everywhere.
  - Encrypted columns cannot be searched, sorted or filtered.
- **When it cannot be opened.** `open` fails with `Unreadable`: a key that is
  gone, another context, or an altered value. Which one is not said.
- **In the admin panel**, declare the field with
  [`Field::encrypted()` or `Field::encrypted_map()`](../admin/forms.md#encrypted-fields).

## Rotating the key

Columns are found by `key:reencrypt`.
- **Admin fields** register theirs.
- **Others:** register them yourself, in a provider's `register`:

  ```rust,ignore
  app.encrypted_column::<integration::Entity>(integration::Column::ApiToken, Binding::Column);
  ```

  Use `Binding::Row` if you seal with `.row(id)`.

To rotate `APP_KEY`, follow [Rotating APP_KEY](../operations/app-key.md).
The old key goes into `APP_PREVIOUS_KEYS`; it only opens values, never
sessions or tokens. Then `rustcms key:reencrypt` moves everything to the new
key:
- every value is checked, so a forged one is reported, never trusted;
- a row changed meanwhile is left for the next run;
- it exits non-zero until it reports 0 to move, 0 changed and 0 unreadable.

`--dry-run` counts without writing, and `--list` shows the columns it knows.
