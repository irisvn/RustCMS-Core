# Actions

An action is an operation on records other than create, edit and delete:
publish, move, enroll, send. It runs on one record (a row or the record's
page) or, when `.bulk()`, also on the records selected in a list. Each record
is authorized like any write, and the whole run is one transaction.

![A demo post row: the labelled Unpublish action and the ⋯ menu](/img/admin/actions-row.webp)
*The demo application: row actions in the posts list.*

## Quick start

<<< @/../../../examples/demo/src/admin/resources/post_resource.rs#publish [src/admin/resources/post_resource.rs]

Add it to the resource's `actions()`, then run `rustcms permission:sync`:
the action declares the permission `posts.publish`.

```rust
fn actions() -> Vec<Action<post::Entity>> {
    vec![/* the actions */]
}
```

Name the entity (`Action::<post::Entity>::new`) so the closure's types are
inferred. The handler receives the `Ctx`, the transaction, the records the
user may run the action on, and the form's `data`, and returns a boxed
future (`Box::pin(async move { … })`). Its writes commit together; an error
rolls the whole run back.

## Who may run it

Each selected record is checked by the policy with the action's **ability**:
its name by default, or `.ability("update")` to reuse an existing one. The
default policy needs `{slug}.{ability}` (`posts.publish`). Denied records are
reported, not processed, and the answer tells the client per record.

An action is listed in the schema, and may run at all, only if:

- the user has its `.requires_permission("…")` permission, when it has one
  (403 otherwise, before the body is read), and
- the policy `admits` its ability ([Authorization](authorization.md)).

Both permissions are declared for `permission:sync`. The LMS's *Enroll a
learner…* runs with the course's `view` ability, but only for users who may
create enrollments:

<<< @/../../../examples/lms/src/admin/resources/course_resource.rs#actions [src/admin/resources/course_resource.rs]

Action handlers are your code: they call no resource hooks and may write any
column. Check what the handler needs in the handler, and return
`ValidationErrors` to refuse (the whole run rolls back and the panel shows
the messages, as above).

## Per-record visibility

<<< @/../../../examples/demo/src/admin/resources/post_resource.rs#unpublish [src/admin/resources/post_resource.rs]

`visible_when(|ctx, record| …)` decides on the server, for each record as
stored and for the user of `ctx`, whether the action is offered there. List
and record answers carry the decision in each envelope's `abilities`
(`"unpublish": false` on drafts), together with the policy's; the client
never decides. Running the action decides again, on each record locked in
the action's transaction:

- a run on **one** record the action is hidden on is refused with **403**,
  and nothing runs;
- a run on **several** records (a selection) **skips** the records it is
  hidden on: they are reported in `denied`, like policy refusals, and never
  reach the handler, which runs on the others.

## Outcomes: a message, a page to open

`Action::new` handlers return `()`. Declare the action with
`Action::with_outcome` when its handler should tell the panel what to do
next; it returns an `ActionOutcome`:

<<< @/../../../examples/demo/src/admin/resources/post_resource.rs#preview [src/admin/resources/post_resource.rs]

- `ActionOutcome::message(text)` / `.with_message(text)`: shown to the user
  with the action's summary (plain text, at most 500 characters,
  `MAX_OUTCOME_MESSAGE`), as written: translate it with `ctx.t("…")`
  ([i18n](i18n.md)).
- `ActionOutcome::open(url)` / `.with_open(url)`: once the action has
  committed, the panel opens the page in a new tab with `noopener` and
  `noreferrer`, and shows a link to it as well (a browser may block a tab
  opened after the request). Accepted: `https://` URLs without credentials
  (a DNS name, an IPv4 or a bracketed IPv6 address; a port of 1–65535),
  and absolute paths of this site (`/admin/…`, `/blog/…`; never `//…`, `..`,
  backslashes or whitespace), at most 2,048 bytes (`MAX_OUTCOME_URL`).
  Anything else (`http:`, `javascript:`, `data:`) fails the action with a
  server error and rolls it back. A page is opened only for a run on **one**
  record: a run on several records whose handler returns one is refused
  (422, rolled back). That is decided once the handler has returned, from the
  keys the request named (a request naming two records, one of them hidden,
  reaches the handler with one record and is still refused): its database
  writes roll back, not what it did elsewhere. Do work outside the database
  in a [job](../digging-deeper/queues.md) dispatched in the action's
  transaction, so a refused run never does it. The panel checks the URL
  again before opening it.

The keys (`processed`, `denied`, `missing`, `replayed`) are the framework's:
whatever the handler sets there is ignored. The outcome is part of the
idempotency receipt, so a retried request answers with the same message and
page, but only to a user who may still view every record it processed:
otherwise (the action or someone else hid a record meanwhile) the message and
page are left out of the answer, first or replayed. Receipts are kept for 24
hours: never put a secret in `open` (a presigned or bearer URL is stored and
answered again as it was, possibly expired); link to a page of the app that
checks the user instead.

## Placement in lists

A list row shows at most one row action with its label: the first action
marked `.primary()` that the row offers. The other row actions are in the
row's **⋯** menu, in declaration order; destructive ones (`.destructive()`,
which is the color `danger`) come last, after a separator. Several actions
may be primary when `visible_when` makes them exclusive (`publish` on
drafts, `unpublish` on published posts). A `.bulk()` action is a row action
too (bulk means *also* offered for selections, in the bar above the table),
on the rows the server offers it. View, Edit, Delete and Restore keep their
buttons. Trashed records offer no actions.

`.color(…)` only has an effect as `"danger"` (what `.destructive()` sets):
the action goes last in the **⋯** menu and its bulk button is the
destructive variant. `.icon(…)` is accepted but not shown yet.

A record's page shows its actions the same way (the first primary one it
offers labelled, the others under ⋯; on a phone Edit and Delete go under ⋯
too, Delete last), and so do the rows of relation tables
on a parent's page, with the related resource's actions: the server's
abilities for each record decide, and running one is authorized as from the
list.

```rust
Action::<post::Entity>::new("publish", publish).visible_when(|_ctx, p| !p.published).primary(),
Action::<post::Entity>::new("unpublish", unpublish).visible_when(|_ctx, p| p.published).primary(),
Action::<post::Entity>::new("purge_cache", purge).destructive(),
```

![The demo's bulk bar with Publish and Move to category](/img/admin/actions-bulk-bar.webp)
*The demo application: bulk actions on a selection of posts.*

## Actions with a form

An action can ask for values first, in a dialog. The demo moves posts to a
category the user may see:

<<< @/../../../examples/demo/src/admin/resources/post_resource.rs#move [src/admin/resources/post_resource.rs]

A form can mix field kinds:

```rust
// A sketch: the handler reads data["mode"], data["weight"], data["note"].
.form(vec![
    ActionField::belongs_to("category_id", "categories").label("Category").required(),
    ActionField::select("mode", [("move", "Move"), ("copy", "Copy")]).default("move"),
    ActionField::integer("weight").help("Higher comes first"),
    ActionField::text("note").max_length(200).placeholder("Why?"),
])
.submit_label("Move")
```

| Field | Value in `data` |
|---|---|
| `text`, `textarea`, `select` | text (`select`: one of the options) |
| `integer` | a 32-bit integer (JSON number) |
| `boolean` | `true` / `false` |
| `date` | `"YYYY-MM-DD"` |
| `datetime` | a point in time, entered in UTC (the input says so) like a form's `DateTimePicker` of an instant column; sent as RFC 3339 and given to the handler in UTC: `"2026-10-12T02:30:00Z"` (`chrono::DateTime::parse_from_rfc3339`) |
| `belongs_to(field, target)` | the key of a record of `target` the user may view, as text (`"12"`, a UUID) |

Every field takes `.label(…)` (default: the name in words), `.help(…)`,
`.placeholder(…)`, `.required()`, `.max_length(n)` and `.default(value)`.
`data` holds every field of the form: fields left empty are `null`
(`.required()` refuses that), or their `.default(…)`. The server checks the
form in this order:

1. The user may run the action at all (`requires_permission`, then the
   policy's [`admits`](authorization.md)): 403 before the body is read.
2. Each value (422 on the field): unknown fields, types, options, lengths. The
   values are made canonical (a `belongs_to` key may be sent as `12` or
   `"12"`; `integer` fields take JSON numbers only), so a
   retry of the same request with the same `Idempotency-Key` is recognized
   however it was written.
3. In the action's transaction: each `belongs_to` value is a record the user
   may view (422 otherwise). A replay of an action that already ran reports
   that run, even if the record has gone since.

The dialog is titled with the action's label without a trailing ellipsis
(`Move to category`), and its button runs the action with
`.submit_label("Move")`, by default that same label.

An action without a form refuses any `data` (422). A handler may return
`ValidationErrors` (on a form field, or for the whole form): the action is
rolled back and the dialog shows them. The dialog's relationship inputs read
`GET …/actions/{action}/fields/{field}/options`, under the same admission.

![The demo's Move to category dialog](/img/admin/actions-form-dialog.webp)
*The demo application: an action's form.*

## Reference

| `Action` | Does |
|---|---|
| `Action::<E>::new(name, handler)` | an action whose handler returns `()` |
| `Action::<E>::with_outcome(name, handler)` | an action whose handler returns an `ActionOutcome` |
| `.label(text)` | the button's label (default: the name in words) |
| `.bulk()` | also offered for selections |
| `.requires_confirmation(question)` | asks this question before running |
| `.ability(name)` | the ability checked per record (default: the action's name) |
| `.requires_permission(name)` | refuses users without this permission (403, hidden) |
| `.visible_when(\|ctx, record\| …)` | offered only on records for which it holds |
| `.primary()` | the row's labelled action |
| `.destructive()`, `.color(name)` | last in the menu (`danger`) |
| `.icon(name)` | accepted, not shown yet |
| `.form(fields)`, `.submit_label(text)` | a dialog asking for values first |

Labels, confirmation questions and submit labels are translated per resource
([i18n](i18n.md)).

### API

`POST /admin/api/resources/{slug}/actions/{action}` with
`{"keys": ["12", "13"], "data": {…}}` (`data` only for actions with a form),
at most 500 keys. It accepts an `Idempotency-Key` header
([retrying writes safely](../reference/admin-api.md#retrying-writes-safely))
and answers the `ActionOutcome`:
`{processed, denied, missing, replayed?, message?, open?}`.

## Checked at boot

The boot stops when:

- two actions of a resource have the same name, or a name is longer than 57
  bytes;
- an action is named after a built-in ability: `view_any`, `view`, `create`,
  `update`, `delete`, `restore`, `force_delete`, `export`, `import`;
- a form field's name is not unique snake_case;
- a form field's `.default(…)` is not a valid value, or is set on a
  `belongs_to` field (those take no default).

## Related pages

- [Authorization](authorization.md), [resources](resources.md), [tables](tables.md)
- [Queues](../digging-deeper/queues.md), for work outside the database
- [Admin API](../reference/admin-api.md)
