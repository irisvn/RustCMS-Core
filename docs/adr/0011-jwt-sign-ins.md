# 0011 — JWT access tokens with rotating refresh tokens

- Status: accepted
- Date: 2026-10-15

## Context

API clients that cannot keep a long-lived secret (mobile and browser apps) want
short access tokens and a way to renew them. 0.1 promised that revocation
applies from the next request (ADR 0005); a stateless JWT alone cannot keep that
promise.

## Decision

- **Access token:** compact JWS, HS256 only, written with the existing `hmac`
  and `sha2` crates (no `jsonwebtoken`). The header must equal our fixed header
  byte for byte, so no other algorithm, `none` or header field is ever
  considered. Strict base64url, a 4 KiB bound, the signature verified in
  constant time before the payload is parsed, typed claims with unknown and
  duplicate fields refused, `exp > now`, `iat` at most 60 s ahead.
- **Keys** are derived from `APP_KEY` per purpose
  (`HMAC-SHA256(APP_KEY, "rustcms/jwt/v1")`, `"rustcms/refresh/v1"`): rotating
  the key ends every sign-in, as it ends sessions.
- **Not stateless:** each request also checks the user (active, `auth_version`)
  and the refresh family (owned by the user, same `auth_version`, not revoked,
  not expired). That costs one indexed read and keeps revocation immediate.
  Abilities come from the family row, fixed at sign-in.
- **Refresh families:** opaque single-use refresh tokens (keyed hash stored),
  rotated on every use under the locks of user → family → token (the order
  issuance and revocation use). A spent token used again revokes the family:
  theft detection, at the cost that clients must serialize refreshes (a retry
  after a lost response also ends the sign-in). The secret is checked before
  anything is decided, so a guessed token id changes nothing. Spent tokens are
  kept until the family ends, so any replay is recognized.
- **Limits:** sign-in shares the password limits and adds a per-user budget;
  refresh and logout are limited per IP before any database work and per
  family once the secret is known good.

## Consequences

- Access tokens are short (15 minutes by default) but still revoked at once.
- Clients must store refresh tokens safely and refresh one at a time.
- `revoke_tokens_on_password_change` concerns API tokens only; JWT sign-ins
  always end with a credential change.
