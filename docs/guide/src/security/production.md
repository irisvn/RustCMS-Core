# Production checklist

- [ ] `APP_ENV=production`, `APP_DEBUG=false`.
- [ ] `APP_KEY` generated once (`key:generate`), stored as a secret, never committed.
      Rotating it signs every admin out, ends every JWT sign-in (API tokens stay valid) and makes
      [secret settings](../admin/settings.md#secrets) unreadable until they are entered again.
- [ ] `APP_URL` is the public **https** URL (enables `Secure` cookies and the default allowed origin).
- [ ] `auth.admin_origins` lists every origin serving the admin panel, nothing else.
- [ ] `http.trusted_proxies` lists only your load balancers, so throttling sees real client IPs.
- [ ] Several instances: build with the `redis` feature and set `[redis] url` (one standalone Redis,
      one `redis.prefix` per deployment) so rate limits are shared and periodic jobs mostly run on one instance (best effort:
      jobs are safe to run twice, also at the same time);
      the permission cache needs nothing (it follows the database). Media storage must be shared
      (S3, or one volume mounted by every instance). Without Redis, run one instance.
- [ ] S3 storage: a private bucket, credentials limited to it, an *abort incomplete multipart uploads* lifecycle
      rule, `storage.root` writable by the app only (uploads are staged there), `media:prune` scheduled
      ([File storage](../digging-deeper/storage.md)).
- [ ] Password-hashing and upload concurrency limits are per instance (each bounds its own CPU and disk).
- [ ] Queue workers: decide where they run (`serve`, or `queue:work` processes with `queue.enabled = false` on
      web instances); clocks synchronized (NTP); `queue:failed` watched; job payloads and error messages carry
      no secrets ([Queues](../digging-deeper/queues.md)); `queue.manage` (retry, forget, cancel jobs) only for
      operators.
- [ ] First super admin created with `make:user --super-admin`; `permission:sync` run after each deploy.
- [ ] TLS terminates before the app; HTTP redirects to HTTPS at the proxy.
- [ ] Database backups include `sessions` only if you want sessions to survive restores.
- [ ] `audit.retention_days` set to what you must keep, no longer: the [audit log](../digging-deeper/audit-log.md)
      holds personal data (IP addresses, emails typed in failed sign-ins), and so do backups that include it.
      Restrict who can read `audit_logs`; ship it elsewhere if it must be tamper-proof.
