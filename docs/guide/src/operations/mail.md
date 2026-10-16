# Running mail

See [Mail](../digging-deeper/mail.md) for the configuration.

- **Check it**: Settings → Mail → *Send test mail* sends now with the saved
  settings and shows the error code (`smtp_auth`, `smtp_tls`, `mail_policy`…).
- **The server is down**: mails wait in the queue and are retried with
  backoff, then fail: `mail.send` and invitations 8 runs (about 20 minutes
  of waits), password resets 5 runs (about 2½ minutes; the user asks again).
  Sending time and a busy queue add to these. Once it is back,
  `rustcms queue:retry --all` (or retry the `mail.send` jobs on the Queue page).
- **A refused message, a bad template**: fails at once (not retried). Fix,
  then retry.
- **The password cannot be decrypted** (`mail_settings_unreadable`, after
  `APP_KEY` changed): enter it again.
- **Reset links**: a link works `auth.password_reset_minutes` (60) and once;
  a request waits at most 15 minutes in the queue. Users ask again from the
  sign-in page.
- **Invitations**: links work `auth.invitation_days` (7); *Send the invitation
  again* on the user page makes a new one (the old one stops working).
- **Several instances**: `mail.max_per_minute` counts per process.
- **Development**: Mailpit (`docker compose up mailpit`, <http://localhost:58025>)
  catches every mail; or the `log` driver with `RUSTCMS_MAIL__LOG_BODY=true`
  (local only) prints them.
