# 0009 — Has-many relations and extension widgets

- Status: accepted
- Date: 2026-10-14

## Context

0.2 adds a relationship manager (a parent's children, on its page) and custom
field types. Both widen what the panel does without widening what a user may
do: every invariant of 0.1 (deny by default, scope and policy on every path,
field rules, `admits` before key lookups) and the strict CSP (ADR 0006) must
hold.

## Decision

- **A relation is the target's own list and create** with one internal
  condition, `fk = parent key`. Record-independent admission (`admits(view)` on
  the parent, `view_any` or `create` on the target) runs before the parent is
  looked up; a parent the user cannot see is a 404. The foreign key is injected
  into the input before acceptance: no rule is skipped, a client value is
  refused, and a hook changing it aborts the write.
- **No parent lock.** A relation create is one transaction of the target. A
  parent trashed concurrently is the same boundary as any `belongs_to` check
  (the check sees the parent when it runs), which avoids lock upgrades and
  ordering issues with idempotency claims.
- **Idempotency claims are bound to the relation and the parent** and replayed
  before the parent is resolved.
- **Widgets come from extension scripts**, trusted code embedded in the binary
  by the application, served same-origin under content-hashed URLs, so the CSP
  (`script-src 'self'`) is unchanged. There is no isolation between
  extensions: they have the panel's privileges.
- **The widget contract is framework-agnostic** (`mount`/`update`/`unmount`,
  API version 1). The panel owns the value as wire JSON; widgets only report
  changes. A missing or failing widget makes its field read-only and never
  submitted.
- **Many-to-many is deferred** after 0.2: attach/detach needs its own pivot
  authorization and locking model.

## Consequences

- Applications get custom inputs without rebuilding the SPA, at the price of
  writing them without React and inside the CSP.
- The server keeps validating custom values by the field's kind and rules; a
  widget can never write what the field does not accept.
- Breaking changes to the widget contract need a new `apiVersion`.
