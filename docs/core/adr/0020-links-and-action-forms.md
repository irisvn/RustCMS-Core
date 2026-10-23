# 0020 — Belongs-to-many links and action forms

- Status: accepted
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
  hidden links) is deferred.
- Self relations and inverse managers over one pivot need another lock order
  first; refused at boot until then.
