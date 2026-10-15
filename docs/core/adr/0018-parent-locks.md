# 0018 — Parent locks

- Status: accepted
- Date: 2026-10-20

## Context

M15c (LMS) needs a rule over a parent's children: at most `capacity` active
enrollments per course. A hook that locked the course itself ran after the
engine had locked the enrollment (child, then parent), the reverse of a
course deletion cascading to enrollments (parent, then child): concurrent
writes could deadlock. Scopes read through the parent (the course's
instructor) could also change between the scope check and the write.

## Decision

`ResourceMeta::lock_parent(field, target)`: before locking any record of the
resource, the engine locks the parent rows its write touches, through the
parent resource (`DynResource::lock_keys`: active or trashed, key order,
MySQL point locks, SQLite already serialized by `BEGIN IMMEDIATE`).

- Order: `xact_lock` (idempotency, first if any) → parents (whole batch,
  deduplicated, key order) → records.
- Effective values: create locks the parent from the accepted input (relation
  binding included); update locks the current parent and the one the input
  moves to; actions lock the parents of every candidate. After hooks or the
  action handler, a parent outside the locked set is an internal error
  (rollback). A record whose parent changed between reading and locking is
  a 409 `write_conflict` (`busy`), never retried by the server.
- One parent per resource; the field must be a `belongs_to` the target; the
  target may not declare its own parent lock (no chains). Checked at boot.

## Consequences

- Writes under one parent are serialized; different parents run in parallel.
- Scope and policy are re-checked after the record lock, now under the parent
  lock too: ownership through the parent cannot change mid-write.
- Hooks must not take an `xact_lock` (it would no longer be first).
