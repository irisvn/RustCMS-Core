# ADR 0029: WebSocket on admin routes

- Status: accepted (0.4, W2b)
- Date: 2026-10-18

## Context

Panel pages need two-way channels: terminals (a PTY, `docker exec`) and
command consoles. SSE (ADR 0027) is read-only. A browser `WebSocket` cannot
send an `Authorization` header or a CSRF header, and cookies with
`SameSite=Lax` still reach it from same-site sibling origins. Sockets are
long-lived and carry input: they must not outlive a sign-out or a lost
permission, must not let a client exhaust memory or hold the server, and must
not run commands for a user who may no longer run them.

## Decision

- **Declaration:** `AdminRoutes::socket(path, AdminAccess, handler)`.
  - GET only; `HEAD` and other methods are 405.
  - The handler takes `LiveSocket` and returns
    `live.accept(|socket| ..).target(check)`.
  - An error before `accept` is an ordinary JSON error; no upgrade happens.
  - HTTP/1.1 only: axum's `http2` feature stays off, so there is no extended
    CONNECT.
- **Admission** runs in the route layer, before any extractor, in this order:
  1. The route's access (401, 403).
  2. `WebSocketUpgrade` is extracted once. Its rejections stand: 400, 405,
     426.
  3. The CSRF subprotocol (403). Every `Sec-WebSocket-Protocol` header and
     token is parsed. The list must be exactly `rustcms.v1` and
     `rustcms.csrf.<the session's token>`, compared in constant time. Only
     `rustcms.v1` is selected, so the token is never echoed.
  4. A mandatory `Origin` (403): exactly one, equal to an allowed origin,
     not `null`. `Sec-Fetch-Site`, when sent, must be `same-origin`. There is
     no `Referer` fallback.
  5. The server is not stopping (503).
  6. The open rate limits and a place in the pool shared with SSE (429).

  A refused handshake takes no place.
- **No tickets:**
  - The threat is a browser opening a socket from another origin (CSWSH).
    Pages cannot forge `Origin`, and the exact match refuses same-site
    siblings. A non-browser client can forge `Origin`, but it also needs the
    session cookie and its token: it already holds the session.
  - Accepted losses: the token is reusable for the session and bound to no
    route. A ticket would be single-use, short-lived and route-bound.
  - Mitigations: TLS in production, and redacting `Sec-WebSocket-Protocol` in
    proxy and APM logs. The framework never logs it.
  - A socket is bound to the exact stored session id and never follows a
    reissued session.
- **Supervision before the `101`:**
  - The response first spawns a supervisor in `Shutdown`. Stopping gives 503.
  - The supervisor owns the permits, then hands axum's upgrade task a oneshot
    to deliver the socket through.
  - It waits 10 s at most. A late socket is dropped.
  - The session closure, the reader and the writer are futures polled by the
    supervisor itself. Ending or aborting the supervisor drops them all
    synchronously, and the permits go with it.
- **Running:**
  - The first check, before the closure runs: the stored session and its
    expiry, the active user and its credential version, a fresh principal,
    the access, then the target.
  - Rechecks every `socket_revalidate_secs` (10), spread out, each bounded by
    5 s and independent of the network.
  - `AdminSocket::authorize()` runs the same check on demand and returns the
    fresh principal. A failure ends the socket.
  - **Reads:** a reader always polls, so pongs and close frames are seen even
    when the session never calls `recv`. Data goes to a bounded queue (64
    messages, 1 MiB); overflow is `lagging`. Messages and frames are capped
    at `max_message_bytes`, and a larger message gets 1009.
  - **Writes:** a bounded queue (64 messages, 1 MiB including the message in
    flight). `send` waits at most `send_timeout_secs` for room, and each
    network write has the same deadline. A larger outgoing message is
    `SendError::TooLarge`. Tungstenite's write buffers are finite.
  - **Keepalive:** one outstanding ping with a counter, 15 s after the last
    answer. Its matching pong must come within 45 s.
  - **Ending:** stop the closure (dropped), discard the queued output, send
    the close frame (best effort, 2 s), drop the connection.
- **Close codes:**

  | Code | Reason |
  |---|---|
  | 4401 | `signed_out` |
  | 4403 | `forbidden` |
  | 1001 | `shutdown` |
  | 4008 | `lagging` |
  | 1009 | `too_big` |
  | 1011 | `error` (also a closure that panics or a failed check) |
  | 1000 | the closure returned |
- **Revocation bound:** application work stops within
  `socket_revalidate_secs + 5 s` (15 s by default) of:
  - a sign-out, or an expired session;
  - a password change;
  - a disabled or deleted user;
  - a lost permission or target.

  Transport cleanup takes up to 2 s more.
- **Commands:** structured commands call `authorize()` right before they run
  and check their own permission on the principal it returns. A raw PTY is
  one capability: the target check authorizes terminal input, and the
  periodic check bounds it. It cannot authorize individual shell commands or
  undo those that ran. This is a deliberate exception to "authorize every
  command".
- **Sessions:** live traffic never renews the admin session. It is persisted
  at the handshake, as by any admin response.
- **SPA:**
  - `ctx.realtime.socket(path, handlers, { reconnect })` connects to the same
    origin with `["rustcms.v1", "rustcms.csrf.<token>"]`.
  - It is bound to the session epoch.
  - It does not reconnect by default (a new socket is a new session on the
    server).
  - With `reconnect`:
    - jittered backoff, and a refreshed token each time;
    - failed and short (under 30 s) connections count toward 5;
    - a refused handshake (1006 before open) probes the session once;
    - it stops on 4401 and 4403.
  - `send` refuses messages that are too large, or that would queue past
    1 MiB in the browser. Input is never replayed.
- **CSP (W6):**
  - The panel's policy is built at boot from the constant.
  - `connect-src` also names `ws(s)://` of each panel origin, for browsers
    whose `'self'` does not cover sockets. Nothing else is added.
  - The xterm.js style shim stays in the app (see the plan's W6 spike).

## Consequences

- Terminals and consoles get a supported path that mirrors SSE: the same
  pool, rate limits and checks.
- Tests need a real listener: `TestApp::listen` and `TestClient::socket`.
- Delivering the close code is best effort. A connection with unread input
  may be reset before the close frame arrives. Clients must treat 1006 like
  an unknown end.
- Limits are per instance, like SSE.
