# ADR 0041: The lock contract of relationship repeaters (and media last)

Status: accepted (0.13.0). Design reviews: `docs/core/plans/contrib-0.13-d2.md` (v1–v4, four Codex rounds).
Amended by ADR 0044 in 0.14.0 (points 4 and 6: several repeaters on one child resource, through filtered
relations, each with its own locking pass; point 5: media, rich text and `belongs_to` fields in rows, their media
checked with the parent's, last; point 6 and Consequences: the 1,000-row bound is `[admin] repeater_rows`, default
50, at most 500).

## Context

A relationship repeater (ADR 0040, part 2) writes the child records of a has-many relation inside the parent's
write: the parent row, its links, then rows of another resource. Every other writer of those rows (the child's own
pages, the API, moves between parents) locks them too. Two designs that tried to order **every** lock a write could
take (including the database's foreign-key locks, cascades and hook writes) were rejected: no RustCMS write orders
those today, and making one would mean a lock planner for the whole write path.

What Core already guarantees:

- **Explicit locks** (`lock_parent`, `lock_visible`, link targets, media reference checks) follow
  `Registry::lock_rank` (ADR 0018, 0020, 0035).
- **Implicit locks** (foreign-key enforcement on INSERT/UPDATE, cascades, hook writes) are not ordered on any
  path. A cycle among them is detected by the database; `From<DbErr> for AppError` answers it, as lock-wait
  timeouts, serialization failures and a busy SQLite database, with **409 `write_conflict`**: the operation is
  aborted, its transaction rolled back, nothing applied, retryable.

## Decision

Relationship repeaters follow that same contract; it is stated as the contract of every write:

1. **New explicit lock acquisitions** follow `lock_rank`, keys in key order within a resource, never back to a
   lower rank. Re-entering a lock already held (a row write under its locked parent) is not a new acquisition.
2. **Media rows are locked last.** `lock_rank` gives the media library a rank after every resource; create and
   update check media references (`media::check_references`, shared locks) after every other explicit lock:
   after the links, and after a parent's rows. Boot refuses `lock_parent` targeting the media library.
3. **A repeater's family serializes on the parent row.** The child must declare `lock_parent` through the
   repeater's relation (boot): every write of a child — direct create, update that moves it, delete, restore,
   repeater row — locks the parent first, as parent writes do.
4. **Phases in rank order.** The lock graph gains, for each relationship repeater (P, C) and each
   belongs-to-many target T of P, an edge T → C; a parent write then locks, in rank order, its parents, its row,
   its link targets (before the row write, as today), the rows of each child resource (one phase per child
   resource, in rank order; two repeaters on one child resource are refused), then media.
5. **A row takes no explicit lock but its own.** Row inputs may not be media, rich-text, encrypted,
   belongs-to-many or `belongs_to` fields (boot, from the child's field metadata); a row whose data holds a media
   or rich-text field referencing media after the child's hooks is refused (500 naming the resource and the field: only a hook can put it there). A parent with relationship
   repeaters may not link media records.
6. **One locking pass per child phase.** The named rows (pre-filtered by parent membership) and, with an order,
   the parent's live children (at most 1,000, discovered before any lock; beyond: 422 `too_many_rows`) are locked
   once by `locked_visible` (scope, view policy, key order, recheck under the lock). Rows not visible are not
   locked and keep their positions.
7. **Implicit locks** stay outside, behind 409 `write_conflict`. The guide asks that hooks of a repeater's child
   do not write other resources.

Also part of the repeater's write (ADR 0040 amended): positions by `MAX` over every child (trash included),
typed by the order column (`i32`, `i64`), checked arithmetic (422 `positions_exhausted`); new rows tagged in the
order (`{"new": ref}`); the parent's history records counts only (child rows are in the child's history);
`addable(false)`/`deletable(false)` enforced by the server; row inputs inherit the child form's options and bounds
(boot refuses their own); new rows on an edit page use the child's create inputs (`createComponents`, vocabulary
16).

## Consequences

- `min_items`/`max_items` hold for saves through the repeater; direct child writes are not bounded by it.
- Children of a relationship repeater pay one parent-row lock per write (already the case with `lock_parent`).
- Reordering a parent with more than 1,000 live children is refused (edit them in the relation section).
- Media checks moving after links and rows changes no result, only the order of locks (a failing check still
  rolls the whole write back).
