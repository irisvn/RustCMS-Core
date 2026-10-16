# ADR 0024: Dashboard widgets

- Status: accepted (0.3, M22)
- Date: 2026-10-25

## Context

An LMS home page needs counts and trends without leaking anything the user
could not list: scope, per-record policies, field rules and the trash must
apply as on the list, on three databases, at bounded cost.

## Decision

- Built-in widgets only (`stat`, `chart`, `recent`) over a registered
  resource; free-form content stays in custom pages (embedding pages in the
  dashboard was cut: page data is trusted application code).
- Admission before any query: `dashboard.<name>`, the resource's `view_any`
  (`admit_list`), `can_read` of every field used (filters, date, columns,
  sort); else hidden and 403. Static checks at boot (declared filters and
  their values, date kind, fields, sortable sort, sizes).
- Counting (`DynResource::counted`): a policy deciding once
  (`!view_depends_on_record`, no trash) → `COUNT` through sea-orm's paginator
  (the list query as a subquery: joins and `DISTINCT` hold), one count per
  range with bounds bound in the column's own kind; a per-record policy, or
  ranges on SQLite (text dates: lexical comparisons are wrong at bucket
  edges) → candidate keys within `MAX_LIST_SCAN`, each loaded and checked
  (`listable`), dates read back from the wire value; beyond → `too_many`.
- Buckets in UTC (day, ISO week, month), computed in Rust, contiguous.
- Recent records through `DynResource::list` (envelopes: field rules and
  labels); the list's scan limit is recognized by its issue code
  (`too_many_matches`).
- No cache; 60 widget requests per user per minute; 4 computations at once
  per process, a 10 s wait, then 429.
- SPA: SVG `<rect>`s with Tailwind fill classes (no inline style), a
  screen-reader table, counts formatted with `BigInt`.

## Consequences

- Charts over policies deciding per record, or on SQLite, stop at 10,000
  candidate rows.
- Only counts (no sums); UTC only.
