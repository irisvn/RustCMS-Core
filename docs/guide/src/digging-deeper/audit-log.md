# Audit log

RustCMS records who changed what, and from where, in the `audit_logs` table
(created by `rustcms migrate`). It needs no setup. The panel shows it to
auditors, and each record's history to those who may view the record
([Viewing the log](#viewing-the-log)).

## What is recorded

| Source | Events | Subject |
|---|---|---|
| Resources (admin panel) | `created`, `updated`, `trashed`, `deleted`, `restored`, `force_deleted`, `action:<name>` | the resource slug and record key |
| Belongs-to-many relations | `attached`, `detached`, `pivot_updated` (meta `{relation, targets, pivot?}`) | the parent's resource slug and key |
| Import and export | `imported` (meta `{import, rows, created, updated, failed}`), `exported` (meta `{rows, query}`: the query's shape, never its values) | the resource slug |
| Media library | `uploaded` (meta `{mime, size}`); a rename or new alt text is `updated` | `media`, the media id |
| Roles | `role.created`, `role.permissions_changed`, `role.deleted` | `role`, its id |
| Users | `user.created`, `user.invited`, `user.invitation_resent`, `user.updated`, `user.deleted`, `user.activated`, `user.deactivated`, `user.roles_changed`, `user.permissions_changed`, `user.password_reset` | `user`, its id |
| Permissions | `permission.synced` (meta `{created, orphaned, restored, regrouped}`) | `permission` |
| Settings | `settings.updated` (secrets as `"[redacted]"`); `mail.test_sent` (meta `{to, ok, code}`, the address masked) | `settings`, the group (none for `mail.test_sent`) |
| Queue and schedules | `queue.retried`, `queue.forgotten`, `queue.cancelled` (meta `{kind, queue}`); `schedule.run` (meta `{run}`) | `job`, its id; `schedule`, its name |
| Account | `auth.password_changed`, `auth.password_reset`, `auth.invitation_accepted`, `auth.signed_out_everywhere`, `token.created`, `token.revoked` | `user`, its id |
| Sign-ins | `auth.signed_in`, `auth.signed_out`, `auth.sign_in_failed`, `auth.sign_in_failed_burst`, `jwt.signed_in`, `jwt.signed_out`, `jwt.reuse_detected` | `user` (`session` for failures) |

A change that changes nothing (saving an unchanged form, assigning a role the
user already has) records nothing. Actions are the exception: an action
records `action:<name>` for every record it processed, even when it changed
no field (the entry then has no `changes`). A refused or failed write records nothing.
Role and permission changes record the user's whole set before and after
(`{"roles": [[1], [1, 4]]}`), not only the difference. If a hook deletes the
record it just saved, or moves it to the trash, a `deleted` or `trashed`
entry follows.

Each entry holds:

| Column | |
|---|---|
| `at`, `event`, `subject_type`, `subject_key` | when, what, on which record |
| `actor_kind` | `user`, `system` (console, periodic jobs), `job` (a [queued job](queues.md), on behalf of its initiator) or `guest` (failed sign-ins, a replayed refresh token) |
| `actor_user_id`, `actor_guard` | the acting user (no foreign key: entries outlive users) |
| `credential` | `session`, `token:<id>`, `jwt:<family>`, `console:<command>` or `queue:<kind>` |
| `ip`, `user_agent`, `request_id` | the client address (after [trusted proxies](../getting-started/configuration.md)), its user agent (cut to 255 characters), and the request id |
| `changes` | `{field: [old, new]}` for the fields that changed |
| `meta` | facts specific to the event (an action's result, a relation's parent, …) |

The request id is the client's `X-Request-Id` when it is a plain token (up to
64 of `A-Z a-z 0-9 . _ - :`, sent once); otherwise RustCMS generates one. It is
also returned in the response and written to the logs, so an entry can be
matched with a log line.

## Guarantees

Data changes are recorded **in the transaction of the change**: the entry
commits with it and disappears with it on rollback. If the entry cannot be
written, the change fails too. This covers every write the admin engine makes
(create, update, trash, restore, force delete, creates through a relation,
actions), uploads, settings, every `RbacService` method, API tokens issued or revoked,
and the console commands `make:user`, `role:assign`, `token:create` and
`permission:sync`.

Sign-ins and sign-outs (session and JWT) are not data changes: they are
recorded on their own once the session is saved or the sign-in ended, **best
effort**. A replayed refresh token ends its sign-in first, then records
`jwt.reuse_detected` (once: later replays change nothing). If that write fails it is logged
as a warning and the request still succeeds, and a crash at the wrong moment
can lose the entry.

Not recorded:

- what your hooks and actions write **beyond the records they were given**
  (another table, other records) — record it yourself, see below;
- migrations, seeding, and SQL run outside the application.

Retries are safe: a create or action replayed with the same
[idempotency key](../reference/admin-api.md) records nothing again.

### Actions

An action records one `action:<name>` entry per selected record it
processed, with the changes of the record's declared fields. If the action
moved the record to the trash, the entry's meta is `{"result": "trashed"}`;
if it deleted it, `{"result": "deleted"}` without the former values (as for a
delete).

## What is stored, and what is not

Only declared fields are compared. A field's values are replaced by
`"[redacted]"` (the entry still says it changed) when the field:

- is marked `.audit_redact()`,
- has a `readable(...)` rule, or is `guarded()`,
- is edited with a password input (`TextInput::make(..).password()`).

The decision depends on the declaration only, never on who is acting. The
parent of a record created through a relation (`meta.parent`) is redacted
when its foreign key field is. A primary key cannot be `.audit_redact()`
(boot fails): it names the record in every entry; use an opaque key, or
`.audit(false)`. Keys longer than 255 characters (and keys that look like
such a shortened form) are stored as their start and a digest.

```rust
fn fields() -> Vec<Field> {
    vec![
        Field::column(customer::Column::Name),
        Field::column(customer::Column::TaxId).audit_redact(),
    ]
}
```

Values are bounded so an entry never makes a valid write fail: a text over
1,000 characters is kept as `{truncated, length, sha256}`, a rich text
document as `{document, bytes, sha256}`; lists and objects keep their first
100 items, and values nested deeper than 8 levels are summarized.
An entry holds at most 64 KiB of `changes` and `meta` together (meta at most
16 KiB); beyond, only the names of the changed fields are kept.

RustCMS never stores passwords or their hashes, API tokens, JWTs, refresh
tokens, cookies, CSRF tokens or session ids. Event meta is a fixed set of
fields per event, never free-form request data.

To skip a resource entirely (a log table of your own, a cache):

```rust
fn meta() -> ResourceMeta {
    ResourceMeta::new("Page view").audit(false)
}
```

## Recording your own events

Record what your code changes beyond what the engine recorded, in the same
transaction as the change, so the entry commits or rolls back with it.

### In hooks and actions

Inside a hook or an action, `ctx.audit(txn, entry)` records an entry in the
same transaction, with the same actor and request:

```rust
use rustcms::audit::AuditEntry;

Action::<order::Entity>::new("refund", |ctx, txn, orders, _data| {
    Box::pin(async move {
        for order in orders {
            refund_payment(txn, &order).await?;
            ctx.audit(txn, AuditEntry::new("order.refunded", "orders").key(order.id)).await?;
        }
        Ok(())
    })
})
```

The LMS's *Move up* / *Move down* actions renumber the other lessons of the
course too. The engine records only an `action:<name>` entry for the lesson
the action was given, so the helper records an `updated` entry for every
lesson whose position changes (the selected lesson included, which then has
both), with the action's own context (`ctx.audit_context`, a public field)
and on the lesson resource, so each lesson's **History** shows it:

<<< @/../../../examples/lms/src/admin/resources/lesson_resource.rs#reorder [src/admin/resources/lesson_resource.rs]

### In your own routes

In your own routes, build the context from the signed-in user and the
request (`RequestMeta` is an extractor: client address, user agent, request
id), and record in your transaction:

```rust
use rustcms::audit::{self, AuditActor, AuditContext, AuditEntry, RequestMeta};

async fn close_course(auth: Auth, request: RequestMeta, State(app): State<AppState>) -> AppResult<StatusCode> {
    let txn = rustcms::db::begin_write(app.db()).await?;
    // … the change …
    let context = AuditContext::new(AuditActor::user(auth.principal()), request);
    audit::record(&txn, &context, AuditEntry::new("course.closed", "courses").key(42)).await?;
    txn.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
```

`AuditEntry::changes` takes a `{field: [old, new]}` object
(`audit::diff` builds one from two snapshots) and `AuditEntry::meta` any
serializable struct; both are bounded like the built-in entries. Do not put
secrets in them.

In a [queued job](queues.md), use the job's context:
`audit::record(&txn, &ctx.audit_context(), entry)` records actor `job`, on
behalf of the user who dispatched it.

### Reference

`rustcms::audit`:

| Item | |
|---|---|
| `record(&txn, &context, entry)` | one entry, in the change's transaction |
| `record_all(&txn, &context, entries)` | several entries, inserted in batches |
| `record_detached(&db, &context, entry)` | an event that is not a data change (a sign-in): on its own, best effort; a failure is logged, never returned |
| `AuditEntry::new(event, subject_type)` | then `.key(k)`, `.changes(v)`, `.meta(m)` |
| `AuditActor::user(&principal)` | the user behind a principal, with the request's credential |
| `AuditActor::signed_in(user_id, guard, credential)` | a user who just proved their credentials |
| `AuditActor::job(kind, initiated_by)` | a queued job (`queue:<kind>`) |
| `AuditActor::system(Some("command"))` | a console command (`console:<command>`), or the application (`None`) |
| `AuditActor::guest()` | nobody signed in |
| `AuditContext::new(actor, request)`, `AuditContext::system(command)` | the actor and request of an operation |
| `diff(before, after, &redact)`, `changes_of(&[(field, old, new)])` | build `changes` |
| `bounded(value)`, `summary(text)` | the bounding the log applies |
| `subject_key(key)` | how a key is stored in `subject_key` (to look entries up) |
| `MAX_TEXT`, `MAX_DEPTH`, `MAX_ITEMS`, `MAX_BYTES` | 1,000 characters, 8 levels, 100 items, 64 KiB |

## Viewing the log

| Permission | Shows |
|---|---|
| `audit.view_all` | **System → Audit log**: every entry, newest first, filtered by event, subject, user and time. Meant for auditors: no row is hidden from them. |
| `audit.view_auth` | with `audit.view_all`: also sign-in, sign-out, token and JWT entries (`auth.*`, `jwt.*`, `token.*`), and the client address and browser of every entry |
| `audit.view` | **History** on a record's page, for records the user may view (scope and policy, as the page itself) |

![The audit log with its filters](/img/digging-deeper/audit-log.webp)
*The LMS example: System → Audit log, with the entries of lesson moves.*

![A lesson's History tab](/img/digging-deeper/audit-history.webp)
*The LMS example: a lesson's History tab.*

A record's history lists what the engine recorded about that record
(`created`, `updated`, `trashed`, `deleted`, `restored`, `force_deleted`,
`attached`, `detached`, `pivot_updated`, `action:<name>`), with only the fields the user may read now. An entry
your code writes appears there too when it uses the resource's slug as
subject type, the record's key and one of those events (as the LMS's
`reorder` above); any other entry is only in the full log.
Actors appear by name to users who may list users (`users.view_any`),
otherwise by id; credentials appear by kind only (`session`, `token`, `jwt`,
`console`). Every filter runs in the database, so pages are always full and
nothing reveals entries a user may not see.

The slugs `user`, `role`, `permission`, `settings` and `session` are reserved
for the framework's own entries: resources cannot use them.

API (admin session):

| Method | Path | Query | Response |
|---|---|---|---|
| GET | `/admin/api/audit` | `event`, `subject_type`, `subject_key` (with `subject_type`), `actor` (user id), `from`, `to` (RFC 3339), `per_page` (≤ 100), `cursor` | `{data: [AuditEntryView], meta: {nextCursor?}}` |
| GET | `/admin/api/resources/{slug}/records/{key}/history` | `per_page`, `cursor` | the same |

Access is checked before the query string is read (403, then 422 for a bad filter).

## Failed sign-ins

`auth.sign_in_failed` has the same shape whatever the reason (unknown email,
wrong password, deactivated account): a guest actor and meta
`{email, guard}`. Requests refused by throttling record nothing. Beyond
`audit.sign_in_failures_per_minute` failures in a minute (per instance), the
rest are counted and recorded as one `auth.sign_in_failed_burst` entry
(`{count}`) with the first failure of a later minute.

## Retention

Entries older than `audit.retention_days` (default 365) are deleted every
hour by a background pass that starts when the application boots (`serve`,
`queue:work`), and by `rustcms audit:prune`
([Data retention](../operations/retention.md)).

`[audit]` (`config/audit.toml`):

| Key | Default | |
|---|---|---|
| `retention_days` | `365` | 1 to 36,500 (boot fails outside) |
| `sign_in_failures_per_minute` | `600` | per instance |

## Personal data

Entries hold personal data: user ids, IP addresses, user agents, names and
emails in user changes, and the email typed in failed sign-ins (which may be
someone else's address, or a password typed in the wrong box). Restrict
access to the table and its backups, keep the retention as short as you can,
and include the table when you answer erasure requests, for example:

```sql
DELETE FROM audit_logs WHERE actor_user_id = 42 OR (subject_type = 'user' AND subject_key = '42');
```

The log is append-only for the application (nothing in RustCMS updates an
entry), not tamper-proof: anyone with write access to the database can change
it. Ship it to external storage if you need that.

## Related

- [Authorization](../admin/authorization.md)
- [Queues](queues.md)
- [Data retention](../operations/retention.md)
- [Revocation](../security/revocation.md)
