# ADR 0044: Repeaters, second round

- Status: accepted for 0.14.0 (vocabularies 24 to 27): parts A and B, reworked after review; `options_fn`
  (part E, vocabulary 26) and reactions on relationship rows (part D, vocabulary 27), redesigned after review.
  Part C, `validate_rows`, `shown_when_fn`, `required_fn` and `form/rules` were not accepted (see "Not accepted,
  or later").
- Date: 2026-11-18 (proposed); 2026-11-23 (decided)
- Contributed by @giangittb112000 (PR #4). Decisions: `docs/core/plans/repeater-v2-decisions.vi.md`; plan of the
  rework: `docs/core/plans/contrib-0.14-part6.md`, and for parts D and E `docs/core/plans/contrib-0.14-6de.md`
  (its v2 and v2.1 sections); review: `docs/core/reports/2026-11-22-pr4-review.vi.md`.
- Amends ADR 0037 (conditions in rows reading outer levels), ADR 0040 (repeaters) and ADR 0041 (points 4, 5 and
  6), and ADR 0043 (form reactions: row patches, a save-time options check outside the transaction). ADR 0018
  (no `lock_parent` chains) is unchanged.

## Context

ADR 0040 and 0041 shipped repeaters with limits that were left "for later". Relationship repeaters had no
conditions, at most one repeater per child resource, no rich text, media or `belongs_to` in rows, one level only,
and nothing on view pages. JSON rows took no media, rich text or record references. Conditions in a row could
read only that row.

The case behind the proposal is a question bank of three tables:

- `questions` → `question_blanks` → `question_options`;
- a choice question's options are the options with `blank_id` null;
- a blank's choices are the options with its `blank_id`.

The question's form should edit its options or its blanks according to the question's type, with rich text
and images in rows, and a view page should show the options under the question.

The constraints of ADR 0040 and 0041 still hold:
- the server decides on every path;
- a relationship repeater's value is a change, not a set: every change to a row is one the user sent;
- every child row goes through the child's scope, policy, field rules, hooks and history;
- explicit locks follow `lock_rank`, media last;
- any error rolls back the whole write.

The proposal had five parts. What was decided, and why, is in the decisions document (2026-11-22). In short:

| Part | Content | Outcome |
|---|---|---|
| A | Conditions on and in relationship repeaters, conditions reading the root form, filtered has-many relations, table layout, relationship rows on view pages | accepted, reworked (vocabulary 24) |
| B | Media, rich text and `belongs_to` in rows (JSON and relationship) | accepted, fixed (vocabulary 25) |
| C | Nested relationship repeaters (three levels), `inherit`, chained locks | not accepted |
| D | `validate_rows`, reactions on relationship rows | reactions accepted, redesigned (vocabulary 27); `validate_rows` not accepted |
| E | Server closures (`shown_when_fn`, `required_fn`, `options_fn`), `form/rules` | `options_fn` accepted, redesigned (vocabulary 26); the rest not accepted |

## Decision: part A (vocabulary 24)

### Conditions on a relationship repeater

- `shown_when` is accepted on a relationship repeater and on the layouts around it, under ADR 0037's rules.
  No condition may name the repeater (it holds a change, not a value), as before.
- **Hiding keeps the rows.** A hidden relationship repeater is a hidden input: its stored rows stay, whatever
  the write. Switching a question from "single choice" to "fill in the blanks" hides its options; switching
  back shows them again. The proposal deleted the rows the user could see when a write hid the repeater. That
  was refused:
  - every change to a row must be one the user sent (ADR 0040); a deletion caused by another input's value
    is not;
  - it could delete rows another user added after the form was opened;
  - it would run on imports and API calls, deleting data nobody saw.

  An application that wants the rows gone removes them in its own hook (`mutate_before_update`,
  `after_save`). An opt-in per repeater, limited to the rows the form showed, may come later if asked for.
- **What the client sends for a hidden repeater** (hidden on the server's state after the user's changes) is
  still admitted (`create`, `update`, `delete` on the child: 403) and shape-checked (422), then discarded.
  Admission and shape come first so that a forbidden or malformed change never passes because a condition
  hides it.
- **`min_items` does not apply while hidden.** On create, a hidden repeater writes nothing.
- **Boot refuses `clear_when_hidden`** on a relationship repeater (its message says the rows are kept).
  A condition on a `deletable(false)` repeater is accepted: hiding deletes nothing.
- **Panel.** The panel hides the repeater and keeps its unsaved changes (shown again, they are back); it
  does not send them while hidden. No notice: nothing is removed.

### Conditions in rows, and the root form (both kinds of repeater)

- **Builder.** `when(name)` keeps naming an input of the same row (or of the form, outside rows). Two new
  starters name inputs outside the row:
  - `when_root(name)`: an input of the record's form, the top level, whatever the depth;
  - `when_parent(name)`: an input of the enclosing row of a JSON repeater nested in a JSON row. Refused in
    relationship rows (they have no parent row) and outside rows.

  The operators are those of `when`.
- **Wire.** Each leaf condition (`eq`, `ne`, `is_in`, `filled`, `is_true`) gains `scope`: `"row"` is the
  default, never sent; otherwise `"parent"` or `"root"`. In Rust, the leaves of `Condition` gain a `scope`
  member (`Scope::{Row, Parent, Root}`); the builders are unchanged.
- **Boot checks** follow ADR 0037: the named input exists at that level and has a kind conditions can read;
  values decode as that kind; no cycle. A row input never depends on itself through the root: the root does
  not read rows.
- **Meaning.**
  - A root or parent input that is hidden counts as `null`, as in ADR 0037.
  - The root's values are what the user's form holds after the write (stored values, readable fields only,
    with the user's input over them). A row is evaluated after the root's own conditions.
- **JSON rows** are checked in two steps:
  - `accept_input` checks every value sent in every row (kind, length, options, bounds), even under a
    condition;
  - dropping hidden inputs and "required when shown" happen once the root's values are known (after the
    lock on update; in the settings save). Settings pages evaluate the root on the page's values; action
    forms on the values sent.
- **Relationship rows: two modes, decided at boot per repeater.**
  - **Legacy mode**: the repeater's `.schema()` declares no condition (no `shown_when` on a row input or a
    layout). Rows are checked exactly as in 0.13: the child form's conditions, evaluated on the child
    form's full inputs. The panel evaluates none in the rows, as in 0.13. Applications upgrading from 0.13 see
    no change.
  - **Explicit mode**: the rows declare conditions (`when`, `when_root`, a layout's). Those are the rows'
    conditions, also evaluated by the panel; the child form's do not apply to the rows. A stored row is
    evaluated on its values (it is locked through `lock_visible`, so viewable) with the row's data over
    them; a new row on the child's declared defaults. Hidden row inputs are dropped from the row's data; a
    shown input whose child field is required is "is required". A conditional row input needs a child column
    that stores a hidden value (nullable or with a default; boot).

  The proposal made every repeater explicit, so the child form's conditions stopped applying to rows: a
  silent change for 0.13 applications. Two modes keep 0.13's behaviour unless the application asks for the
  new one.
- **`clear_when_hidden`** stays refused inside rows of both kinds.

### Filtered has-many relations

- `Relation::has_many(name, target, field).where_null(column)` and `.where_eq(column, value)` restrict the
  relation to the target's records matching every filter (AND). Filters are refused on belongs-to-many
  relations.
- **Kinds (boot).** `column` is a field of the target, not encrypted, not the parent key, not the primary key,
  not a repeater's order column, never a row input.
  - `where_null` needs a nullable column.
  - `where_eq` takes boolean, integer (`i32`, `i64`), UUID and date fields only, with a value of the field's
    kind. Strings, JSON, floats and decimals are refused: a database collation (MySQL compares text without
    case) or rounding would let SQL hold rows the engine's exact check does not, and disjointness at boot
    would not be exact. String filters are left for later.
- **One access check for every path** (`relation_access`): the schema (`RelationSchema`, row repeaters), the
  relation's routes (list, count, create), `counts` columns, `RepeatableEntry::relationship()`, repeater
  changes (before any parent lookup), and reactions through the relation (`via`).
  - **Read.** Every filtered field must be readable by the user (the child's field rules), as the parent
    key already had to be. Otherwise the relation is absent from the schema, its routes answer 404 before
    reading any record (an unreadable parent key alone stays 403, as before), its repeater is left out of
    the form (a change sent for it: 403; no `min_items` to meet on create), its view-page entry and count
    columns are left out.
  - **Create.** The parent key and every filtered field must be writable on create (`can_write`), since the
    create writes them. Otherwise the section has no New, the repeater's `create` is false, and a create
    sent is a 403. The proposal said "the user needs no write rule on the column"; that would let a user
    write, through a relation, a value their field rules forbid.
- **Reads.** The filters apply wherever the relation is read: the section's list and count, `counts`
  columns (including counts under a view policy that depends on the record: the counting query is the
  relation's filtered list), the repeater's rows (reads and locking pass, rechecked under the lock) and
  `RepeatableEntry` rows. The child resource's own pages are not filtered.
- **Writes through the relation.**
  - A record created through it (repeater rows, the section's New) gets the filtered values from the
    server, as the parent key. They are in the input before `required` and the conditions are checked (a
    required filtered field does not fail the create), and re-applied after the conditions. A client that
    sends them gets 422 (`set_by_relation` in the section; "is not allowed" in a row).
  - **Hooks keep membership.** On create, every injected value must survive the mutating hook with an equal
    canonical value; on update, the hook's patch over the stored row must still match the filters.
    Otherwise 500, nothing saved.
  - A key outside the filter is "is not one of the rows" (422), as a key of another parent.
  - New rows of an ordered repeater take positions after every row of the parent, whatever its relation, so a
    unique `(parent, position)` holds across relations; an order renumbers only the relation's rows.
- **Several repeaters on one child resource** are accepted when their relations cannot share a row: some
  column filtered by `where_null` on one side and `where_eq` on the other, or by `where_eq` with two different
  values. Anything else is refused at boot.
  - **Locks** (ADR 0041, points 4 and 6, amended). Each repeater's rows keep their own locking pass, the
    repeaters of one child in declaration order, after children of lower rank. That is safe without one pass
    over their union: every writer of those rows locks the parent first (`lock_parent`, point 3), so two
    writes that could lock them in another order already wait for each other on the parent row. The lock graph
    is unchanged.
  - History records one `rows_changed` entry per relation. The parent's `after_save` sees every row.
- **Internals.** `ListQuery.relation_filters` (`#[doc(hidden)]`, `#[serde(skip)]`, as `through`) carries the
  filters into the relation's list; it is set by the engine and never read from a request.
  `DynResource::counted_by` takes them.

### Rows per parent: `[admin] repeater_rows`

- `[admin] repeater_rows` (default 50, 1 to 500, checked at boot) is the most rows of one parent a
  relationship repeater reads and locks to order, and the most entries of each part of a change (`create`,
  `update`, `delete`, `order`). Over it: 422 `too_many_rows` ("has more than {max} rows (`[admin]
  repeater_rows`)") at the repeater, checked before the change's shape.
- It replaces 0.13's fixed bounds: 100 entries per part and per read, 1,000 rows to order. The default is
  lower so that a save stays short; applications with larger repeaters raise it. The ceiling equals the key
  limit (`Limits::keys`, 500), so the rows locked in one pass fit one key list.
- The schema sends it (`RepeaterRelationship.rowLimit`); the panel reads at most that many stored rows (and
  `max_items`), and shows the repeater read-only when the parent has more. Relation lists (`parent` set)
  accept `per_page` up to it when it is above the resource's page size.

### Table layout

- `.table()` on a repeater (JSON or relationship) lays each row out as a table row: one column per input, the
  row's buttons last.
- Boot refuses `.table()` with layouts (`Section`, `Grid`), a nested repeater, `collapsible`, a file upload or a
  rich editor in the row.
- Schema: `RepeaterSchema.layout` is `"table"`, absent for stacked rows.
- **Panel:** from the `md` breakpoint, a `<table>` whose headers are the inputs' labels (each cell keeps its
  label for screen readers; static classes, no inline style); below `md`, stacked rows; an input hidden by its
  condition leaves its cell empty; errors show under the cell.

### Relationship rows on view pages

- `RepeatableEntry::make(relation).relationship().schema(components![…])` shows a has-many relation's records,
  filtered as above, each with entries of the child's fields. ADR 0040 described such entries but the boot
  refused them; they are built here.
- **Reading.** The panel reads them through the relation's list (`GET …/relations/{rel}`), with the child's
  scope, view policy and read rules. No new route.
  - Order: by the form's repeater order column when the user's lists may sort by it (`sortable()`), else the
    list's own order; the server sorts, so the first `limit` rows are the first by position.
  - Conditions in rows read the row and, with `when_root`, the parent record's stored values.
  - At most `limit(n)` rows (default and maximum 100), then a link to the relation section.
  - Left out, without an error, for a user who may not list the child (`view_any`) or read the relation's
    filtered fields.
- **Checks.** Entries name fields of the child, checked at boot; an unreadable one is left out per user.
- **Default view page.** `Infolist::from_form` shows a relationship repeater as such an entry, its rows'
  inputs as entries (0.13 showed nothing).
- Schema: `EntrySchema.rows` (`{resource, relation, orderBy?, limit}`).

## Decision: part B (vocabulary 25)

Rows may hold what references other records: files, rich text (whose images are files) and, in relationship
rows, `belongs_to` selects. The constraint is ADR 0041's: media rows are locked last, once, after every other
explicit lock of the write.

### Rules of row inputs (JSON rows)

- The rules of a JSON row's inputs are declared on the inputs, as before, inside the row compiler:
  - **`FileUpload`** holds one file id, or a list with `.multiple(max)`; it takes `.accept([...])`,
    `.max_size(bytes)` and `.collection(name)`;
  - **`RichEditor`** holds a document, checked as a rich text field's (versions, bounds); it takes
    `.nodes([...])`, checked at boot against the panel's declared nodes.

  These rules are refused on inputs of fields and settings (the field decides there), and on inputs that are
  not uploads or editors.
- `accept_input` checks the shape of each row's values; media are checked after the hooks with the record's
  own (below).
- **Still refused in JSON rows:** `belongs_to` selects (a JSON row has no field for an options route, and the
  id would have no foreign key), secrets, belongs-to-many inputs, tabs and splits.
- **Settings pages** take the same inputs in their JSON rows (no application nodes there); their media are
  checked with the page's own, and a save rechecks a file moved to another input.

### Relationship rows

- A relationship row's media, rich text and `belongs_to` fields are accepted (ADR 0041 point 5 amended);
  encrypted and belongs-to-many fields stay refused.
- A row's `belongs_to` is checked as a form's: the target visible to the user (`keys_in_scope`), read without
  an explicit lock, after the child's hooks. The foreign key's implicit lock stays behind ADR 0041's 409
  `write_conflict`, as for every form's `belongs_to`.
- The parent key stays out of rows; a child field that `lock_parent`s another resource is not a row input:
  every row write locks only the repeater's parent (no chains, ADR 0018).
- A row's media are gathered after the child's hooks, so a hook adding media to a row is no longer an error.
- A parent with relationship repeaters still cannot link media records (belongs-to-many to media).

### One media check per write, last

- A write's media references are gathered from its media and rich text fields, its JSON rows at any depth,
  and the rows its relationship repeaters write. They are checked once, after every other lock of the write,
  by `media::check_references`:
  - every `(path, rule, ids)` is kept, so each error is at its row's path (`faq.2.icon`, `photos.n1.image`);
  - the ids are deduplicated and sorted, then looked up (visibility first) and locked (shared) in chunks of
    `Limits::keys` (500), in id order across chunks, so the lock order of ADR 0041 point 2 holds;
  - more than 2,000 distinct ids in one write is a 422 `too_many_media` ("refers to too many files at once")
    at the input whose references pass the limit, before any lookup.

  Resource writes and settings saves share it.
- **Hook timing.** Mutating hooks run before references are gathered (they may add references), and the
  media check runs last. A row's `after_save` may therefore run before the write's media check; if the check
  fails, the whole write rolls back, including the rows, the audit entries and the jobs hooks dispatched in
  the transaction. The guide says to do outside effects through jobs dispatched in the write's transaction
  (`ctx.dispatch(txn, …)`, `queue::dispatch(txn, …)`), never `queue::enqueue(db, …)` nor direct calls. The
  decisions document asked for `after_save` after the media check; the plan's review kept the order and
  documents it instead, with a regression test (a row's job, rows and audit entries are not committed when a
  later media check fails).
- **Restores do not check JSON rows' media.** Neither did 0.13 (rows had no media). One column may hold rows
  written under several forms, and the compiled row rules are per column, so a strict check could refuse a
  valid restore. Field media are still checked; a row's file missing after a restore shows as unavailable,
  as a deleted media does.

### Rich text in rows

- In a row of either kind, a `RichEditor` shows a compact toolbar: bold, italic, underline, superscript,
  subscript, images and application elements, plus the tools of formatting the selection already holds.
  `.full()` gives the full toolbar. Schema: `InputSchema.toolbar` is `"compact"` (absent: full).
- The toolbar is presentation: the server checks the document by the field's (or the input's) rules.
- Uploads in a row's editor go into the field's collection (or the input's, in a JSON row).

### Panel

- An upload in progress in any row blocks saving, tracked per row input (`lines.n1.image`, `faq.2.icon`).
- A row's editor content and uploads follow the row through additions, removals, moves and hiding, keyed by
  its identity. Nothing is stored in the browser.
- Media pickers in rows open on the field's (or input's) collection.
- View pages show JSON rows' files and documents (`MediaEntry`, `RichTextEntry`).

## Decision: part E, `options_fn` (vocabulary 26)

The proposal ran closures (`shown_when_fn`, `required_fn`, `options_fn`) inside the save, and a `form/rules`
route answered every rule at once. Only `options_fn` is kept, redesigned so that application code never runs
inside a write's transaction or with a lock held, and never decides on a value the user cannot read.

### Declaring

- `SelectInput::options_fn(f)` / `CheckboxList::options_fn(f)`, where `f` is
  `for<'a> Fn(&'a Ctx, &'a FormState) -> BoxFuture<'a, AppResult<Vec<SelectOption>>>` (`SelectOption` is now at
  the crate root).
- `.depends_on([...])` is required: the inputs of the same form whose change asks again, and whose values a
  save compares again under the lock (may be empty).
- `.options_timeout(d)`: 5 s by default, more than zero and at most 30 s.
- **The contract with the application** (in the guide): the closure gets the request's `Ctx` with **no
  transaction** and the form's state (readable values only). Other reads it makes are not atomic with the
  write: options are a form rule, not a data invariant; invariants belong in hooks.
- **Boot refuses** (panics): `depends_on`/`options_timeout` without `options_fn`, `options_fn` without
  `depends_on`; a timeout out of bounds; an input that is not a select or checkbox list, a `belongs_to` select,
  an encrypted field; a field with `options_fn` bound to two inputs of the form; a dependency that is not an
  input of the same form level with a scalar or list value (hidden, encrypted, link, repeater or JSON value
  inputs refused); `options_fn` in JSON or relationship rows, on settings pages and in action forms; and any
  resource with `options_fn` that is the child of a relationship repeater (its rows are written by
  `create_row`/`update_row` inside the parent's transaction, where no preflight runs).

### The state the closure reads

One builder for the route, the preflight and the locked check:
- the reaction state's authorization (ADR 0043): stored values only for a record the user may view, of fields
  they may read; typed values only for fields they may write; the key a relation binds and its fixed values
  over what was sent;
- then the declarative conditions' normalization (hidden inputs `null`);
- fields the user may read only.

### `POST {admin}/api/resources/{slug}/form/options`

- Request `{operation, key?, via?, input, state}`; answer `{options}` or `{failed: true}`.
- **Authorized as `form/react`, before the body**: create or update admitted, the same `form-react` bucket
  (120 per user per minute, shared with reactions) and the same turns (2 per user, 8 in all; 429 beyond).
  Then create: `create` allowed (through `via`, the relation's `creatable`, part A's `relation_access`); edit:
  `update` allowed on the record, in scope (no `via` on edit).
- The input must be one the user's form holds: readable and writable for the operation, with `options_fn`,
  not hidden by the conditions on the state, not set by the relation. Otherwise 404, the same as an unknown
  input.
- **The answer is checked**: at most 500 options and 64 KiB; no `null` value; a select's values decoded to the
  column's kind, canonical and storable; no value twice; labels translated. A failure (error, timeout, a bad
  answer) is logged and answered `failed: true`; the panel keeps the declared options.
- Listed in `http::routes`, with cases in `fixtures/authz/admin.toml`.

### Schema and panel

- `InputSchema.optionsFrom: {dependsOn}`; the declared options are sent as they are, and no closure runs while
  a schema is built.
- The panel asks when the form opens and when a dependency changes, debounced (about 400 ms); only the latest
  answer for the current dependency values is applied. A value no longer among the options stays shown,
  disabled, until the user changes it.

### On save (resource create and update; panel, API, import execution)

1. **Triggers.** An input is evaluated when the save could change it: on create, or when the input or one of
   its dependencies is sent. An old record is never refused for an input the save leaves alone.
2. **Replay first.** Admission, then the idempotency claim from the original input, then a **committed-receipt
   lookup** (a new fast path at the create and update entry points, before links, rows and validation): a
   retry of a committed request is answered from its receipt and runs no closure. Concurrent duplicates may
   still run read-only closures; the transactional acquire replays as before if another request committed
   meanwhile. A relation create keeps its replay before the parent lookup.
3. **Preflight, before `begin()`.** The state is built from the record read **without a lock** (update) with
   the request over it, and each triggered input the user may access is run. One save-wide deadline, the
   largest `options_timeout` of those inputs (at most 30 s), covers waiting for a global turn (saves wait up to
   the deadline: no per-user refusal), every closure and the answers' checks; each closure keeps its own
   timeout too. Failure: form error `options_failed` ("could not check the options; try again"), nothing
   saved.
4. **Under the lock.** The state is rebuilt from the locked record with the principal `begin` refreshed. If
   the set of checked inputs or any dependency projection differs from the preflight's: 409 `stale`. An input
   is never checked without an answer.
5. **Membership** of the effective value (accepted, conditions applied; else the stored one) uses the existing
   single/many checker with the evaluated options (`not_option`); an empty value is not checked. The declared
   static options are not checked for that input, but a checkbox list's **shape** is: an array without
   repeats, in saves and in reaction patches, with or without an answer (its own step, not done by clearing
   the rule's options).
6. **No oracle over unreadable values.** A save checks a value the client sent, and an omitted stored value
   only when the user may read the field and view the record (the reaction state's rule). The dependency
   projections compared under the lock hold readable values only. Otherwise the input is skipped: nothing is
   accepted or refused (and no 409 is answered) because of a value the caller cannot observe. Blind updates
   (no `view`) check sent values only.
7. **Imports.** The dry run (no database) skips these checks. The run executes each row as a save; `stale` and
   `options_failed` are retried with the job, as busy writes are (rows already written are skipped by their
   receipts), and reported under their own codes once the attempts run out.
8. No closure ever runs with the write's transaction or a lock held.

## Decision: part D, reactions on relationship rows (vocabulary 27)

The proposal's patches named rows by index or by an untagged id and were checked against what the client
sent. The redesign keeps ADR 0043's rule, "a patch is kept only when the user could make the change", and
checks it against records the server reads for the user.

### State

- `FormState::relation_rows(name) -> &[StateRow]`: the rows of a **top-level** relationship repeater, as the
  form holds them (stored rows with unsaved edits, then new rows, in the form's order). On the wire:
  `state[name] = [{"key": "8", "data": {…}}, {"ref": "n1", "data": {…}}]`.
- **Tagged identities.** `RowId::Key(String)` (a stored row, edit forms only, canonical) and
  `RowId::Ref(String)` (a new row, `^n[0-9]{1,9}$`) never compare equal, whatever their text: a key `"n1"` is
  not the ref `n1`. `StateRow` gives `id()`, `key()`, `reference()`, `get`, `text`, `values`. `RowId` and
  `StateRow` are at the crate root.
- **Only repeaters the user's form holds**: relation readable (`relation_access`), child listable, not hidden
  on the state. At most `[admin] repeater_rows` rows; more, an unknown member, a malformed id or a row given
  twice leave the repeater out of the state.
- In each row only the repeater's inputs, fields the user may read, values that decode, canonical.

### Patches

`FormPatch` gains `add_relation_row(field, data)` (or `add_relation_row_as(field, ref, data)`, naming the new
row for later patches), `update_relation_row(field, id, data)`, `remove_relation_row(field, id)` and
`flag_relation_row(field, id, message)`; `id` is a tagged `RowId` (`{"key": "8"}` / `{"ref": "n1"}`).

A patch is kept only when:
- **Every patch:** the repeater is one the user's form holds, whose rows the user may change, shown on the
  state as the previous patches left it; the id names a row of the state (as the previous patches left it).
- **Keyed patches** name a stored row loaded **read-only, without a lock**, in one query per repeater in key
  order (bounded by the state), through the child's scope (not trashed), parent membership (the bound parent
  key), the relation's filters and the view policy on each. A key not found that way drops the patch.
- **Add:** the relation `creatable`, the child's `create` admitted and allowed with the parent bound (on a
  parent create, without a parent record, as the panel's new-parent form does), `addable`, under `max_items`
  and `repeater_rows`; a ref given must be valid and new.
- **Update:** a ref is checked as an add; a key needs `update` admitted and allowed on that record.
- **Remove:** a key needs `delete` admitted and allowed on the record; keys and refs both honour `deletable`
  and `min_items`.
- **Flag:** a message of at most 500 characters.
- **Values** (adds and updates): each a row input, never the parent key, a filtered field or the order column;
  the child field readable and writable for the operation the save would run (create for a ref, update for a
  key), a value its input accepts (`accept_row_value`), canonical.
- **One cleaner** for adds and updates drops the inputs hidden by the repeater's conditions, in its condition
  mode (legacy: the child form's conditions over its inputs; explicit: the rows' rules, with the record's form
  as `root`). The condition state is built from authorized values: the stored row's readable values (keyed) or
  the child's declared defaults (new), the state's row and the patch over them, then the server's values as
  `create_row` puts them before conditions: the bound parent key (`null` on a parent create) and the
  relation's fixed values.
- **Parent create:** only adds, updates and removals of refs, and flags; keyed patches are dropped (there are
  no stored rows).
- An update that changes nothing is dropped; dropped patches are logged.

ADR 0043's limits (200 patches, one deadline) cover the reaction, the rows it loads and the patch checks.
Nothing is written; the save checks everything again. The save's wire format (`order` with `{"new": ref}`) is
unchanged: tagged ids exist only in the reaction state and patches. The panel maps a patch's ref to a
reference of its own.

### Triggers

A top-level relationship repeater may declare `.reacts()` (boot accepts it; rows' inputs still may not). The
schema says `reacts: true` when the user may change the repeater's rows; the panel asks when rows are added,
removed, reordered or edited, sends each held repeater's rows (stored rows as its widget read them, with
unsaved edits; new rows by ref; a repeater with more stored rows than the widget reads is left out), applies
the four patches by tagged id, and shows a flag under its row.

## Wire

- **Vocabulary 24:** `scope` on leaf conditions; `shownWhen` on relationship repeaters and conditions in their
  rows; `RepeaterSchema.layout`; `RepeaterRelationship.rowLimit`; `RelationSchema.fixed` (`{field: value}`,
  canonical, `null` for `where_null`; kept out of the New form like the parent key); `EntrySchema.rows`.
- **Vocabulary 25:** file uploads and rich editors in JSON rows carry `media`, `collection` and `richNodes` from
  the input; relationship rows may hold the child's media, rich text and `belongs_to` inputs;
  `InputSchema.toolbar`.
- **Vocabulary 26:** `InputSchema.optionsFrom` (`{dependsOn}`); the `form/options` route.
- **Vocabulary 27:** relationship repeaters may carry `reacts`; `form/react` states carry relationship rows by
  tagged id; patches `add_relation_row`, `update_relation_row`, `remove_relation_row`, `flag_relation_row`.
- Issue codes: `too_many_rows` (new message), `too_many_media` (new), `options_failed` (new); 409 `stale`
  (reason `options`).
- An older SPA sees an unknown vocabulary and shows forms read-only (ADR 0002).

## Consequences

- Applications upgrading from 0.13 keep their rows' behaviour: legacy mode keeps the child form's conditions,
  hidden repeaters keep their rows, imports and API updates never delete rows.
- Relationship repeaters hold 50 rows by default where 0.13 read 100 and reordered 1,000; larger ones raise
  `[admin] repeater_rows` (upgrade guide).
- Filtered relations are filtered everywhere the relation is read, and need their filtered fields readable
  (and writable on create to create through them). A field that is `guarded()` can filter a relation that is
  read and edited, not created through.
- Media references in JSON rows are checked (existing, visible, accepted type and size) on every write through
  the panel, the API and imports; JSON written by other code (hooks, migrations) is not. Media held only in JSON
  rows have no foreign key: deleting the media leaves the row showing it as unavailable.
- Rust API: `Condition` leaves gain `scope`; `ListQuery` gains a hidden field; `DynResource` (implemented by
  the engine only) changes signatures and gains methods; `FormPatch` (not `#[non_exhaustive]`) gains four
  variants, a source break for exhaustive matches; `SelectOption`, `RowId` and `StateRow` are at the crate
  root. Listed in the upgrade guide.
- `options_fn` adds a read-only round trip to the application before a save that could change the input, and
  a 409 `stale` when a dependency changed between the preflight and the lock; imports retry such rows.
  Idempotent retries of a committed create or update are now answered before validation.
- Row reactions read stored child rows for the user (one query per repeater) while the user types; they are
  bounded by the reaction limits and write nothing.

## Not accepted, or later

- **Part C, nested relationship repeaters (three levels), `inherit`, chained `lock_parent`:** not accepted.
  `lock_parents` locks only the direct parent and the recursive write orders locks within each level only, so
  there is no global lock order; ADR 0018's "no chains" stays. To be redesigned (locks and the delete, trash
  and restore lifecycle) if a second application needs it; meanwhile the third level is edited on its own
  page.
- **Part D, `validate_rows`:** not accepted in its form. It hands application code every row with every field,
  rows the user may not see included (a privileged API), and cannot hold a real invariant: a write of the
  parent alone, or of the child table directly, goes around it. Such invariants ("one correct option") belong
  in the application's hooks or services.
- **Part E, `shown_when_fn` and `required_fn`:** not accepted (`when(..)`, `required` and validation hooks
  cover them); the `form/rules` route returning every rule is not accepted. `options_fn` is accepted as
  decided above.
- **Reactions declared on row inputs** (`.reacts()` inside rows), and `options_fn` in rows, on settings pages
  and in action forms: not in 0.14.
- **Deleting the rows of a hidden relationship repeater:** not accepted (above); an opt-in per repeater may be
  considered later.
- **String `where_eq` filters:** later, with an exact comparison across databases.
