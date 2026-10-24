# Actions

Actions run on one record (row actions) or, when `.bulk()`, also on the
selected records, with the same authorization as any write.

```rust,ignore
fn actions() -> Vec<Action<post::Entity>> {
    vec![Action::<post::Entity>::new("publish", |_ctx, txn, posts, _data| {
        Box::pin(async move {
            for post in posts {
                let mut post = post.into_active_model();
                post.published = Set(true);
                post.update(txn).await?;
            }
            Ok(())
        })
    })
    .label("Publish")
    .bulk()
    .requires_confirmation("Publish the selected posts?")]
}
```

Name the entity (`Action::<post::Entity>::new`) so the closure's types are
inferred. Each selected record is checked by the policy with the action's
ability (default: its name, or `.ability("update")`); denied records are
reported, not processed, and envelopes tell the client per record. Add
`.requires_permission("posts.publish")` to refuse the action outright (403, and
hidden from the schema) for users without that permission.

## Per-record visibility

```rust,ignore
Action::<post::Entity>::new("unpublish", unpublish)
    .ability("update")
    .visible_when(|_ctx, post| post.published)
```

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

```rust,ignore
Action::<invoice::Entity>::with_outcome("pdf", |_ctx, _txn, invoices, _data| {
    Box::pin(async move {
        let url = format!("https://files.example.com/invoices/{}.pdf", invoices[0].id);
        Ok(ActionOutcome::open(url).with_message("The PDF opens in a new tab."))
    })
})
```

- `ActionOutcome::message(text)` / `.with_message(text)`: shown to the user
  with the action's summary (plain text, at most 500 characters).
- `ActionOutcome::open(url)` / `.with_open(url)`: once the action has
  committed, the panel opens the page in a new tab with `noopener` and
  `noreferrer`, and shows a link to it as well (a browser may block a tab
  opened after the request). Accepted: `https://` URLs without credentials
  (a DNS name, an IPv4 or a bracketed IPv6 address; a port of 1–65535),
  and absolute paths of this site (`/admin/…`, `/blog/…`; never `//…`, `..`,
  backslashes or whitespace), at most 2,048 bytes. Anything else
  (`http:`, `javascript:`, `data:`) fails the action with a server error and
  rolls it back. A page is opened only for a run on **one** record: a run
  on several records whose handler returns one is refused (422, rolled
  back). That is decided once the handler has returned, from the keys the
  request named (a request naming two records, one of them hidden, reaches
  the handler with one record and is still refused): its database writes
  roll back, not what it did elsewhere. Do work outside the database in a
  [job](../digging-deeper/queues.md) dispatched in the action's
  transaction, so a refused run never does it.
  The panel checks the URL again before opening it.

The keys (`processed`, `denied`, `missing`) are the framework's: whatever
the handler sets there is ignored. The outcome is part of the idempotency
receipt, so a retried request answers with the same message and page, but
only to a user who may still view every record it processed: otherwise (the
action or someone else hid a record meanwhile) the message and page are
left out of the answer, first or replayed. Receipts are kept for 24 hours:
never put a secret in `open` (a presigned or bearer URL is stored and
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
buttons.

A record's page shows its actions the same way (the first primary one it
offers labelled, the others under ⋯), and so do the rows of relation tables
on a parent's page, with the related resource's actions: the server's
abilities for each record decide, and running one is authorized as from the
list.

```rust,ignore
Action::<post::Entity>::new("publish", publish).visible_when(|_ctx, p| !p.published).primary(),
Action::<post::Entity>::new("unpublish", unpublish).visible_when(|_ctx, p| p.published).primary(),
Action::<post::Entity>::new("purge_cache", purge).destructive(),
```

## Actions with a form

An action can ask for values first, in a dialog:

```rust,ignore
Action::<post::Entity>::new("move", |_ctx, txn, posts, data| {
    Box::pin(async move {
        // `data` holds every field of the form: checked, canonical, defaults applied.
        let category = data["category_id"].as_str().and_then(|k| k.parse::<i32>().ok());
        for post in posts {
            let mut post = post.into_active_model();
            post.category_id = Set(category);
            post.update(txn).await?;
        }
        Ok(())
    })
})
.label("Move to category…")
.bulk()
.form(vec![
    ActionField::belongs_to("category_id", "categories").label("Category").required(),
    ActionField::select("mode", [("move", "Move"), ("copy", "Copy")]).default("move"),
    ActionField::integer("weight"),
    ActionField::text("note").max_length(200),
])
```

| Field | Value in `data` |
|---|---|
| `text`, `textarea`, `select` | text (`select`: one of the options) |
| `integer` | a 32-bit integer (JSON number) |
| `boolean` | `true` / `false` |
| `date` | `"YYYY-MM-DD"` |
| `datetime` | a point in time, entered in UTC (the input says so) like a form's `DateTimePicker` of an instant column; sent as RFC 3339 and given to the handler in UTC: `"2026-10-12T02:30:00Z"` (`chrono::DateTime::parse_from_rfc3339`) |
| `belongs_to(field, target)` | the key of a record of `target` the user may view, as text (`"12"`, a UUID) |

Fields left empty are `null` (`.required()` refuses that), or their
`.default(…)` (checked at boot). The server checks the form in this order:

1. The user may run the action at all (`requires_permission`, then the
   policy's [`admits`](authorization.md)): 403 before the body is read.
2. Each value (422 on the field): unknown fields, types, options, lengths. The
   values are made canonical (`7` and `"07"` are the same number), so a
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
