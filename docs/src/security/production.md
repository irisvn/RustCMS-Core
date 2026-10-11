# Production checklist

- [ ] `APP_ENV=production`, `APP_DEBUG=false`.
- [ ] `APP_KEY` generated once (`key:generate`), stored as a secret, never committed.
- [ ] `APP_URL` is the public **https** URL (enables `Secure` cookies and the default allowed origin).
- [ ] `auth.admin_origins` lists every origin serving the admin panel, nothing else.
- [ ] `http.trusted_proxies` lists only your load balancers, so throttling sees real client IPs.
- [ ] One application instance (0.1 keeps rate limits and the permission cache in memory).
- [ ] First super admin created with `make:user --super-admin`; `permission:sync` run after each deploy.
- [ ] TLS terminates before the app; HTTP redirects to HTTPS at the proxy.
- [ ] Database backups include `sessions` only if you want sessions to survive restores.
