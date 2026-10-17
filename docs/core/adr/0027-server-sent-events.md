# ADR 0027: Server-sent events on admin routes

- Status: accepted (0.4, W2a)
- Date: 2026-10-17

## Context

Panel pages need live data (logs, stats, status) from things that are not
rows. Browsers' `EventSource` and `WebSocket` cannot send an `Authorization`
header, so streams run on the session cookie. A long-lived connection must
not outlive a sign-out or a lost permission. It must not let another site
read the user's data, and must not exhaust the server. WebSocket (commands,
terminals) is deferred to W2b; commands go through W1 `AdminRoutes` POSTs.

## Decision

- **Declaration:** `AdminRoutes::events(path, AdminAccess, handler)`.
  - GET only; `HEAD` is 405 and starts nothing.
  - W1's access check runs before extractors.
  - The handler takes `LiveSession` (first argument) and returns
    `live.stream(events).target(check)`.
  - Streams are read-only by contract: changes belong in a POST.
- **Admission:**
  - no CSRF token: a GET stream changes nothing, and CORS keeps other
    origins from reading it;
  - it must come from the panel's own origin:
    - an `Origin`, when sent, must be exactly an allowed origin (not `null`,
      no path);
    - `Sec-Fetch-Site`, when sent, must be `same-origin`, which refuses
      same-site siblings that `SameSite=Lax` cookies still reach;
    - a request with neither (old browsers) needs a same-origin `Referer`;
  - repeated or unreadable headers are refused (403).
  - Same-origin `fetch` GETs send no `Origin`, so the Fetch Metadata rule is
    what admits the panel's own streams.
- **Live authorization:**
  - every `[realtime] revalidate_secs` (30), at a random initial offset and
    with a 5 s timeout, each stream rechecks on its own:
    - the session row (same id, unexpired, same user and version);
    - the user (active, same `auth_version`);
    - a fresh principal through the resolver's generation, against the
      route's access and the stream's target check;
  - any failure or error ends delivery at once and sends a best-effort
    `event: closed {"reason"}`: `signed_out`, `forbidden`, `shutdown`,
    `lagging` or `error`;
  - revocation latency is at most `revalidate_secs + 5 s`;
  - a re-issued session (credential change) has another id, so its streams
    end;
  - live traffic never renews the session.
- **Isolation from the client:**
  - a task under `Shutdown` produces into a bounded queue (64 events,
    1 MiB) that the response body only reads;
  - a client that stops reading cannot block rechecks or cancellation;
  - a full queue ends the stream (`lagging`).
- **Limits, per instance:**
  - `max_connections` (1000) and `max_per_user` (10): non-waiting permits
    owned by the stream, so every path releases them; when full, 429;
  - opens per user (60/min) and per IP (300/min);
  - events at most 64 KiB;
  - a keep-alive every 15 s.
- **Shutdown:**
  - on the signal, `Shutdown` closes admission (503) and cancels streams,
    which end with `closed: shutdown`;
  - they get `shutdown_grace_secs` (5) before being aborted, and the server
    is not awaited longer;
  - queue workers drain as before.
- **Client** (`ctx.realtime.events`):
  - `fetch`-based, so statuses are visible and nothing reconnects on its own;
  - jittered backoff after drops;
  - final on 401, 403, 404 and `signed_out`/`forbidden`;
  - closed at once on a session epoch change and on unmount.

## Consequences

- An open stream costs about 3 queries per recheck plus its target check.
  This is about 100 queries/s at 1000 streams every 30 s, so size the pool
  (10 by default) or raise the interval.
- Limits and the hub are per instance; deployments with several instances
  multiply them.
- Handlers must not change state. A per-target check (`target`) is the place
  for "may this user still see container X", and it reruns with every
  recheck.
- E2E admission is checked on Chrome only (the maintainer's choice); the
  header rules are tested directly against a real server on every database.
