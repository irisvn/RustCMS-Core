# Mail

Mail is rendered from templates and sent by the driver set in `[mail]`. With
SMTP, the server is set in the panel (**Settings → Mail**) or in the
configuration, and always checked against the operator's policy. Mail sent
from a change is queued with that change: it exists only if the change
commits, and the queue retries it when the server is temporarily away.

## Quick start

Mail needs the `mail` feature (on by default in `rustcms`) and the provider.
The demo registers it with the others:

<<< @/../../../examples/demo/src/bootstrap/providers.rs#register [src/bootstrap/providers.rs]

Add `.mail_settings()` to your `AdminProvider` for the **Settings → Mail**
page:

```rust
app.provider(AdminProvider::new().mail_settings());
```

Write a template (`resources/views/mail/en/welcome.subject.txt`,
`welcome.txt`), register it, then queue a mail with the change that causes it:

```rust
use rustcms::mail::{self, MailJob};

app.mail_template(rustcms::mail_template!("welcome", "en"));

// In the change's transaction:
mail::send_later(&txn, MailJob {
    to: vec![learner.email.clone()],
    template: "welcome".into(),
    locale: "en".into(),
    data: json!({ "name": learner.name, "course": course.title }),
}).await?;
```

Locally, `driver = "log"` logs each mail instead of sending it. It still
needs a sender address: saved on **Settings → Mail**, or `source = "config"`
with `[smtp] from_address` (otherwise sending fails with
`mail_not_configured`), as the LMS's `config/mail.toml` does.

## Configuration

```toml
# config/mail.toml
driver = "smtp"           # disabled (default), smtp, log, memory
source = "settings"       # the panel's Mail settings (default), or "config" ([mail.smtp])
allowed_hosts = ["smtp.example.com", "*.mailgun.org"]   # or ["*"]
allowed_ports = [465, 587]
allow_insecure = false    # encryption = "none" (then without authentication)
timeout_secs = 30         # the SMTP exchange, connection included
max_per_minute = 60       # per process (burst, then a steady rate)
log_body = false          # log driver only, APP_ENV=local only
# tls_ca_file = "dev-ca.pem"   # an extra trusted root (a test server)

# With source = "config":
[smtp]
host = "smtp.example.com"
port = 587
encryption = "starttls"   # starttls (required, never optional), tls (port 465), none
username = "app"
from_address = "no-reply@example.com"
from_name = "Example"
reply_to = "support@example.com"
# password: RUSTCMS_MAIL__SMTP__PASSWORD (or RUSTCMS_MAIL__SMTP__PASSWORD_FILE)
```

`[mail]`:

| Key | Default | |
|---|---|---|
| `driver` | `disabled` | `disabled`, `smtp`, `log`, `memory` (see below) |
| `source` | `settings` | where the SMTP server comes from: `settings` (the panel's Mail settings, read at each send) or `config` (`[mail.smtp]`) |
| `allowed_hosts` | `[]` | servers mail may go to: names, `*.example.com`, or `*`; empty allows any host, and is refused in production with `driver = "smtp"` and `source = "settings"` |
| `allowed_ports` | `[465, 587]` | |
| `allow_insecure` | `false` | allows `encryption = "none"` (then without authentication) |
| `timeout_secs` | `30` | 1 to 300; bounds the SMTP exchange, connection included (reading the settings and waiting for the rate limit come before it) |
| `max_per_minute` | `60` | 1 to 10,000; a token bucket per process (each web and worker process counts alone): a burst of `max_per_minute`, refilled at `max_per_minute` per minute; a send waits up to a minute for a token, then fails with `smtp_throttled` |
| `log_body` | `false` | `log` driver: also log recipients, subject and body; only with `APP_ENV=local` |
| `tls_ca_file` | | an extra root certificate (PEM) trusted for SMTP TLS (a test server) |

`[mail.smtp]` (with `source = "config"`):

| Key | Default | |
|---|---|---|
| `host`, `port` | | the server (465: TLS, 587: STARTTLS) |
| `encryption` | `starttls` | `starttls`, `tls` or `none` |
| `username` | | empty: no authentication |
| `password` | | set it in the environment (`RUSTCMS_MAIL__SMTP__PASSWORD`, or `_FILE`), not in a committed file |
| `from_address`, `from_name` | | the sender |
| `reply_to` | | the `Reply-To` address |

Both sections refuse unknown keys: a typo fails the boot. Boot also fails on
a value out of range, a `[mail.smtp]` server outside the policy, `log_body`
outside local development, or no `allowed_hosts` in production when the
panel sets the server.

| Driver | Sends |
|---|---|
| `disabled` | nothing: sending fails with `mail_disabled` |
| `smtp` | through the server (TLS verified, `STARTTLS` required unless `tls` or `none`) |
| `log` | nothing; logs the number of recipients and the message id (recipients, subject and body too with `log_body`, local only) |
| `memory` | nothing; keeps the mails for tests (`TestApp::mails`) |

![The Mail settings page with the Send test mail card](/img/admin/settings-mail.webp)
*A RustCMS demo application: Settings → Mail.*

**Who may change the server.** With `source = "settings"`, whoever has
`settings.mail.update` uses the saved SMTP credentials and makes the
application connect to the server they name: treat that permission like a
system administrator's. `allowed_hosts` and `allowed_ports` bound where mail
may go; in production `allowed_hosts` is required (boot fails otherwise; `*`
says any server is fine). The policy is checked when the configuration is
loaded, when the page is saved, and at every send. Changing the host, port or
user name without giving the password again clears the saved password: it is
never sent to a server it was not given for. The saved settings are the
`mail` settings group (`rustcms::mail::MailSettings`); the password is a
secret sealed with `APP_KEY`.

## Sending

### Later, with a change

`mail::send_later(&txn, MailJob { to, template, locale, data })` queues a
`mail.send` job in the change's transaction:

- 1 to 50 recipients (`mail::MAX_RECIPIENTS`); otherwise it returns an error.
- `data` at most 64 KiB serialized (`mail::MAX_DATA_BYTES`).
- Up to 8 runs, 2 minutes each, with backoff on temporary failures
  (`smtp_connect`, `smtp_temporary`, `smtp_timeout`, `smtp_tls`,
  `smtp_throttled`); permanent ones (`smtp_rejected`, `smtp_auth`,
  `mail_not_configured`, `mail_policy`, `mail_template`, …) fail at once,
  shown on the Queue page; `queue:retry` runs one again once fixed.
- Each run has its own `Message-ID` (`{job}.{run}@{host of APP_URL}`), so a
  mail repeated after a run that the server took but did not confirm can be
  recognized.

**No secrets in `data`.** It stays in the `jobs` table until the job is done
(failed jobs for `queue.failed_retention_days`). A mail carrying a secret (a
password reset link) is sent by a job of its own, which makes the secret when
it runs and renders and sends the mail itself.

### Now

```rust
let rendered = mail::render(&state, "welcome", "en", &data)?;
mail::send(&state, &Message::rendered([to], rendered)).await?;
```

`render` and `send` return `Result<_, MailError>`. A `Message` has `to` (1
to 50 addresses, `Name <address>` allowed), `subject`, `text`, an optional
`html` and an optional `message_id`; `Message::rendered` fills them from a
template. Sending in a request makes the request wait for the server: prefer
a job.

### Sending from your own job

When a mail needs more than a template and data (rows read when it runs,
settings, a receipt), send it from a [job](queues.md) of your own. The LMS
welcome job renders and sends, then maps the error:

<<< @/../../../examples/lms/src/jobs/send_welcome.rs#send [src/jobs/send_welcome.rs]

`Failure::from(err)` keeps the error's code and message; `.permanent()`
fails the job at once when `err.is_permanent()`. The full job is on
[Queues](queues.md#writing-a-job).

Delivery is at least once: a mail may be sent twice when a run is cut after
the server took it. This job records a receipt (`queue::once`) after
sending, and checks it first: once the receipt commits, later runs send
nothing. A crash between the send and the commit, or two runs at the same
time, can still send the mail twice. No pattern makes an SMTP send
exactly-once; the run's `Message-ID` lets recipients' systems recognize a
repeat of a `send_later` mail.

## Templates

A template is two or three files per language in
`resources/views/mail/{locale}/`: `{name}.subject.txt` (one line),
`{name}.txt`, and optionally `{name}.html`. The LMS registers its welcome
mail in English and Vietnamese, with HTML:

<<< @/../../../examples/lms/src/providers/app_service_provider.rs#mail-templates [src/providers/app_service_provider.rs]

`mail_template!(name, locale)` embeds the subject and text; add `html` to
embed `{name}.html` too. The LMS's English files:

::: code-group

```text [welcome.subject.txt]
Welcome to {{ data.course }}
```

```text [welcome.txt]
Hello {{ data.name }},

You have a seat in {{ data.course }} at {{ data.school }}.

{{ app_url }}
```

```html [welcome.html]
<p>Hello {{ data.name }},</p>
<p>You have a seat in <strong>{{ data.course }}</strong> at {{ data.school }}.</p>
<p><a href="{{ app_url }}">{{ app_url }}</a></p>
```

:::

- They are embedded in the binary; in local development (debug build,
  `APP_ENV=local`) they are read again from the files at each send.
- They are minijinja templates with `data`, `app_name`, `app_url` and
  `locale`.
- Text is not escaped; HTML always is, and nothing marks a value safe (no
  `safe`, `tojson`, `escape`, `autoescape`, includes or inheritance).
- Work, recursion, data (64 KiB) and output (256 KiB per part) are bounded.
- A subject must be one line: a line break, from the template or from
  `data`, fails the mail (`mail_template`), so no header can be injected.
- The template of the locale asked for is used, else the one of
  `app.locale`.
- `app.mail_template` panics at boot on an invalid template, or one added
  twice.

## Errors

Errors are codes (`rustcms::mail::MailError`, its code in `.0`); the
server's reply, the addresses, the password and the body never reach a log,
a job's error or a response. `err.is_permanent()` says whether retrying can
help, `err.message()` gives a short English message. See
[Error codes](../reference/error-codes.md#mail).

## Testing

With `rustcms-testing`'s `mail` feature, tests use the `memory` driver
(`test_config` sets it):

```rust
rustcms::queue::run_until_idle(app.state(), None, 10).await?;
let mails = app.take_mails();
assert_eq!(mails[0].to, ["learner@example.com"]);
```

Each kept mail (`SentMail`) has `to`, `from`, `subject`, `text`, `html` and
`message_id`. Outside `TestApp`, `mail::mailer(&state)` gives the bound
`Mailer`, whose `sent()` and `take_sent()` do the same.

For a real SMTP server in development, run Mailpit
(`docker compose up mailpit`, UI on <http://localhost:58025>). Its port is
outside the default policy, so allow it:

```toml
# config/mail.toml
driver = "smtp"
source = "config"
allowed_hosts = ["localhost"]
allowed_ports = [51025]
allow_insecure = true

[smtp]
host = "localhost"
port = 51025
encryption = "none"       # no user name: no authentication
from_address = "no-reply@example.com"
```

## Reference

| Item | |
|---|---|
| `MailProvider` | binds the mailer for `[mail]` and registers the `mail.send` job |
| `send_later(&txn, MailJob)` | queues a mail with the change |
| `render(&state, name, locale, &data)` | a `Rendered` (subject, text, html) |
| `send(&state, &message)` | sends now |
| `mailer(&state)` | the bound `Mailer` (`config()`, `sent()`, `take_sent()`) |
| `valid_address(text)` | whether `text` is one plain address (`user@domain`, at most 254 characters) |
| `MailSettings` | the `mail` settings group the panel edits |
| `MAX_RECIPIENTS`, `MAX_DATA_BYTES` | 50, 64 KiB |

## Related

- [Running mail](../operations/mail.md)
- [Queues](queues.md)
- [Settings](../admin/settings.md)
- [Error codes](../reference/error-codes.md#mail)
