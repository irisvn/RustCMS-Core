# ADR 0044: Repeaters, second round

- Status: accepted for 0.14.0 (vocabularies 24 and 25): parts A and B, reworked after review. Parts C, D and E
  were not accepted (see "Not accepted, or later").
- Date: 2026-11-18 (proposed); 2026-11-23 (decided)
- Contributed by @giangittb112000 (PR #4). Decisions: `docs/core/plans/repeater-v2-decisions.vi.md`; plan of the
  rework: `docs/core/plans/contrib-0.14-part6.md`; review: `docs/core/reports/2026-11-22-pr4-review.vi.md`.
- Amends ADR 0037 (conditions in rows reading outer levels), ADR 0040 (repeaters) and ADR 0041 (points 4, 5 and
  6). ADR 0018 (no `lock_parent` chains) and ADR 0043 (form reactions) are unchanged.

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
| D | `validate_rows`, reactions on relationship rows | not in 0.14 |
| E | Server closures (`shown_when_fn`, `required_fn`, `options_fn`), `form/rules` | not in 0.14 |

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

## Wire

- **Vocabulary 24:** `scope` on leaf conditions; `shownWhen` on relationship repeaters and conditions in their
  rows; `RepeaterSchema.layout`; `RepeaterRelationship.rowLimit`; `RelationSchema.fixed` (`{field: value}`,
  canonical, `null` for `where_null`; kept out of the New form like the parent key); `EntrySchema.rows`.
- **Vocabulary 25:** file uploads and rich editors in JSON rows carry `media`, `collection` and `richNodes` from
  the input; relationship rows may hold the child's media, rich text and `belongs_to` inputs;
  `InputSchema.toolbar`.
- Issue codes: `too_many_rows` (new message), `too_many_media` (new).
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
  the engine only) changes signatures. Listed in the upgrade guide.

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
- **Part D, reactions on relationship rows:** later, with a design of its own (fields readable and writable,
  per-row permissions, reaction limits, patches by key or ref) and a review. ADR 0043 is unchanged.
- **Part E, server closures:** `shown_when_fn` and `required_fn` are not accepted (`when(..)`, `required` and
  validation hooks cover them). `options_fn` (options depending on another input) is later, with a design of
  its own: run before the transaction and locks, under ADR 0043's limits, one input at a time; the
  `form/rules` route returning every rule is not accepted.
- **Deleting the rows of a hidden relationship repeater:** not accepted (above); an opt-in per repeater may be
  considered later.
- **String `where_eq` filters:** later, with an exact comparison across databases.
