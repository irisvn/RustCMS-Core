# 0007 — Idempotency keys for creates and actions

- Status: accepted
- Date: 2026-10-13

## Context

A write whose response is lost (timeout, cut connection) has an unknown
outcome. In 0.1 the panel paused writes until a reload, because resubmitting a
create could duplicate it. 0.2 lets clients retry safely.

## Decision

- Only resource **creates** and **actions** take an `Idempotency-Key`: they are
  the writes whose repetition duplicates. Updates and deletes converge when
  repeated; system creates are guarded by unique email/name; token issuance and
  credential changes rotate secrets and stay pause-only.
- The key is claimed **inside the write's transaction**, before hooks run, with
  `INSERT … ON CONFLICT DO NOTHING` into `idempotency_keys`; the result (the
  created key, or the action outcome) is stored before commit. Claim and write
  commit together or not at all; a failed write releases the key.
- A concurrent request with the same key waits on the claim (PostgreSQL: the
  unique index, bounded by `lock_timeout`; SQLite: the write lock) and then
  replays the committed receipt, or answers 409 `write_conflict` if the wait
  times out.
- Receipts store no request bodies and no response bodies: only a hash of the
  canonical request and the result reference. A receipt is looked up **before**
  validating the request (field permissions may have changed since). A create
  is **re-rendered** for the user at replay time (current scope and
  permissions); admission is checked again before replaying.
- Action outcomes (first response and replay) list processed and denied keys
  only for records the user may view now; hidden keys are left out, never moved
  to `missing`.
- A claim first takes the user's named lock (ADR 0010; the transaction's first
  lock), then resolves an existing receipt, then applies the per-user quota: a
  retry is never refused by it, even racing the request that took the last key,
  and an expired, unpruned receipt is replaced once (deleted only while still
  expired). Pruning deletes in batches.
- Keys are scoped to (guard, user, resource, operation); per-user cap of 1,000
  live keys; 24-hour lifetime, pruned hourly.
- The SPA keeps the pending request (method, path, exact body, key, user, the
  form that sent it) in memory only. A new session (sign-in, sign-out, 401,
  another account) drops every pause and pending request; an unknown outcome
  answered after such a change pauses nothing. A pending request belongs to the
  user, not the credential epoch: after the same user's credential rotation it
  may still be re-sent (the server scopes keys by user). Retry goes through the
  resource's write queue, refreshes the CSRF token once if refused, and lifts
  the pause only after the answer is handled (queries refreshed, the
  originating create form navigated) — other pages show a notice.

## Consequences

- No generic middleware: routes outside the admin resource engine are not
  covered (an API can add its own keyed handling).
- A natural key reused after a force delete can make a replayed create show the
  newer record (still filtered by the user's current access).
- A write in flight still finishes with the principal it authenticated with
  (0.1 boundary, unchanged).
