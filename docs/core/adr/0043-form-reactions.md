# ADR 0043: Form reactions

- Status: accepted (0.13.1, vocabulary 19). Contributed by @giangittb112000
  (PR #3, drafted as "ADR 0041"), ported onto 0.13.0 and amended after
  review: no strict reactions, the state is the user's form (ADR 0037),
  bounded answers. Amended by ADR 0044 (part D): relationship rows in the
  state, row patches by key or ref, reactions on relationship repeaters.
- Date: 2026-11-17

## Context

A form cannot react to what the user types. Common cases:

- a blog or a company site fills a slug from the title, unless the user typed one;
- a shop totals the lines of an order as they are edited;
- a course platform keeps the blanks of a question in step with its text (an
  application rule: the framework only runs it).

Today an application can compute these in `mutate_before_create`/`update`,
but the user only sees the result after saving, and cannot correct it before.

Constraints from the existing design:

- **The SPA draws from data** (ADR 0002): Rust closures cannot run in the
  browser. A reaction runs on the server and answers with changes to apply.
- **The server decides.** Field rules, `scope()`, policies and conditions
  (ADR 0037) hold on every path; what the browser applied is never trusted.
- **What a write's outcome tells.** ADR 0037 evaluates conditions on what the
  user's form holds, never on values the user may not read or a record they
  may not view: a reaction's answer is an outcome too.
- **`computed()` (ADR 0039) already runs application code on demand** for one
  page: an async closure with `Ctx`, a time limit, a concurrency bound, a rate
  limit per user, failures logged and shown as "not available". Reactions
  follow it.
- Forms of settings pages (`SettingField`) and actions (`ActionField`) are
  their own types, not `Input`.
- JSON repeater rows have no stable ids yet; relationship repeater rows have
  keys or client refs (ADR 0040).

## Decision

### Declaring

```rust
fn form() -> Form {
    Form::new().schema(components![
        TextInput::make("name").reacts(|_ctx, state| {
            let before = state.previous().and_then(|v| v.as_str()).map(slugify).unwrap_or_default();
            let follows = state.text("slug").is_empty() || state.text("slug") == before;
            let next = slugify(state.text("name"));
            Box::pin(async move { Ok(if follows { vec![FormPatch::set("slug", next)] } else { vec![] }) })
        }),
        TextInput::make("slug"),
        Repeater::make("lines").schema(components![/* … */]).reacts(order_service::totals),
    ])
}
```

- `Input::reacts(f)` with
  `f: for<'a> Fn(&'a Ctx, &'a FormState) -> BoxFuture<'a, AppResult<Vec<FormPatch>>>`:
  async, with the request's `Ctx`, so it may read the database (as
  `Computed::make`). It reads; it should not write (its `Ctx` is not a
  transaction, and nothing it writes is audited).
- `.reaction_timeout(d)`: default 5 seconds, at most 30.
- Resources only. Settings pages and action forms are left for later.

`FormState` is what the user has in the form:

- `operation()` (`Create` or `Update`), `key()` (the record's, on edit),
  `changed()` (the input whose change asked for the reaction), `previous()`
  (that input's value when the panel last applied an answer or opened the
  form, when sent);
- `get(name) -> Option<&Value>` (wire format, ADR 0003), `text(name) -> &str`
  (`""` when absent or not a string), `rows(name) -> &[Value]` (a JSON
  repeater's rows; empty when absent).

The state is built as the form is (ADR 0037, `condition_values`), then what
the user typed:

1. **What the form held**: on create, the declared defaults of fields the
   user may read; on edit, the stored values of readable fields **only when
   the user may view the record** (a user who may update but not view it
   edits blind, ADR 0037); anything else is the form's empty value.
2. **What the user typed**: values sent for inputs of the form whose field
   the user may **write** in this operation, that decode (the form may be
   half typed: a value that does not decode is left out, no 422). Values sent
   for fields the user may not write are not the form's and are ignored.
3. **Through a relation** (a create form opened from another record's
   relation section): the key the relation binds, authorized as that create
   is (`relation_create`), as the save binds it.

Conditions (ADR 0037) are evaluated on that state. The reaction sees only the
fields the user may read, never encrypted fields, links
(`BelongsToManySelect`) or relationship repeaters. **Its values are user
input**: a reaction looking up a `belongs_to` id or a media id checks it
against what the user may see (`ctx.records(…)`), as any handler does.

### Patches

| Patch | Effect |
|---|---|
| `set(field, value)` | sets the field |
| `set_if_empty(field, value)` | sets it when it is `null`, `""` or `[]` |
| `add_row(field, row)` / `add_row_at(field, index, row)` | adds a row to a JSON repeater (at the end, or at `index`) |
| `remove_row(field, index)` | removes a row |
| `move_row(field, from, to)` | moves a row |
| `flag_row(field, index, message)` | a warning under a row; changes nothing |
| `notice(message)` | a message to the user; changes nothing |

- Values are in wire format. Patches apply in order: a row index refers to
  the rows as the previous patches left them, and **conditions are evaluated
  again after each patch** (a patch that shows a field lets the next one fill
  it).
- Messages are translated with `ctx.t()` and hold at most 500 characters.
- An answer holds at most 200 patches; the rest are dropped.
- Row patches apply to **JSON repeaters only**, to the form's list in place;
  relationship repeaters are left for later (they need patches by row key).

The server keeps a patch only if:

- its field is an input of the form, at the top level, that the user may
  **read and write** in this operation (the value lands in the user's form);
- the field is not hidden by its conditions on the state as the previous
  patches left it;
- the field is not hidden, encrypted, a link list or a relationship repeater;
- the value passes the input's checks (`check_input`: kind, options, bounds,
  length; `required` is not checked) and, for rows, the repeater's checks;
- row indexes are in range and the row count stays within the repeater's;
- it changes something (`set` to the current value and `set_if_empty` on a
  filled field are dropped).

A kept value is sent **as a save would store it**: rows checked (hidden row
inputs left out), values canonical (a decimal at the column's scale, a
select's option as declared). A dropped patch is logged (`tracing::warn!`,
resource, field and reason, never values): it is the application's bug, not
the user's.

### No strict reactions

The contribution's `.strict()` (refuse a save while the reaction still had
changes to make) is not kept. It ran application code inside the write's
transaction with the record locked, outside the reaction limits; relationship
rows and actions bypassed it; a non-canonical patch or a record saved before
the reaction existed could refuse every later save; API clients (403 on
`form/react`) and relation dialogs could not see what it wanted. Reactions
help the user fill the form; invariants belong in `mutate_before_*` or
validation, which hold on every write path. A later ADR may add a bounded,
server-side consistency rule.

### The route

`POST {admin}/api/resources/{slug}/form/react`, listed in `http::routes` and
in `fixtures/authz/admin.toml`.

```json
{ "operation": "create", "changed": "name", "state": { "name": "Hello" }, "previous": "", "seq": 7 }
→ { "seq": 7, "patches": [{ "op": "set", "field": "slug", "value": "hello" }] }
```

- **Before the body is read**: the user may create or update records of the
  resource (`admits`, the same 403 whatever the body), and the request counts
  towards 120 per user per minute (`form-react:{user}`).
- `operation`: `create` needs `CREATE` (policy, no record), with `via:
  {parent, record, relation}` when opened through a relation; `edit` needs
  `key`, `UPDATE` admitted, the record found within `scope()` and the policy
  allowing `UPDATE` on it (404/403 as `update`); `VIEW` on it decides whether
  its stored values are read. `view`, or `edit` without a key, is 422.
  **Reactions never run for view pages, tables, exports or saves.**
- `changed` must name an input with `.reacts()` the user may write; otherwise 422.
- A JSON repeater sent with more rows than it may hold (its `max_items`, at
  most 1,000) is 422 before the reaction runs.
- Only the reaction of `changed` runs. Patches the SPA applies do not trigger
  further reactions (no loops).
- Errors and timeouts answer `{seq, patches: [], failed: true}` and are logged.
- At once: 2 reactions per user, 8 across users (429 beyond, after a short
  wait); the usual body limit.
- Nothing is stored, nothing is audited.

### Schema (vocabulary 19)

- `InputSchema.reacts: true` on inputs with a reaction, for `create` and
  `edit` only, for a user who may change the input; never on repeater rows.
- Wire types `FormReactRequest` (`previous?`, `via?`), `FormReactVia`,
  `FormReactResponse`, `FormPatch` (tagged by `op`).

### The SPA

- After an input with `reacts` changes, and the user stops typing for about
  400 ms, the SPA sends the form's values in wire format (as a save sends
  them, without secrets, links or relationship rows) with an increasing
  `seq`, the input's `previous` value, and `via` on a relation's create form.
- A response is applied only if its `seq` is the latest **and** the form has
  not changed since it was sent (a newer request is then on its way).
- `set`/`set_if_empty` and row patches go through `setValue` (dirty, not
  validated); a rich text editor draws a patched document without reporting a
  change; `flag_row` shows a warning under the row until the next response or
  until the user changes that repeater; `notice` shows a toast.
- Network errors, 429 and `failed` are ignored silently: the form still works
  without reactions.
- An SPA of vocabulary 18 or below ignores `reacts`.

### Boot checks (panic)

- `.reacts()` on an input that is not a field of the resource, on `hidden`,
  encrypted, link or relationship repeater inputs, or inside repeater rows.
- `.reaction_timeout()` without `.reacts()`; a timeout of zero or over 30
  seconds.

## Consequences

- Slugs, totals and synchronized rows are declared next to the input, in
  Rust, with access to services and the database; the user sees the result
  before saving.
- The browser calls the server while the user types: one request per pause,
  bounded per user. Reactions should be light; a slow one only fails alone.
- Saves never wait for a reaction; what must always hold is enforced by hooks
  or validation.
- `InputSchema` gains `reacts`; applications matching it exhaustively need it.
- Left for later: reactions on settings pages and action forms, row patches
  for relationship repeaters (row keys), reactions inside rows, a bounded
  server-side consistency rule, and an SDK `registerFormReaction` for
  immediate feedback in the browser.
