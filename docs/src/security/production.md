# Production checklist

- [ ] `APP_ENV=production`, `APP_DEBUG=false`.
- [ ] `APP_KEY` generated once (`key:generate`), stored as a secret, never committed.
      Rotating it signs every admin out and ends every JWT sign-in (API tokens stay valid).
- [ ] `APP_URL` is the public **https** URL (enables `Secure` cookies and the default allowed origin).
- [ ] `auth.admin_origins` lists every origin serving the admin panel, nothing else.
- [ ] `http.trusted_proxies` lists only your load balancers, so throttling sees real client IPs.
- [ ] Several instances: build with the `redis` feature and set `[redis] url` (one standalone Redis,
      one `redis.prefix` per deployment) so rate limits are shared and two instances never run one periodic job at the
      same time (each may still run it in its own interval: jobs are safe to repeat);
      the permission cache needs nothing (it follows the database). Media storage must be shared
      (S3, or one volume mounted by every instance). Without Redis, run one instance.
- [ ] Password-hashing and upload concurrency limits are per instance (each bounds its own CPU and disk).
- [ ] First super admin created with `make:user --super-admin`; `permission:sync` run after each deploy.
- [ ] TLS terminates before the app; HTTP redirects to HTTPS at the proxy.
- [ ] Database backups include `sessions` only if you want sessions to survive restores.
