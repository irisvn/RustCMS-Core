# 0020 — Belongs-to-many links and action forms

- Status: accepted; amended in 0.9.0 (`after_relation_change`, ADR 0035) and 0.10.1 (below)
- Date: 2026-10-22

## Context

0.3 (M16) needs many-to-many relations (tags of a post) with columns on each
link, actions that ask for values (enroll a learner in a course), and a role
page that reads as resource × ability. Each new write path must keep the
engine's invariants: admission before anything is read (403 before 422/404),
scope and policy on the parent and every target, undeclared columns never
exposed, one lock order across writers (ADR 0018), and SQLite, PostgreSQL and
MySQL alike.

## Decision

**Belongs-to-many.** `Relation::belongs_to_many(name, target, parent, related)`
over a pivot entity (typed columns; a `PivotAdapter<P>` erased to `DynPivot`),
with `.pivot(fields)`: pivot columns are `Field`s (read and write rules, wire
format). Lists reuse the target's list engine with a hidden
`ListQuery::through` (`pk IN (SELECT related FROM pivot WHERE parent = $key)`)
and add readable pivot columns by target key. Writes:

- abilities of the parent: `attach_{rel}`, `detach_{rel}`, `update_pivot_{rel}`
  (declared for `permission:sync`); handlers admit before reading the body
  (raw body, `json_body`);
- then static checks (1–100 canonical keys, pivot input by field rules), then
  one transaction: parent `find_authorized_locked` (scope, policy, its own
  `lock_parent`), targets `lock_visible` (scope + view policy, locked in key
  order, re-read and re-checked under the lock: only surviving keys are
  written), then pivot rows read and inserted or deleted by both keys;
- one lock order: parent ancestors → parent → targets → pivot rows. Boot
  refuses what would need another (self relations, one pivot used by two
  relations, a target with `lock_parent`, and any cycle in the graph of
  belongs-to-many parent → target and `lock_parent` parent → child edges). Every pivot write of a parent holds
  its lock, so links are read then inserted (no `ON DUPLICATE`, no gap locks
  on MySQL); foreign-key failures map to a form error;
- attach of an existing link keeps it (convergent), detach touches only
  targets the user may view; no `Idempotency-Key`: an unknown outcome pauses
  the resource until reload (SPA `guardedWrite` under the parent slug);
- audit on the parent: `attached` / `detached` / `pivot_updated` with
  `{relation, targets, pivot}` (pivot values redacted by declaration). The
  history shows targets and pivot values (those readable now) only to readers
  who may list the target, else `{relation, count}`; pivot updates that change
  nothing are not written nor recorded.
- `Resource::after_relation_change` (added in 0.9.0): the parent's hook,
  in the write's transaction after the pivot rows and the audit entry, with
  the parent model and a `RelationChange` of the targets that really changed;
  writes that change nothing call nothing. The parent row is not updated.

**Action forms.** `Action::form(vec![ActionField::…])` (text, textarea,
integer, boolean, date, `datetime` (an instant, edited in UTC; added
later), select, `belongs_to(field, target)`). Order:
admission (`requires_permission`, `admits`) before the body; static checks
and canonical values (every field present, defaults, re-encoded by kind,
canonical keys) before the idempotency claim, so equivalent requests share
it; `belongs_to` values checked in the action's transaction (not on a
replay). Actions without a form refuse `data`. `ActionSchema.form` and a
dedicated options route; `SCHEMA_VERSION` 2 so older SPAs never run a form
they do not understand.

**Permission matrix.** `PermissionInfo.resource` / `ability` come from how
resources use each permission (`DynResource::permission_uses`: abilities and
explicit admission permissions), set only when a permission has exactly one
use, an ability, named `{slug}.{ability}`. The SPA draws the matrix; writes
are unchanged (`RbacService::set_role_permissions_if`, expected permissions,
symmetric difference grantable).

## Consequences

- Custom policies deciding per record admit everything by default: link and
  action abilities that need a permission should be listed in `admits`
  (documented).
- Multi-select of links in the record form (replacing a set, stale checks,
  hidden links) is deferred (done in 0.9.0 as a change, not a set: ADR 0035).
- Self relations and inverse managers over one pivot need another lock order
  first; refused at boot until then.

## Amendment (0.10.1): per-record actions and outcomes

- `Action::visible_when(|ctx, record| bool)` decides per record whether an
  action is offered. It is part of the record's `abilities` (with the
  policy) and decided again on the locked record when the action runs: one
  hidden record is 403 and nothing runs; hidden records of a run on several
  are `denied`, never passed to the handler. `primary()` marks the labelled
  row action (others under ⋯), `destructive()` sorts last.
- `Action::with_outcome` handlers return an `ActionOutcome` with an optional
  `message` (≤ 500 characters, shown as text) and `open` (a page to open in
  a new tab). `open` is checked before the commit: `https://` without
  credentials (DNS name, IPv4, bracketed IPv6, port 1–65535) or an absolute
  path of the site (no `//`, dot segments, backslashes, whitespace), at most
  2,048 bytes; anything else is an application error (500, rolled back). A
  run on several records may not return one (422 `one_record_only`, decided
  after the handler, documented). The panel opens it after the commit with
  `noopener` and `noreferrer`, checking it again. Outcomes are kept in the
  idempotency receipt and left out of any answer when the user may no
  longer view every processed record.
