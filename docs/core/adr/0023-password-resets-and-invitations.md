# ADR 0023: Password resets, invitations, remember me

- Status: accepted (0.3, M20)
- Date: 2026-10-25

## Context

Users must recover access without an administrator, administrators must add
users without choosing their password, and sign-in may be remembered — none
of it may reveal which accounts exist, leak a usable secret (logs, queue,
`Referer`, backups of the jobs table), outlive a credential change, or race
with the revocations of ADR 0005.

## Decision

- **Tokens** `{uuid}.{43 base64url}` in `user_tokens` (HMAC-SHA256 of
  purpose, id and secret under `APP_KEY.derive("rustcms/user-tokens/v1")`,
  constant-time compare; one row per user and purpose; email, `auth_version`,
  `request_id`, `requested_at`, `issued_at`, `expires_at`, `sent_at`).
- **Made only by jobs**, under the user's row lock: the queue payload holds
  the email (reset) or the user id and credential version (invitation), the
  request id and time — never a secret. Same request and sent: nothing;
  older than the live row's request: nothing (superseded); another request
  within `reset_cooldown_secs`: nothing; else delete + insert. A request past
  its deadline (reset 15 min, invitation `invitation_days`) fails
  permanently. `sent_at` set after the mail, conditionally on the hash.
- **No enumeration**: `forgot-password` validates the shape, throttles (IP,
  email hash, total), always inserts one job and answers 202 `{}`; a failed
  insert is logged and still 202.
- **Use**: password rules, IP throttle, strict parse, unlocked check (hash,
  purpose, expiry), then a per-token budget (only after the secret matched),
  Argon2 outside the transaction, then under user lock → token lock: every
  check again (email and `auth_version` equal, enabled, pending matches the
  purpose); password, `invitation_pending = false` (+ email verified, name),
  delete all the user's tokens, `revoke_all`, audit — one transaction. One
  422 for every token problem; infrastructure errors keep their codes.
- **Links** carry the secret in the fragment; the two pages are served with
  `Referrer-Policy: no-referrer`; the SPA's fetches set
  `referrerPolicy: same-origin` so same-origin POSTs keep their `Origin`
  (the Origin check is not relaxed).
- **Invitations**: `RbacService::invite_user` (`users.create` +
  `users.invite`, roles as for create) creates the user pending with
  password `!invited` (not a PHC hash: `Passwords::verify` treats it as an
  unknown user) and dispatches the job in the same transaction;
  `resend_invitation` (`users.invite` + `users.view` under the RBAC lock;
  409 accepted / inactive). Pending users cannot sign in and do not count as
  the last active super admin. An administrator setting the password ends
  the invitation (tokens deleted, `auth_version` moves).
- **Remember me**: `remember` on sign-in sets session data `remember`; such a
  session's idle lifetime and cookie `Max-Age` are `remember_days`.
- **Retention**: expired tokens pruned hourly; reset payloads (an email) stay
  in `jobs` until done or until failed jobs are pruned
  (`failed_retention_days`).

## Consequences

- A run cut after the mail but before `sent_at` mails a second link (the
  first then dies).
- Rotating `APP_KEY` kills every link; restoring an old backup revives old
  rows only with the old key.
