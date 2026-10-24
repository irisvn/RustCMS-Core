# Review checklist

Distilled from the Codex reviews since M3. Check every change against it before
review; reviewers use it as their dimensions.

## Authorization and disclosure
- Authorize **before** reading anything about the target (existence, current
  state): unauthorized callers get the same 403 whatever the input.
- Responses of writes contain only what the caller may read (204 otherwise).
- Field rules, `scope()` and policies apply on every path: lists, options,
  relationships, actions, write responses, schema defaults.
- Replacing someone's credentials (password, email) is a takeover: super admin
  of every guard only. Equal permissions are not equal access.
- Own credential changes need reauthentication (current password).
- No escalation: grant/remove only what the actor holds; never on oneself;
  protected roles by super admins; at least one active super admin.
- Bulk selections (matrix rows and columns, groups, presets) change only
  grantable permissions, and toggles only those shown (search, open areas).
- History rows (`job_runs`, schedule outcomes) are written only in the
  transaction of a transition that changed the job; texts shown to viewers
  (descriptions, summaries, errors) are safe messages, never error text.
- Per-record action predicates (`visible_when`) show in the envelope's
  `abilities`, and are decided again on each record locked in the action's
  transaction (one record: 403; several: `denied`, never passed to the handler).
- Action outcomes: `open` only for a run on one record (else 422
  `one_record_only`), `https://…` or a path of the site; `message` ≤ 500
  characters; both checked before the commit, dropped on replay unless the user
  may still view every processed record.
- Runtime values reaching CSS or images go through the theme's checks
  (tokens, hex colors, PNG/WebP/ICO by magic bytes); never SVG or app CSS.

## Concurrency
- Re-check authorization and state **under the lock** in the same transaction
  (RBAC lock, row locks); use only rows actually locked.
- Replacement writes (role sets, permission sets, profiles) carry the values the
  editor started from (`expected…`) and are refused when stale.
- The caller's own credentials can be revoked mid-request: re-check
  `auth_version` under lock.
- Migrations that rebuild tables lock them and keep id high-water marks.
- Writes locking rows of several resources follow one order:
  `Registry::lock_rank` across resources, key order within one
  (`lock_visible` / `lock_visible_with`); never a lock outside that order.

## Data integrity and wire format
- i64/decimal as strings; JSON numbers beyond ±2^53 or non-finite never written back.
- `null` vs `""` vs omitted follow nullability and declared defaults; untouched
  defaults go out exactly as declared; JSON defaults left to the database.
- Instants edited in UTC; date-range "until" is `lt` next day start.
- Count characters as code points, like Rust `chars()`.
- Rich text stays bounded (`MAX_BYTES`, nodes, depth, images, embeds, table
  size), saved in the lowest format version it needs (version 1 still read);
  embeds are a provider and an id (never a URL, HTML or iframe); editor nodes
  render no `style` attribute (CSP).

## Session, CSRF and client lifecycle
- Every response carries its user (`X-Rustcms-User`); a different user resets the tab.
- Answers to requests from an earlier session epoch never trigger auth handling.
- Credential rotation (password change, logout-others) starts a new epoch.
- Writes are serialized per resource; queued writes die with their epoch.
- Unknown write outcomes (5xx, 408, cut body, network) pause the resource until reload.

## Limits and DoS
- Costly work (hashing, scans) after authorization and behind a per-actor limit.
- Inputs bounded (lengths, list sizes, pages); errors are 422/413/429, never 500.

## UI state
- Drafts survive refetches, failed saves and background errors; forms keyed by identity.
- Completion handlers act only if their page is still mounted. Exception: toasts (`props.toast`)
  may outlive their view within the same session epoch; every toast is dropped on an epoch change.
- Stale rows (placeholder data) are not actionable; selections follow the shown rows.
- After a write, invalidate lists, records, schemas and options.

## Contract and build artifacts
- Contract changes bump `VOCABULARY_VERSION` (components, columns, filters) or
  `SCHEMA_VERSION` (breaking shape), and update `admin-protocol.md`.
- Regenerate `fixtures/contract/*.json` (`UPDATE_CONTRACT=1`) and
  `admin-ui/src/api/types.gen.ts`; review the diff.
- A new rich text node or attribute bumps the document format version.
- Built admin assets (`crates/rustcms-admin/assets`, the demo's
  `resources/admin/{js,css}`) are committed, and the SDK copies match
  `admin-ui/sdk`: `just generated` passes.

## Process
- Every fix gets a regression test and a reverse check (revert → test fails).
- Reviews are exhaustive and batched: list all findings per dimension, fix in one pass, verify once.
