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
| `media(name)` | `Option<uuid::Uuid>` (always nullable) | [media input](media.md#media-fields): pick or upload one file |
| `media_list(name, max)` | `Vec<uuid::Uuid>` | up to `max` files, in order |

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

### Media settings

```rust,ignore
SettingField::media("logo").accept(&["image/*"]).max_size(500_000),   // logo: Option<Uuid>
SettingField::media_list("slides", 5),                                 // slides: Vec<Uuid>
```

They hold ids of files of the [media library](media.md) (it must be enabled
with `AdminProvider::media()`; boot fails otherwise), edited with the same
media input as resource fields (`FileUpload`): upload a file, remove or
reorder them; the page is not saved while an upload is on its way. Each file
a save **changes** to is checked like a resource's media field, inside the
save and as the user is then: it exists, is not trashed, is visible to the
user (`media.view_any` and `media.view`; their own uploads, or every file
with `media.view_all` or `media.manage_all`), and matches `.accept(…)` and `.max_size(…)`; a list
holds at most `max` distinct ids. The referenced rows are locked (shared)
until the save commits. A reference kept as it was is not checked again
(a file trashed later stays referenced until the setting changes; see
[when a referenced file is trashed or deleted](media.md#when-a-referenced-file-is-trashed-or-deleted)).
The input is the resource fields' media input (`FileUpload`): upload, remove,
reorder. Their default must
be empty (`None`, `vec![]`).

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
`rustcms::settings::load::<GeneralSettings, _>(&txn, state.keyring())` to see the
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

When you rotate `APP_KEY`, keep the old key in `APP_PREVIOUS_KEYS` and run
`rustcms key:reencrypt` ([Rotating APP_KEY](../operations/app-key.md)):
saved secrets stay readable. A secret that no current or previous key opens
cannot be decrypted: `get` fails with an error naming the secret, the page
marks it **Cannot be read**, and the group cannot be saved until each such
secret is replaced or cleared.

### Secrets bound to other settings

`SettingField::secret("password").kept_while(&["host", "port", "username"])`
keeps the saved secret only while those settings keep their values: changing
one without giving the secret again clears it, so a password is never used
with a server it was not given for.

`validate_in(state, ctx, settings)` is `validate` with the application (its
configuration and services), for checks that depend on them.

### Reacting to a save

`after_save` runs in the save's transaction once the new values are written
(and the audit entry recorded), before the commit, with the group as it was
and as saved; an error rolls the save back. Use it to react to a change in
the same transaction, e.g. dispatch a job that rebuilds what depends on the
setting:

```rust,ignore
async fn after_save(
    _state: &AppState,
    _ctx: &Ctx,
    txn: &DatabaseTransaction,
    before: &Self,
    after: &Self,
) -> AppResult<()> {
    if before.site_name != after.site_name {
        rustcms::queue::dispatch(txn, &RebuildSitemap).await?; // committed with the save
    }
    Ok(())
}
```

`before` holds the defaults when the group was never saved; a secret that
could not be decrypted (which the save replaced or cleared) reads as unset
there, and a stored value that no longer fits the struct (its declaration
changed) reads as its default, so the save that replaces it still succeeds. A save that changes nothing writes nothing and does not call it.
`before` and `after` hold secrets in plain text: never log them or put them in
a job's payload. Secrets of either version are replaced by `[redacted]` in the
messages of the errors it returns (validation, bad request and internal).
Secrets quoted in a `ValidationErrors` it returns, as they were or as
saved, are redacted like `validate`'s.

The hook sees secrets as values: `before.smtp_password.expose()` and
`after.smtp_password.expose()` are the plaintext (an unset or unreadable one
is `None`). Compare them, or hand them to the service that needs them, but
never write them to logs, the audit log, a job's description, an error
message or anything sent to the browser (ADR 0030's rule for secrets): a
job that needs the secret reads the settings itself when it runs.

## Mail

`AdminProvider::mail_settings()` adds the **Mail** page: the SMTP server of
[`MailSettings`](../digging-deeper/mail.md) (host, port, encryption, user
name, password, sender, reply-to), checked against `[mail]`'s policy when
saved, the password bound to host, port and user name. Its **Send test mail**
card (`POST /admin/api/settings/mail/test` `{to}`) sends a mail now with the
saved settings and answers `{ok}` or `{ok: false, code}` (`smtp_auth`,
`mail_policy`, …); it needs `settings.mail.update` (checked before the body
is read), is limited to five a minute per user, and is recorded in the audit
log as `mail.test_sent` with the recipient masked (`a***@example.com`).

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
