# Settings

Settings are values your application reads at run time and administrators
change in the panel: a site name, whether enrolments are open, an SMTP
password. They live in the database (the `settings` table, created by
`rustcms migrate`), one row per group, and every instance sees a change as
soon as it is saved.

## Declaring a group

A group has two halves, in two files (`rustcms make:settings GeneralSettings`
writes both and registers the page):

- the **data**, `src/settings/general_settings.rs`: a struct whose fields are
  the settings (`impl Settings`); `Default` gives their values until someone
  saves the page. Services, jobs and handlers read it.
- the **page**, `src/admin/pages/general_settings_page.rs`: how the panel
  shows and checks it (`impl SettingsPage` for the same struct).

```rust,ignore
// src/settings/general_settings.rs
use rustcms::settings::{Secret, Settings};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralSettings {
    pub site_name: String,
    pub tagline: Option<String>,
    pub enrolments_open: bool,
    pub max_class_size: i32,
    pub smtp_password: Secret,
}

impl Default for GeneralSettings {
    fn default() -> Self {
        Self {
            site_name: "Academy".into(),
            tagline: None,
            enrolments_open: true,
            max_class_size: 30,
            smtp_password: Secret::default(),
        }
    }
}

impl Settings for GeneralSettings {
    const GROUP: &'static str = "general";
}
```

```rust,ignore
// src/admin/pages/general_settings_page.rs
use rustcms::admin::{SettingField, SettingsMeta, SettingsPage};
use rustcms::async_trait;

use crate::settings::general_settings::GeneralSettings;

#[async_trait]
impl SettingsPage for GeneralSettings {
    fn meta() -> SettingsMeta {
        SettingsMeta::new("General").icon("settings")
    }

    fn fields() -> Vec<SettingField> {
        vec![
            SettingField::text("site_name").required().max_length(100),
            SettingField::text("tagline").nullable(),
            SettingField::boolean("enrolments_open"),
            SettingField::integer("max_class_size").help("Learners per class"),
            SettingField::secret("smtp_password").label("SMTP password"),
        ]
    }
}
```

Register it with the panel (`src/providers/admin_panel_provider.rs`):

```rust,ignore
AdminProvider::new().settings::<GeneralSettings>()
```

Settings are for values administrators change while the application runs;
values that change with a deploy (URLs, credentials of the database) belong
in `config/` and `.env`.

The page appears under **Settings** in the navigation, at
`/admin/settings/general`. The group name is 1–32 of `a-z`, `0-9`, `_`.

## Kinds

| `SettingField::` | Struct type | Input |
|---|---|---|
| `text(name)` | `String` (`Option<String>` with `.nullable()`) | single line |
| `textarea(name)` | `String` | several lines |
| `integer(name)` | `i32` | number |
| `boolean(name)` | `bool` | switch |
| `select(name, [(value, label)])` | `String` | one of the options |
| `key_value(name)` | `BTreeMap<String, String>` | key/value rows (`.max_items(n)`, `.max_length(n)` per value) |
| `date(name)` | `chrono::NaiveDate` (`Option` with `.nullable()`) | date picker |
| `secret(name)` | `Secret` | [see below](#secrets) |

Rules: `.required()`, `.nullable()`, `.max_length(n)`; presentation:
`.label(…)`, `.help(…)`, `.placeholder(…)`, `.email()`, `.url()`;
`.audit_redact()` keeps the values out of the [audit log](../digging-deeper/audit-log.md).
Values are checked like resource input (same messages and
[codes](../reference/error-codes.md)).

Boot fails, with a message naming the field, when the declaration and the
struct disagree. Each field is checked by passing a value of its kind
through the struct: a struct field without a `SettingField` (or the
reverse), a type that does not hold the kind (an `i64` for `integer`, a
`Secret` declared as `text`), `.nullable()` without an `Option` (or an
`Option` without `.nullable()`), a default that breaks its rules, a secret
whose default is set, a select without options, a form input that is not a
field or offers other values than its field, or a group registered twice.

For larger numbers or decimals, store them as text and parse them where you
read them.

### Layout and checks

`form()` arranges the inputs with the usual [form layouts](forms.md)
(sections, grids, tabs); each input names a field, and may change its label,
help or option labels (not its values). `validate` checks the whole group
before it is saved, inside the write (its `ctx` has the transaction and the
user's current permissions). Secrets quoted in its messages are replaced by
`[redacted]`:

```rust,ignore
fn form() -> Form {
    Form::new().schema(components![
        Section::make("Site").schema(components![TextInput::make("site_name"), Textarea::make("tagline")]),
        Toggle::make("enrolments_open"),
        NumberInput::make("max_class_size"),
        SecretInput::make("smtp_password"),
    ])
}

async fn validate(_ctx: &Ctx, settings: &Self) -> Result<(), ValidationErrors> {
    if settings.max_class_size < 1 {
        return Err(ValidationErrors::field("max_class_size", "is out of range"));
    }
    Ok(())
}
```

## Reading settings

Anywhere you have the application state:

```rust,ignore
let general: GeneralSettings = rustcms::settings::get(&state).await?;
if general.enrolments_open { /* … */ }
let password = general.smtp_password.expose(); // Option<&str>
```

`get` reads the row (one query) and is correct on every instance at once,
with no cache to clear. Inside a transaction, read with
`rustcms::settings::load::<GeneralSettings, _>(&txn, &state.config().app.key)` to see the
transaction's own writes. A group that was never saved reads as its
`Default`; a field added to the struct later reads as its default until saved.

## Secrets

A `Secret` field is stored encrypted (AES-256-GCM, with a key derived from
`APP_KEY`, bound to its group, field and version). It is never sent to the
browser: the page shows only whether it is set, with **Set a value** /
**Replace**, **Clear** and **Undo**. Saving other settings keeps it.

`Secret` serializes as `null` and prints as `Secret(set)`, so a settings
struct can be logged, queued or serialized without leaking it; read the value
with `.expose()`. A secret's default is always unset (`Secret::default()`).

If `APP_KEY` changes, stored secrets can no longer be decrypted: `get` fails
with an error naming the secret, the page marks it **Cannot be read**, and
the group cannot be saved until each such secret is replaced or cleared. Keep
`APP_KEY` stable, and plan to enter secrets again if you rotate it.

## Permissions and history

Each group declares `settings.<group>.view` and `settings.<group>.update`
(run `permission:sync` after adding a group). Saving needs both. Permissions
are checked again inside the write, so access revoked meanwhile is honoured.

Saves are optimistic: the page sends the version it was loaded at, and a save
made by someone else meanwhile is refused (code `stale`); the page offers to
reload. A save that changes nothing creates no version and no entry. Each
change records `settings.updated` in the audit log with the changed values
(secrets and `.audit_redact()` settings as `"[redacted]"`).

## API

| Method | Path | Body | Response |
|---|---|---|---|
| GET | `/admin/api/settings/{group}` | | `SettingsResponse` `{form, data, secrets, version, abilities}` |
| PUT | `/admin/api/settings/{group}` | `{version, data}` | `SettingsResponse` |

`data` holds the settings to change. For a secret: a string replaces it,
`null` clears it, leaving it out keeps it. Without access the answer is 403
before the body is read; an unknown group is 404 for users who may view some
settings.
