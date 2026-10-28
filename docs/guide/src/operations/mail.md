# Running mail

What to check and what to do when mail does not arrive. See
[Mail](../digging-deeper/mail.md) for the configuration.

## Checking the server

![Settings → Mail: Send test mail and its result](/img/operations/mail-test.webp)
*The demo application: Settings → Mail, a test mail refused with an error code.*

**Settings → Mail → *Send test mail*** sends now with the saved settings and
shows the error code (`smtp_auth`, `smtp_tls`, `mail_policy`…). It is
recorded in the audit log (`mail.test_sent`, the address masked).

In production with `driver = "smtp"` and the server set in the panel,
`mail.allowed_hosts` must list the servers mail may go to (or `*`): the
application does not start otherwise.

## When something fails

- **The server is down**: mails wait in the queue and are retried with
  backoff, then fail: `mail.send` and invitations 8 runs (about 20 minutes
  of waits), password resets 5 runs (about 2½ minutes; the user asks again).
  Sending time and a busy queue add to these. Once it is back, list the
  failed jobs (`rustcms queue:failed`, or the Queue page filtered on failed)
  and retry the mail jobs by id (`rustcms queue:retry <id>`). Do not use
  `queue:retry --all`: it also runs every other failed job again (imports,
  your own jobs, interrupted Run now tasks), whose effects may already have
  happened. A mail that timed out may have been delivered: check before
  retrying it.
- **A refused message, a bad template, a refused password**: fails at once
  (not retried). Fix, then retry.
- **The password cannot be decrypted** (`mail_settings_unreadable`): usually
  `APP_KEY` changed without the old key in `APP_PREVIOUS_KEYS`. Put the old
  key back in `APP_PREVIOUS_KEYS` on every process and restart; enter the
  password again only if no key you hold opens it
  ([Rotating APP_KEY](app-key.md)).
- **Too many mails** (`smtp_throttled`): each process sends at most a burst
  of `mail.max_per_minute` (60), then about `max_per_minute / 60` per second;
  a send waits up to a minute for its turn before failing with this code,
  and the job is retried later.
- **Nothing is sent at all**: check that a queue worker runs somewhere
  ([Running the queue](queue.md)), that `driver` is not `disabled` or
  `log`, and that a sender address is set (`mail_not_configured` otherwise).

## Reset links and invitations

- **Reset links**: a link works `auth.password_reset_minutes` (60) and once.
  A request waits at most 15 minutes in the queue: an older one fails as
  `expired_before_sent` and is not sent. A user gets a new link at most once
  per `auth.reset_cooldown_secs` (60). Users ask again from the sign-in page.
- **Invitations**: links work `auth.invitation_days` (7, at most 30);
  *Send the invitation again* on the user page makes a new one (the old one
  stops working).

## Several instances

`mail.max_per_minute` counts per process (a token bucket: a burst of
`max_per_minute`, then a steady rate of `max_per_minute` per minute): with
several web and worker processes, the total rate and burst are that many
times higher. Set it so the total stays within your provider's limit.

## Development

- Mailpit (`docker compose up mailpit`) catches every mail: SMTP on port
  51025, UI on <http://localhost:58025>; a second one with STARTTLS
  (`mailpit-tls`) listens on 51026. Allow its port and plain SMTP:
  `allowed_ports = [51025]`, `allow_insecure = true`, `encryption = "none"`
  without a user name ([Mail](../digging-deeper/mail.md#testing)).
- Or the `log` driver with `RUSTCMS_MAIL__LOG_BODY=true` (only with
  `APP_ENV=local`) prints them. It still needs a sender address: saved on
  **Settings → Mail**, or `source = "config"` with `[smtp] from_address`.

## Related

- [Mail](../digging-deeper/mail.md)
- [Running the queue](queue.md)
- [Error codes](../reference/error-codes.md#mail)
