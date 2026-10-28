# Mail

Mail is rendered from templates and sent by the driver set in `[mail]`. With
SMTP, the server is set in the panel (**Settings → Mail**) or in the
configuration, and always checked against the operator's policy. Mail sent
from a change is queued with that change: it exists only if the change
commits, and the queue retries it when the server is temporarily away.

Mail needs the `mail` feature (on by default in `rustcms`) and the provider:

```rust
app.provider(rustcms::mail::MailProvider);
app.provider(AdminProvider::new().mail_settings()); // the Mail settings page
```

## Configuration

```toml
# config/mail.toml
driver = "smtp"           # disabled (default), smtp, log, memory
source = "settings"       # the panel's Mail settings (default), or "config" ([mail.smtp])
allowed_hosts = ["smtp.example.com", "*.mailgun.org"]   # or ["*"]
allowed_ports = [465, 587]
allow_insecure = false    # encryption = "none" (then without authentication)
timeout_secs = 30         # a whole send, connection included
max_per_minute = 60       # per process
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
# password: RUSTCMS_MAIL__SMTP__PASSWORD
```

| Driver | Sends |
|---|---|
| `disabled` | nothing: sending fails with `mail_disabled` |
| `smtp` | through the server (TLS verified, `STARTTLS` required unless `tls` or `none`) |
| `log` | nothing; logs the number of recipients and the message id (the body too with `log_body`, local only) |
| `memory` | nothing; keeps the mails for tests (`TestApp::mails`) |

**Who may change the server.** With `source = "settings"`, whoever has
`settings.mail.update` uses the saved SMTP credentials and makes the
application connect to the server they name: treat that permission like a
system administrator's. `allowed_hosts` and `allowed_ports` bound where mail
may go; in production `allowed_hosts` is required (boot fails otherwise; `*`
says any server is fine). The policy is checked when the configuration is
loaded, when the page is saved, and at every send. Changing the host, port or
user name without giving the password again clears the saved password: it is
never sent to a server it was not given for.

## Sending

```rust
use rustcms::mail::{self, MailJob, Message};

// Later, with the change (outbox): queued in the same transaction.
mail::send_later(&txn, MailJob {
    to: vec![learner.email.clone()],
    template: "welcome".into(),
    locale: learner_locale,
    data: json!({ "name": learner.name, "course": course.title }),
}).await?;

// Now (in a request or a job of your own).
let rendered = mail::render(&state, "welcome", "vi", &data)?;
mail::send(&state, &Message::rendered([to], rendered)).await?;
```

`send_later` queues a `mail.send` job: up to 8 runs with backoff on temporary
failures (`smtp_connect`, `smtp_temporary`, `smtp_timeout`, `smtp_tls`,
`smtp_throttled`); permanent ones (`smtp_rejected`, `smtp_auth`,
`mail_not_configured`, `mail_policy`, `mail_template`, …) fail at once, shown
on the Queue page; `queue:retry` runs one again once fixed. Each run has its
own `Message-ID` (`{job}.{run}@{host of APP_URL}`), so a mail repeated after a
run that the server took but did not confirm can be recognized.

**No secrets in `data`.** It stays in the `jobs` table until the job is done
(failed jobs for `queue.failed_retention_days`). A mail carrying a secret (a
password reset link) is sent by a job of its own, which makes the secret when
it runs and renders and sends the mail itself (`mail::render` + `mail::send`).

Delivery is at least once: a mail may be sent twice when a run is cut after
the server took it. A job that must not repeat a mail records that it sent it
(`queue::once`) after sending, and checks first.

## Templates

A template is three files per language in `resources/views/mail/{locale}/`:
`{name}.subject.txt` (one line), `{name}.txt`, and optionally `{name}.html`.

```rust
app.mail_template(rustcms::mail_template!("welcome", "vi", html));
app.mail_template(rustcms::mail_template!("welcome", "en", html));
```

They are embedded in the binary; in local development (debug build,
`APP_ENV=local`) they are read again from the files at each send. They are
minijinja templates with `data`, `app_name`, `app_url` and `locale`. Text is
not escaped; HTML always is, and nothing marks a value safe (no `safe`,
`tojson`, `escape`, `autoescape`, includes or inheritance). Work, recursion,
data (64 KiB) and output (256 KiB per part) are bounded. A subject must be one
line: a line break, from the template or from `data`, fails the mail
(`mail_template`), so no header can be injected. The template of the locale
asked for is used, else the one of `app.locale`.

## Errors

Errors are codes (`rustcms::mail::MailError`); the server's reply, the
addresses, the password and the body never reach a log, a job's error or a
response. See [Error codes](../reference/error-codes.md#mail).

## Testing

With `rustcms-testing`'s `mail` feature, tests use the `memory` driver
(`test_config` sets it):

```rust
rustcms::queue::run_until_idle(app.state(), None, 10).await?;
let mails = app.take_mails();
assert_eq!(mails[0].to, ["learner@example.com"]);
```

For a real SMTP server in development, run Mailpit
(`docker compose up mailpit`, UI on <http://localhost:58025>) with
`allow_insecure = true`, `encryption = "none"`, port `51025`.
