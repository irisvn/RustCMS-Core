# Settings

Settings are values your application reads at run time and administrators
change in the panel: a site name, whether enrolments are open, an SMTP
password. They live in the database (the `settings` table, created by
`rustcms migrate`), one row per group, and every instance sees a change as
soon as it is saved. Values that change with a deploy (URLs, database
credentials) belong in `config/` and `.env` instead.

![The Site settings page with a secret and a media logo](/img/admin/settings-site.webp)
*A RustCMS demo application: Settings → Site.*

## Quick start {#declaring-a-group}

```sh
rustcms make:settings GeneralSettings
rustcms permission:sync
```

`make:settings` writes both halves of a group and registers the page:

- the **data**, `src/settings/general_settings.rs`: a struct whose fields are
  the settings (`impl Settings`); `Default` gives their values until someone
  saves the page. Services, jobs and handlers read it.
- the **page**, `src/admin/pages/general_settings_page.rs`: how the panel
  shows and checks it (`impl SettingsPage` for the same struct).

The name must be PascalCase and end in `Settings`; the group is the
snake_case stem (`GeneralSettings` → `general`), 1–32 of `a-z`, `0-9`, `_`.
The command writes both files or neither, and prints the permissions to
sync. The page appears under **Settings** at `{admin}/settings/general`.

## Full example

The demo's `site` group, the data:

<<< @/../../../examples/demo/src/settings/site_settings.rs#site-settings [src/settings/site_settings.rs]

the page:

<<< @/../../../examples/demo/src/admin/pages/site_settings_page.rs#settings-page [src/admin/pages/site_settings_page.rs]

and the registration, with the built-in mail page:

<<< @/../../../examples/demo/src/providers/admin_panel_provider.rs#settings [src/providers/admin_panel_provider.rs]

Reading it, in the demo's runtime branding:

<<< @/../../../examples/demo/src/admin/branding.rs#site-branding [src/admin/branding.rs]

## Kinds

| `SettingField::` | Struct type | Input |
|---|---|---|
| `text(name)` | `String` (`Option<String>` with `.nullable()`) | single line |
| `textarea(name)` | `String` | several lines |
| `integer(name)` | `i32` | number |
| `boolean(name)` | `bool` | switch |
| `select(name, [(value, label)])` | `String` | one of the options |
| `key_value(name)` | `BTreeMap<String, String>` | key/value rows; by default at most 50 pairs of 1,000 characters (`.max_items(n)`, `.max_length(n)` per value) |
| `date(name)` | `chrono::NaiveDate` (`Option` with `.nullable()`) | date picker |
| `secret(name)` | `Secret` | [see below](#secrets) |
| `media(name)` | `Option<uuid::Uuid>` (always nullable) | [media input](media.md#media-fields): pick or upload one file |
| `media_list(name, max)` | `Vec<uuid::Uuid>` | up to `max` files, in order |

| Modifier | |
|---|---|
| `.required()`, `.nullable()`, `.max_length(n)`, `.max_items(n)` | rules |
| `.label(…)`, `.help(…)`, `.placeholder(…)` | presentation |
| `.email()`, `.url()` | the value's format |
| `.accept(&[…])`, `.max_size(bytes)` | media settings |
| `.kept_while(&[fields])` | secrets ([below](#secrets-bound-to-other-settings)) |
| `.audit_redact()` | keeps the values out of the [audit log](../digging-deeper/audit-log.md) |

Values are checked like resource input (same messages and
[codes](../reference/error-codes.md)). For larger numbers or decimals, store
them as text and parse them where you read them.

### Boot checks

Boot fails, with a message naming the field, when the declaration and the
struct disagree. Each field is checked by passing a value of its kind
through the struct:

- a struct field without a `SettingField`, or the reverse; a field declared
  twice;
- a type that does not hold the kind (an `i64` for `integer`, a `Secret`
  declared as `text`);
- `.nullable()` without an `Option`, or an `Option` without `.nullable()`;
- a default that breaks its rules, a secret whose default is set, a select
  without options;
- a form input that is not a field, is used twice, or offers other values
  than its field;
- a media setting without `AdminProvider::media()`;
- a group registered twice.

### Media settings

```rust
SettingField::media("logo").accept(&["image/*"]).max_size(500_000),   // logo: Option<Uuid>
SettingField::media_list("slides", 5),                                 // slides: Vec<Uuid>
```

They hold ids of files of the [media library](media.md) (enable it with
`AdminProvider::media()`; boot fails otherwise), edited with the same media
input as resource fields (`FileUpload`): upload a file or pick one from the
library, remove or reorder them. The page is not saved while an upload is on
its way.

- Each file a save **changes** to is checked like a resource's media field,
  inside the save and as the user is then: it exists, is not trashed, is
  visible to the user (`media.view_any` and `media.view`; their own uploads,
  or every file with `media.view_all` or `media.manage_all`), and matches
  `.accept(…)` and `.max_size(…)`. A list holds at most `max` distinct ids.
- The referenced rows are locked (shared) until the save commits.
- A reference kept as it was is not checked again, in a list too
  (reordering or removing files checks only those added; a file trashed
  later stays referenced until it is removed; see
  [when a referenced file is trashed or deleted](media.md#when-a-referenced-file-is-trashed-or-deleted)).
- Their default must be empty (`None`, `vec![]`).

### Layout and checks

`form()` arranges the inputs with the usual [form layouts](forms.md)
(sections, grids, tabs); each input names a field, and may change its label,
help or option labels (not its values). `validate` checks the whole group
before it is saved, inside the write (its `ctx` has the transaction and the
user's current permissions, and `ctx.t` for translated messages). Secrets
quoted in its messages are replaced by `[redacted]`.

```rust
// Sketch: a form() for the demo's SiteSettings (full example above).
fn form() -> Form {
    Form::new().schema(components![
        Section::make("Site").schema(components![TextInput::make("site_name"), TextInput::make("tagline")]),
        Toggle::make("comments_open"),
        NumberInput::make("posts_per_page"),
        SecretInput::make("mail_password"),
        FileUpload::make("logo"),
    ])
}

async fn validate(_ctx: &Ctx, settings: &Self) -> Result<(), ValidationErrors> {
    if !(1..=100).contains(&settings.posts_per_page) {
        return Err(ValidationErrors::field("posts_per_page", "is out of range"));
    }
    Ok(())
}
```

`validate_in(state, ctx, settings)` is `validate` with the application (its
configuration and services), for checks that depend on them; the built-in
mail page uses it to check the server against `[mail]`'s policy.

## Reading settings

Anywhere you have the application state:

```rust
let site: SiteSettings = rustcms::settings::get(&state).await?;
if site.comments_open { /* … */ }
let password = site.mail_password.expose(); // Option<&str>
```

A job reads the settings itself when it runs, as the LMS welcome mail does:

<<< @/../../../examples/lms/src/jobs/send_welcome.rs#read-settings [src/jobs/send_welcome.rs]

- `get` reads the row (one query) and is correct on every instance at once,
  with no cache to clear.
- Inside a transaction, read with
  `rustcms::settings::load::<SiteSettings, _>(&txn, state.keyring())` to
  see the transaction's own writes.
- A group that was never saved reads as its `Default`; a field added to the
  struct later reads as its default until saved.
- Lower-level functions (`rustcms::settings::{read, write, typed, reseal,
  groups}`) read and write stored rows by group name; the panel and
  `key:reencrypt` use them.

## Secrets

![A secret setting: Set, Replace, Clear and Undo](/img/admin/settings-secret.webp)
*A RustCMS demo application: a secret on the Site settings page.*

A `Secret` field is stored encrypted (AES-256-GCM, with a key derived from
`APP_KEY`, bound to its group, field and version). It is never sent to the
browser: the page shows only whether it is set, with **Set a value** /
**Replace**, **Clear** and **Undo**. Saving other settings keeps it.

- `Secret` serializes as `null` and prints (`Debug`) as `Secret(set)` or
  `Secret(unset)`, so a settings struct can be logged, queued or serialized
  without leaking it.
- `.expose()` returns the value (`Option<&str>`), `.is_set()` whether there
  is one; `Secret::new(value)` makes a set one (tests, seeders).
- A secret's default is always unset (`Secret::default()`).

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

## Hooks

### Reacting to a save

`after_save` runs in the save's transaction once the new values are written
(and the audit entry recorded), before the commit, with the group as it was
and as saved; an error rolls the save back. Use it to react to a change in
the same transaction, e.g. dispatch a job that rebuilds what depends on the
setting:

```rust
// Sketch: `RebuildSitemap` is your job.
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

- `before` holds the defaults when the group was never saved. A secret that
  could not be decrypted (which the save replaced or cleared) reads as unset
  there, and a stored value that no longer fits the struct (its declaration
  changed) reads as its default, so the save that replaces it still
  succeeds.
- A save that changes nothing writes nothing and does not call it.
- Secrets of either version are replaced by `[redacted]` in the messages of
  the errors it returns (validation, bad request and internal).

::: danger Secrets in hooks are plain text
`before.mail_password.expose()` and `after.mail_password.expose()` are the
plaintext (an unset or unreadable one is `None`). Compare them, or hand them
to the service that needs them, but never write them to logs, the audit log,
a job's payload or description, an error message or anything sent to the
browser. A job that needs a secret reads the settings itself when it runs.
:::

## Mail

![The Mail settings page with the Send test mail card](/img/admin/settings-mail.webp)
*A RustCMS demo application: Settings → Mail.*

`AdminProvider::mail_settings()` (the `mail` feature, on by default) adds
the **Mail** page: the SMTP server of
[`MailSettings`](../digging-deeper/mail.md).

| Field | Rules |
|---|---|
| `host` | at most 253 characters |
| `port` | required |
| `encryption` | required: `starttls`, `tls` or `none` |
| `username` | at most 255 characters |
| `password` | secret, at most 1,000 characters, kept while `host`, `port` and `username` keep their values; refused with `encryption = none` |
| `from_address` | email, required once `host` is set |
| `from_name`, `reply_to` | at most 255 characters; `reply_to` an email |

The server is checked against `[mail]`'s policy when saved.

The **Send test mail** card (`POST {admin}/api/settings/mail/test` `{to}`)
sends the `rustcms.test` template now, in the user's language, with the
saved settings (never values not yet saved), and answers `{ok}` or
`{ok: false, code}` (`smtp_auth`, `mail_policy`, …). It needs both
`settings.mail.view` and `settings.mail.update`, checked before the body is
read, then counts against a limit of five a minute per user. It gives up
after 15 seconds. It is recorded in the audit log as `mail.test_sent` with
the recipient masked (`a***@example.com`).

## Permissions and history

Each group declares `settings.<group>.view` and `settings.<group>.update`
(run `permission:sync` after adding a group). Saving needs both. Permissions
are checked again inside the write, so access revoked meanwhile is honoured.

Saves are optimistic: the page sends the version it was loaded at, and a save
made by someone else meanwhile is refused (code `stale`); the page offers to
reload. A save that changes nothing creates no version and no entry. Each
change records `settings.updated` in the audit log with the changed values
(secrets and `.audit_redact()` settings as `"[redacted]"`).

![The prompt to reload after someone else saved](/img/admin/settings-stale.webp)
*A RustCMS demo application: a save refused as stale.*

## API

| Method | Path | Body | Response |
|---|---|---|---|
| GET | `{admin}/api/settings/{group}` | | `SettingsResponse` |
| PUT | `{admin}/api/settings/{group}` | `{version, data}` | `SettingsResponse` |
| POST | `{admin}/api/settings/mail/test` | `{to}` | `MailTestResult` `{ok, code?}` |

`SettingsResponse` holds `schemaVersion`, `vocabularyVersion`, `group`,
`label`, `form`, `data` (values other than secrets, defaults for unsaved
ones), `secrets` (each `set`, `unset` or `unreadable`), `version` (a string,
`"0"` when never saved) and `abilities` `{update, test}`.

`data` in a PUT holds the settings to change:

- `version` must be an integer string;
- a field the group does not have is "is not allowed";
- for a secret: a string replaces it (`""` is "is empty", a non-string is
  refused), `null` clears it, leaving it out keeps it.

Without access the answer is 403 before the body is read; an unknown group
is 404 for users who may view some settings, 403 for others.

## Security notes

- Secrets never leave the server: not in responses, logs, the audit log or
  `Debug` output.
- Settings checks run server-side on every save, inside the transaction, with
  the user's current permissions.
- Media references in settings follow the same scope and checks as resource
  fields.

## Related pages

- [Encrypted columns](../digging-deeper/encryption.md): secrets in your own tables
- [Rotating APP_KEY](../operations/app-key.md)
- [Mail](../digging-deeper/mail.md)
- [Media](media.md#media-fields)
- [Admin API](../reference/admin-api.md)
