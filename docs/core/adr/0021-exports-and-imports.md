# 0021 — CSV exports (and imports)

- Status: accepted (exports, M17a); imports to follow (M17b)
- Date: 2026-10-22

## Context

LMS staff need lists as files (enrollments, learners). A file escapes every
check the panel makes per request: it is kept, re-read and forwarded. Large
lists cannot be built in a request, and a job runs later, as someone whose
access may have changed.

## Decision

- **What**: the list engine's own query (scope, filters, search, sort,
  trash, per-record policy within `MAX_LIST_SCAN`), readable fields only, in
  `fields()` order; values in the wire format; text that spreadsheets run as
  formulas gets a leading `'` (text and JSON columns only).
- **Who**: ability `export` (opt-in `ResourceMeta::exportable`) plus
  `view_any`, admitted before the query is read.
- **At once**: ≤ 1 000 rows, built in full before the response starts (a
  limit is a 422, never a truncated file).
- **Background**: a row in `exports` (user id, credential version, canonical
  query, fixed columns, object key named up front) and a job in the same
  transaction. The job reloads the user per batch (active, same
  `auth_version`, current grants through the resolver), refuses rather than
  narrows when a fixed column is no longer readable, keeps the keys it wrote.
  Download: owner only (404 otherwise), same credential version, `export`,
  fields, and every kept key still viewable through the query; else 409
  `export_stale`. Files expire after 24 h; pruning deletes rows and objects,
  and fails exports whose job never finished.
- **Audit**: `exported` with the row count and the query's shape only.

## Consequences

- Revocation stops a running export at the next batch, and any download.
- A file reflects records as the job read them (no snapshot across batches).
- Imports reuse the per-batch reload and the engine's write paths (M17b).
