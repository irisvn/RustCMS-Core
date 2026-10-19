# 0005 — Own session store, synchronizer CSRF, deterministic guards

- Status: accepted
- Date: 2026-10-12

## Context

The admin panel needs cookie sessions with CSRF protection; the API needs
revocable tokens. Credentials must be revocable immediately (password change,
disabling) and sessions must survive a leaked database dump.

## Decision

- A small session store in `rustcms-auth` instead of `tower-sessions`: the cookie
  carries a random token, the `sessions` table its HMAC-SHA-256 keyed directly by
  `APP_KEY`; ids regenerate at
  sign-in and on credential changes; sliding expiry with hourly pruning.
- CSRF uses a synchronizer token stored in the session plus a strict
  `Origin`/`Referer` allowlist, on every state-changing admin request including
  sign-in.
- One middleware selects the guard by path (admin prefix → session, otherwise
  bearer). A present but invalid credential is a 401, never a fallback.
- `users.auth_version` is checked on every request; there is no cache in front
  of it, so revocation has no delay.
- Roles and permissions change only through `RbacService`, which carries the
  anti-escalation rules and locks the super-admin role row for the
  last-super-admin invariant.

## Consequences

- One user lookup per authenticated request; permissions are cached (≤ 30 s,
  keyed by a database counter every RBAC change advances, so every instance
  sees a change on its next request — since 0.2).
- Rate limits are per process, or shared through Redis (0.2, ADR 0012).
- JWT (0.2, ADR 0011) is another bearer driver selected by token shape.
