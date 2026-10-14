# 0014 — Audit log

- Status: accepted
- Date: 2026-10-17

## Context

0.3 targets internal back-offices (an LMS admin) that must answer "who changed
this, when, from where". The log must not lie: an entry for a change that
rolled back, or a committed change without its entry, defeats the purpose. It
must also not become a second copy of secrets or personal data, and must never
make a valid write fail.

## Decision

- **One table, written by the framework** (`audit_logs`, framework migration,
  no foreign key to `users` so entries outlive their actor). UUID v7 ids;
  indexes for a subject's history, an actor's history, and pruning by time.
- **Data changes are recorded in the transaction of the change**
  (`audit::record(txn, ctx, entry)`): the admin engine (every write path,
  actions in batches of 50 rows after one batch reload by primary key),
  uploads (with the receipt and the claim), every `RbacService` method, token
  issuance and revocation, console commands. An entry that fails to insert
  fails the write. Writes that change nothing record nothing.
- **Sign-ins, sign-outs and JWT events are best effort**
  (`audit::record_detached`): recorded on their own once the session is
  persisted (the guard takes an `AfterSession` from the response), a failure
  is a warning. A replayed refresh token ends its family first; the
  `jwt.reuse_detected` entry (as a guest) follows the commit, once, so a
  failing log can never keep stolen tokens alive.
- **Who and where** travel in `AuditContext { actor, request }`. The actor is
  a user (id, guard, credential: `session`, `token:<id>`, `jwt:<family>`),
  the system (`console:<command>`), or a guest. `Principal.credential` carries
  the credential from the guards. `RequestMeta` holds the client IP after
  trusted proxies, the user agent (cut) and the request id, which is the
  client's only when it is a short plain token sent once (otherwise
  generated), so it cannot inject into logs or forge correlation.
- **Changes are a diff of declared fields** (`{field: [old, new]}`), taken
  from the locked record before the hooks and the record re-read by primary
  key after `after_save`, so hooks are reflected and leaving the scope does
  not hide the change. Redaction depends on the declaration only (never on
  the user): `.audit_redact()`, a `readable` rule, `guarded`, password inputs.
  Rich text is stored as size and digest. Values are bounded (text over 1,000
  characters → length and digest; depth, list length; 64 KiB per entry for
  changes and meta together). A primary key cannot be redacted (boot fails);
  a relation's parent key follows its foreign key's redaction; identifiers
  over the column width keep their start and a digest, and slugs (64) and
  action names (57) are checked at registration.
- **Outcome after hooks**: the record is re-read after `after_save`; if the
  hooks deleted or trashed it, a `deleted` / `trashed` entry follows.
  Membership changes record whole sets before and after.
- **Meta is typed per event** (a struct per event, never request data), so no
  secret can be forwarded by accident. Failed sign-ins have one shape for
  every reason; beyond a per-minute cap (per instance) they are summarized.
- **Retention** by age (`audit.retention_days`, 1–36,500, checked at boot), pruned hourly by the auth
  prune job and by `audit:prune`, oldest first in bounded batches, safe on
  several instances at once.
- Applications record their own events with `ctx.audit` (hooks, actions) or
  `audit::record` (their routes), in their transaction.

## Viewer (M14b)

- Row-level filtering of the whole log was rejected: skipping rows after a
  query leaks through empty pages, cursors and timing, and resources' policies
  cannot be evaluated for deleted records or removed resources. Instead:
  - the full log is for auditors (`audit.view_all`), filtered only in SQL
    (`auth.*`, `jwt.*`, `token.*` need `audit.view_auth`, which also shows
    addresses);
  - a record's history (`audit.view` + `view` on the record, checked as on
    its page) lists only engine events, by an SQL allowlist, projected to the
    fields the viewer may read; meta keeps only an action's result.
- Credentials are shown by kind; actor names need `users.view_any`.
  Framework subject names are reserved resource slugs.

## Consequences

- Every engine write does one more read (the reload) and one insert; an
  action over N records does one batch read and ⌈N/50⌉ inserts.
- What hooks and actions write beyond the records they were given is not
  recorded automatically; the guide says so.
- Auth events can be lost on a crash between the session write and the
  entry; data changes cannot.
- The log is append-only for the application, not tamper-proof against
  database writers. Shipping entries elsewhere (outbox) comes later.
- Tests that build tables from entities need `audit_logs` too.
