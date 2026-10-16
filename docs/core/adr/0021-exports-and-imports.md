# 0021 — CSV exports (and imports)

- Status: accepted (exports M17a, imports M17b)
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
- **Runs**: each run writes its own object (a fresh key, recorded in
  `export_objects` before the upload, so a run cancelled at shutdown and
  still copying never shares a path with the next) and publishes it with
  its key list in one conditional update; losers delete theirs; pruning
  deletes every recorded object and the published one. Fields are
  re-checked at each batch; byte budgets (10 MiB at once, 50 MiB in the
  background); `Idempotency-Key` on creation; per-user limits counted under
  a named lock; failure reasons are codes.
- **Imports**: `importable` / `import_key` (a field with its own declared
  unique constraint). Upload admitted before the body, parsed within limits,
  stored before its row and receipt are committed (`Idempotency-Key`: the
  same file answers with the same import; a refused or failed commit deletes
  the object);
  at most four files parsed at once (429 beyond), cells bounded, header
  included; preview = static checks only; run once (conditional status
  update + dispatch in one transaction; the import is then kept 24 h from
  the start). The job reloads the user per batch and writes each row through
  `DynResource::create` / `update` (`Ctx::import_row`): the row's transaction
  holds the RBAC lock (`begin_as`: the user as they are then, `import`
  admitted again), checks the import is still running and unexpired (held,
  so pruning waits), and commits a receipt `import_rows(import, row,
  outcome, key)` with the write, after checking `create` (or the record's
  `update`) for the user as they are then; `write_conflict` (lock waits,
  deadlocks, a busy database) retries the job, never fails the row. A later run skips rows with a receipt and
  counts them as recorded (never turning a created row into an update), and
  spends none of the user's idempotency quota. Abandonment is judged from
  the start time; an expired import is never written. Reports keep row,
  field and codes whose parameters are numbers; any other message (a hook's,
  one quoting a value) becomes `is invalid`. A quoted empty cell (`""`) of a
  text field is the empty text, an empty cell `null`, as exports write them.

## Consequences

- Revocation stops a running export at the next batch, and any download.
- A file reflects records as the job read them (no snapshot across batches).
- Imports reuse the per-batch reload and the engine's write paths (M17b).
