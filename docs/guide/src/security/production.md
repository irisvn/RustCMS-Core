# Production checklist

Go through this list before the first deploy, and again when you add
instances, Redis, S3 or mail. Each item links to the page that explains it.

## Environment and keys

- [ ] `APP_ENV=production` (also the default): `db:seed`, `migrate:refresh`
      and `migrate:fresh` then need `--force`. `migrate:rollback` has no such
      guard: it runs in production as it does elsewhere.
- [ ] `APP_DEBUG=false`. It is read into `app.debug`, but the framework does
      not act on it: internal errors always answer a fixed "Server error."
      whatever its value. Your own code may read it.
- [ ] `APP_KEY` generated once (`key:generate --show`), stored as a secret,
      never committed. Rotating it signs every admin out and ends every JWT
      sign-in (API tokens stay valid). Keep the old key in `APP_PREVIOUS_KEYS`
      and run `key:reencrypt`, then check that `key:reencrypt --dry-run`
      exits successfully before removing the old key, or
      [secret settings](../admin/settings.md#secrets) and
      [encrypted columns](../digging-deeper/encryption.md) become unreadable
      ([Rotating APP_KEY](../operations/app-key.md)).
- [ ] Secrets come from the environment or from files (`APP_KEY_FILE`,
      `DATABASE_URL_FILE`, …: [secrets from files](../getting-started/configuration.md#from-files-docker-and-kubernetes-secrets)),
      not from `config/*.toml` in the repository.
- [ ] `APP_URL` is the public **https** URL. It makes session cookies `Secure`
      and is the default allowed origin of the admin panel.
- [ ] `auth.secure_cookies` is unset or `true`. Never force it to `false`
      behind https.

## Admin panel and sign-in

- [ ] `auth.admin_origins` lists every origin serving the admin panel, nothing
      else ([CSRF](authentication.md#csrf)).
- [ ] `http.trusted_proxies` lists only your load balancers, so throttling and
      the audit log see real client IPs
      ([Client IP and proxies](../architecture/lifecycle.md#client-ip-and-proxies)).
- [ ] `auth.jwt.enabled = false` when no client uses JWT sign-ins.
- [ ] `auth.password_reset = true` only with a mail driver configured: boot
      fails otherwise ([Mail](../digging-deeper/mail.md)).
- [ ] First super admin created with `make:user --super-admin`;
      `permission:sync` run after each deploy.
- [ ] `settings.mail.update` (SMTP settings, test mails) only for
      administrators; `queue.manage` (retry, forget, cancel jobs) and
      `schedules.manage` (run a schedule now) only for operators.
- [ ] [Runtime branding](../admin/customizing.md#runtime-branding): the brand
      name and logo are public (the sign-in page shows them); only operators
      may change them; you know the recovery switch
      (`RUSTCMS_ADMIN__BRANDING=static`, then restart) if a branding makes the
      panel unusable.

## Database

- [ ] Migrations run once per deploy (`migrate`, then `permission:sync`), with
      a backup taken first; you know how to go back: `migrate:rollback` with
      the new binary for your migrations, or the old binary with
      `migrate --allow-newer` ([Upgrading](../getting-started/deployment.md#upgrading)).
- [ ] MySQL: the database, its tables and `sql_mode` pass the startup check
      ([MySQL](../basics/database.md#mysql)).
- [ ] `database.max_connections` (10 per process) fits the server's connection
      limit for every instance and worker process together.
- [ ] Database backups include `sessions` only if you want sessions to survive
      restores ([Backup and restore](../operations/backup-restore.md)).
- [ ] `audit.retention_days` set to what you must keep, no longer: the
      [audit log](../digging-deeper/audit-log.md) holds personal data (IP
      addresses, emails typed in failed sign-ins), and so do backups that
      include it. Restrict who can read `audit_logs`; ship it elsewhere if it
      must be tamper-proof. `audit.sign_in_failures_per_minute` (600) bounds
      how many failed sign-ins are recorded one by one.

## HTTP

- [ ] TLS terminates before the app; HTTP redirects to HTTPS at the proxy.
- [ ] `http.body_limit` (1 MiB) and `http.timeout_secs` (30) suit your API;
      the proxy's request body limit is at least `storage.max_upload_bytes`
      (10 MiB) plus 64 KiB (the media upload route accepts that much, for the
      multipart framing).
- [ ] WebSocket routes ([realtime](../admin/realtime.md#websocket)): the proxy
      passes `Upgrade` and allows long-lived connections, and its access and
      APM logs drop the `Sec-WebSocket-Protocol` request header (it carries
      the session's CSRF token). The `[realtime]` limits (open streams per
      instance and per user, opens per minute, message size) suit your
      traffic ([Configuration](../getting-started/configuration.md)).
- [ ] `log.json = true` when a log shipper reads the output.

## Several instances and workers

- [ ] Several instances: build with the `redis` feature and set `[redis] url`
      (one standalone Redis, one `redis.prefix` per deployment) so rate limits
      are shared and periodic jobs mostly run on one instance (best effort:
      jobs are safe to run twice, also at the same time); the permission cache
      needs nothing (it follows the database). Media storage must be shared
      (S3, or one volume mounted by every instance). Without Redis, run one
      instance ([Several instances](../digging-deeper/instances.md)).
- [ ] Password-hashing and upload concurrency limits are per instance (each
      bounds its own CPU and disk).
- [ ] Queue workers: decide where they run (`serve`, or `queue:work` processes
      with `queue.enabled = false` on web instances); clocks synchronized
      (NTP); `queue:failed` watched; job payloads and error messages carry no
      secrets ([Queues](../digging-deeper/queues.md)).
- [ ] The hourly clean-up of sessions, sign-ins, links and old audit entries
      (`rustcms.auth-prune`) runs only in `serve` with `[schedule] enabled`:
      keep the scheduler on at least one instance, or cron it
      ([Authentication](authentication.md#clean-up)).

## Storage

- [ ] S3 storage: a private bucket, credentials limited to it, an *abort
      incomplete multipart uploads* lifecycle rule, `storage.root` writable by
      the app only (uploads are staged there), `media:prune` scheduled
      ([File storage](../digging-deeper/storage.md)).

## Related pages

- [Deployment](../getting-started/deployment.md)
- [Configuration](../getting-started/configuration.md)
- [Authentication](authentication.md)
- [Credential revocation](revocation.md)
- [What is kept, and for how long](../operations/retention.md)
