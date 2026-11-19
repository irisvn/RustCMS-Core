# ADR 0044: Repeaters, second round

- Status: accepted (unreleased, vocabularies 24 to 28), parts A to E.
- Date: 2026-11-18
- Amends ADR 0037 (conditions inside rows), ADR 0040 (repeaters), ADR 0041 (one phase per child resource; in
  part C, a locking pass per parent row), ADR 0018 (in part C, chains of parent locks) and, in parts D and E,
  ADR 0043 (form reactions).

## Context

ADR 0040 and 0041 shipped repeaters with limits that were left "for later". Relationship repeaters had no
conditions, at most one repeater per child resource, no rich text, media or `belongs_to` in rows, one level only,
and nothing on view pages. JSON rows took no media, rich text or record references. Conditions in a row could
read only that row.

The case that drives this round is a question bank with a schema of three tables, which the application will
not change to work around the framework:

- `questions` → `question_blanks` → `question_options`;
- a choice question's options are the options with `blank_id` null;
- a blank's choices are the options with its `blank_id`;
- a double foreign key `(blank_id, question_id)` references `question_blanks(id, question_id)`.

The question's form should edit all of it at once:
- the options or the blanks, shown according to the question's type;
- the choices of each blank;
- rich text and images in rows;
- a view page that shows the options under the question.

The constraints of ADR 0040 and 0041 still hold:
- the server decides on every path;
- a relationship repeater's value is a change, not a set;
- every child row goes through the child's scope, policy, field rules, hooks and history;
- explicit locks follow `lock_rank`, media last;
- any error rolls back the whole write.

The work is split into four parts, each its own release step, decided in order:

| Part | Content | Vocabulary |
|---|---|---|
| A | Conditions on and in relationship repeaters, conditions reading the root form, filtered has-many relations, table layout, relationship rows on view pages | 24 |
| B | Server rules of each input applied in every row of every repeater; media, rich text and `belongs_to` in rows (JSON and relationship) | 25 |
| C | Nested relationship repeaters (three levels), `inherit`, chained locks | 26 |
| D | `validate_rows`, reactions on relationship rows | 27 |
| E | Server closures for visibility, options and required | 28 |

Decisions taken with the owner (2026-11-18), by number in the plan:

- **Q1.** A relationship repeater hidden by its condition has its rows deleted on save.
- **Q2.** Relationship repeaters nest three levels at most.
- **Q3.** Filtered relations let one child resource back several repeaters.
- **Q4.** Rows use a compact rich-text editor.
- **Q5.** Server closures come last.
- **Q6.** Breaking changes are accepted, with an upgrade guide.
- **Q7.** Ancestors' keys are filled in by the framework.

## Decision: part A

### Conditions on a relationship repeater (Q1)

- `shown_when` is accepted on a relationship repeater and on the layouts around it, under ADR 0037's rules.
- No condition may name the repeater (it holds a change, not a value), as before.
- **Hiding deletes rows, but only when the write hides the repeater.** The repeater must be shown before the
  user's changes and hidden after them (for example, a question switched from "single choice" to "fill in the
  blank"). Both states are evaluated on what the user's form holds (ADR 0037, "Values the conditions read").
  In that case the parent's write deletes every row of the relation that the user can see:
  - **What is deleted.** The live children of the parent that pass the child's `lock_visible` (scope and view
    policy, rechecked under the lock), at most 1,000 (422 `too_many_rows` beyond, as an order). Rows the user
    cannot see are kept.
  - **Permission.** Each row needs the child policy's `delete`; one refusal is a 403 and nothing is saved,
    the parent included. With no visible row, nothing is checked.
  - **Soft deletes.** Rows go to the trash when the child has soft deletes, as a repeater's `delete` part.
  - **History.** Each row records its own entry. The parent's `rows_changed` entry counts the deleted rows.
- **It deliberately differs from other inputs.** A hidden field keeps its value unless it has
  `clear_when_hidden`; JSON repeaters keep that rule too. A relationship repeater has no option to keep its
  rows: children that the form no longer shows would be edited nowhere and still be read by the application.
- **Only on the transition, like `clear_when_hidden`.** A repeater that was already hidden before the write
  deletes nothing: rows added meanwhile from the relation section or an import stay, and editing another input
  does not remove them. A user who cannot read the input the condition names sees it as `null` before and
  after, so their write never deletes anything.
- **What the client sends is dropped.** Changes sent for a repeater hidden after the write are ignored,
  whether hidden before or not, as values of hidden inputs are (an old SPA, or a draft from before the
  switch). No 422. Admission of the parts sent still runs, before anything is read: `create`, `update` and
  `delete` on the child (403).
- **On create**, the parent has no rows: a hidden repeater writes nothing, and its `min_items` does not
  apply.
- **`min_items` applies only while shown.** A repeater hidden after the write is not checked against
  `min_items`; deleting everything is the intended outcome.
- **Boot refuses:**
  - `clear_when_hidden` on a relationship repeater: it would be redundant.
  - a condition on a repeater with `deletable(false)`, its own or one of the layouts around it: hiding it
    would delete rows the repeater forbids deleting.
- **Panel:**
  - The panel hides the repeater and keeps its unsaved changes (shown again, they are back).
  - When the user's changes hide a repeater that was shown when the form opened and holds saved rows, the panel
    says so in its place: "Saving removes the N rows of {label} you can see" (N from the relation's list).
  - The panel does not send the hidden repeater's changes.

### Conditions in rows, and the root form (both kinds of repeater)

- **Builder.** `when(name)` keeps naming an input of the same row (or of the form, outside rows). Two new
  starters name inputs outside the row:
  - `when_root(name)`: an input of the record's form, the top level, whatever the depth;
  - `when_parent(name)`: an input of the row one level up. It is refused at boot until part C, since in part
    A no row has a parent row. For a JSON repeater nested in a JSON row it is accepted, and reads the
    enclosing row.

  The operators are those of `when`.
- **Wire.** Each leaf condition (`eq`, `ne`, `is_in`, `filled`, `is_true`) gains `scope`:
  - `"row"` is the default, never sent;
  - otherwise `"parent"` or `"root"`.

  At the top level of a form, `when_root`/`when_parent` are refused at boot (there `when` already reads the
  form).
- **Boot checks** follow ADR 0037: the named input exists at that level and has a kind conditions can read;
  values decode as that kind; there is no cycle (dependencies are ordered within each level). A row input
  never depends on itself through the root: the root does not read rows.
- **Meaning.**
  - A root or parent input that is hidden counts as `null`, as in ADR 0037.
  - The root's values are what the user's form holds after the write (stored values, readable fields only,
    with the user's input over them).
  - A row is evaluated after the root's own conditions.
- **JSON rows.**
  - `accept_input` still checks every value sent in every row: kind, length, options, bounds, even under a
    condition.
  - Dropping hidden inputs and "required when shown" move to the conditions step, once the root's values are
    known (after the lock on update).
  - Rows whose conditions read only the row behave as before.
  - Settings pages evaluate the root on the page's values; action forms on the values sent.
- **Relationship rows.**
  - Rows follow **the repeater's conditions only** (declared in its `.schema()`). The child form's
    conditions no longer apply to repeater rows: the user edits the row the repeater shows, not the child's
    form. This changes behaviour, see "Consequences".
  - A stored row is evaluated on its values when the user may read the field and view the row (it is locked
    through `lock_visible`, so it is viewable), with the row's data over them.
  - A new row is evaluated on the child's declared defaults.
  - Hidden row inputs are dropped from the row's data.
  - A shown input whose child field is required (or whose column is not nullable) is "is required" by ADR
    0037's rules for create and update.
  - The child's non-nullable columns without a default may not have conditional row inputs (boot, as ADR
    0037).
- **`clear_when_hidden`** stays refused inside rows of both kinds. A JSON row is rewritten anyway; for a
  relationship row the question is left for later.

### Filtered has-many relations (Q3)

- `Relation::has_many(name, target, field).where_null(column)` and `.where_eq(column, value)` restrict the
  relation to the target's records matching every filter (AND).
- **Boot checks:**
  - `column` is a field of the target, not the parent key, not the primary key, not an order column;
  - `where_null` needs a nullable column;
  - the value of `where_eq` decodes as the column's kind;
  - filters are refused on belongs-to-many relations.
- **Reads.** The filter applies wherever the relation is read:
  - the relation section's list and its count;
  - `counts(relation)` columns;
  - the repeater's rows (its reads and locking pass);
  - `RepeatableEntry` rows (below).

  The child resource's own pages are not filtered.
- **Writes through the relation set the filter.**
  - A row created by a repeater or from the relation section ("New" with the parent bound) gets the filter's
    values from the server, as the parent key: written after `accept_input`, so the user needs no write
    rule on the column.
  - A hook that changes them is a 500, as for the parent key.
  - A filter column is never a row input (boot), so a client that sends it gets 422 (`is not allowed`).
  - A row updated through the repeater cannot change it.
  - A key outside the filter is "is not one of the rows" (422), as a key of another parent.
  - New rows of an ordered repeater take positions after every row of the parent, whatever its relation, so a
    unique `(parent, position)` holds across relations; an order renumbers only the relation's rows.
- **Several repeaters on one child resource** are accepted when their relations cannot share a row.
  - **What counts as disjoint.** Some column must be filtered by both, by `where_null` on one side and
    `where_eq` on the other, or by `where_eq` with two different values. Anything else is refused at boot.
  - **Locks** (ADR 0041, point 4, amended). Each repeater's rows keep their own locking pass, the repeaters of
    one child in relation order, after children of lower rank. That is safe without a single pass over their
    union: every writer of those rows locks the parent first (`lock_parent`, point 3), so two writes that could
    lock them in another order already wait for each other on the parent row. The lock graph is unchanged.
  - History records one `rows_changed` entry per relation.
  - The parent's `after_save` sees every row of every repeater.

### Table layout

- `.table()` on a repeater (JSON or relationship) lays each row out as a table row: one column per input, the
  row's buttons last.
- Boot refuses `.table()` when a row holds layouts (`Section`, `Grid`) or a nested repeater, as an input that
  does not fit a cell would be (part B adds rich text and file uploads to that list).
- Schema: `RepeaterSchema.layout` is `"table"`, absent for the default (stacked rows).
- **Panel:**
  - from the `md` breakpoint, a `<table>` whose headers are the inputs' labels;
  - below `md`, stacked rows with labels, no horizontal scroll;
  - an input hidden by its condition leaves its cell empty and keeps the column;
  - errors show under the cell.

### Relationship rows on view pages

- `RepeatableEntry::make(relation).relationship().schema(components![…])` shows a has-many relation's records,
  filtered as above, each with entries of the child's fields.
- **Reading.** The panel reads them through the relation's list (`GET …/relations/{rel}`), with the child's
  scope, view policy and read rules. No new route.
  - Order: the list sorted by the repeater's order column when the form has one (`order_column`) and the user's
    lists may sort by it (`sortable()`), else the list's own order. The server sorts, so the first `limit` rows
    are the first by position.
  - Conditions in rows read the row and, with `when_root`, the parent record's stored values.
  - At most `limit` rows (100), then a link to the relation section.
  - A user who may not list the child (`view_any`) does not see the entry; there is no error.
- **Checks.** Entries name fields of the child that are readable (an unreadable one is left out per user, as
  columns are); boot checks that the relation exists and is has-many.
- **Default view page.** A view page built from the form (`Infolist::from_form`) shows a relationship
  repeater as such an entry, with its rows' inputs as text entries.
- The existing `RepeatableEntry` of a JSON field is unchanged.
- ADR 0040 already described relationship entries (`limit`, default 10), but they were never built: the boot
  refused them. They are built here, with `limit(n)` at most 100 and a default of 100 rather
  than 10 (decided 2026-11-18).

### Wire (vocabulary 24)

- `Condition` leaves: `scope` (`"parent"`, `"root"`; absent: the row, or the form).
- `RepeaterSchema.layout`: `"table"`, absent for stacked rows.
- Relationship repeaters' `InputSchema.shownWhen` is accepted, and so are conditions in their rows.
- `EntrySchema` of a `RepeatableEntry` gains `relationship: {resource, relation, orderBy?}`.
- An SPA on vocabulary 23 sees vocabulary 24 as unknown and shows forms read-only (ADR 0002).

## Consequences (part A)

- **Rows deleted on hiding.** Applications must not hide a relationship repeater on a value whose change
  should keep the rows. For example, the blanks' choices hold the accepted answers of free-text blanks too,
  so "choices" must not be hidden by the input mode. The guide says so.
- **Child-form conditions stop applying to rows (breaking).** An application that relied on the child's
  `shown_when` dropping a row input now declares the condition in the repeater's `.schema()`. The upgrade
  guide lists it.
- **`Condition` variants gain a `scope` member (breaking).** Applications that build or match variants by hand
  need it. `when`, `Condition::all` and `Condition::any` are unchanged.
- **Filtered relations are filtered everywhere the relation is read.** A relation section of options with
  `where_null("blank_id")` no longer shows a blank's choices.
- **Several repeaters on one child** keep the lock order of ADR 0041: their rows are locked under the parent's
  lock, which serializes every writer of them.

## Decision: part B (vocabulary 25)

Part B lets rows hold what references other records: files, rich text (whose images are files) and, in
relationship rows, `belongs_to` selects. The constraint is ADR 0041's: media rows are locked last, once, after
every other explicit lock of the write.

### Rules of row inputs (JSON rows)

- The rules of a JSON row's inputs are declared on the inputs, as before. The kinds of rule grow, still
  inside the row compiler (no new trait):
  - **`FileUpload`** holds one file id, or a list of ids with `.multiple(max)`. It takes `.accept([...])`,
    `.max_size(bytes)` and `.collection(name)` on the input; these are allowed on inputs of JSON rows only, as
    `required` and `max_length` are.
  - **`RichEditor`** holds a document, checked as a rich text field's (versions, bounds, `MAX_BYTES`). It
    takes `.nodes([...])` for application nodes, checked at boot against the panel's declared nodes.
- **Checking.** `accept_input` checks each row's values: a file input holds ids, a document is a valid
  document. Media are checked after the hooks with the record's own media (below), never earlier.
- **Still refused in JSON rows:**
  - `belongs_to` selects: a JSON row has no field for an options route, and the id would have no foreign key;
  - secrets and belongs-to-many inputs;
  - tabs and splits.
- **Settings pages** take the same inputs in their JSON rows; their media are checked with the page's own.

### References gathered, checked once (both kinds of rows)

- **One media check per write.** A write's media references are gathered from every place:
  - its media and rich text fields;
  - the rows of its JSON repeaters, at any depth;
  - the rows its relationship repeaters write, after the child's hooks.

  They are checked once, after every other lock of the write, by `media::check_references`: shared locks in
  id order, as before (ADR 0041 point 2). Each reference keeps its path for errors: `faq.2.icon`,
  `options.n1.image`, `options.8.content`.
- **ADR 0041 point 5 is amended.**
  - A relationship row's media and rich text fields are accepted (boot). The row write gathers its
    references instead of refusing them; they are checked with the parent's, last. A hook adding media to a
    row is no longer an error.
  - Encrypted and belongs-to-many fields stay refused in rows.
- **`belongs_to` in relationship rows (R7).**
  - A row's `belongs_to` field is checked as a form's: the target must be visible to the user
    (`keys_in_scope`), read without an explicit lock, after the child's hooks.
  - The database's foreign key takes its implicit lock. A cycle there is ADR 0041's 409 `write_conflict`, as
    for every form's `belongs_to`.
  - The parent key stays out of rows. A child field that `lock_parent`s another resource is not an input of
    the rows: every row write locks only the repeater's parent.
  - The panel's select reads its options from the child's options route (the child's field rules and scope),
    as on the child's own form.
- **A parent with relationship repeaters still cannot link media records** (belongs-to-many to media): its
  link targets would lock media before its rows.

### Rich text in rows

- **A compact editor by default** (Q4). In a row of either kind, a `RichEditor` shows a compact toolbar: bold,
  italic, underline, superscript, subscript, images and the field's application elements. `.full()` gives
  the full toolbar.
- Schema: `InputSchema.toolbar` is `"compact"` (absent: full).
- **The toolbar is presentation.** The server checks the document by the field's rules (or, in JSON rows, the
  input's); content pasted from elsewhere (a heading, a table) is kept when those rules allow it.
- Uploads in a row's editor go into the field's collection (`Field::collection`, or the input's in a JSON row).

### Panel

- **Uploads and saving.** An upload in progress in any row blocks saving. The work is tracked per row
  (`lines.n1.image`, `faq.2.icon`), so a row's finished upload does not release another's.
- **Rows keep their state.** A row's editor content and uploads follow the row through additions, removals,
  moves and hiding, keyed by its identity (key or ref, JSON row keys). Leaving the page with unsaved row
  changes asks first, as for any input. Nothing is stored in the browser.
- **Table layout** refuses file uploads and rich text, which do not fit a cell (boot).
- **Media pickers in rows** open on the field's (or input's) collection.

### View pages

- JSON rows' entries may be `MediaEntry` and `RichTextEntry`; the default view page shows a row's file
  uploads and rich text with them. Relationship rows already fit their entries to the child's fields.

### Wire (vocabulary 25)

- `InputSchema.toolbar`: `"compact"` on a rich editor in a row (absent: full).
- File uploads and rich editors in JSON rows carry `media` (accept, size, how many) and `collection` from the
  input's rules, as inputs of fields do from the field.
- `RepeaterSchema` of a JSON repeater may hold `file_upload` and `rich_editor` inputs; relationship rows may
  hold them and `select` inputs of `belongs_to` fields (`relationship` set).
- An SPA on vocabulary 24 sees vocabulary 25 as unknown and shows forms read-only.

## Consequences (part B)

- **Media references in JSON rows are checked** (existing, visible, accepted type and size) on every write
  through the panel, the API and imports. JSON values written by other code (hooks, migrations) are not.
- **A write's media check covers its rows.** Errors are at the row's path; one unavailable file in a row
  refuses the whole save, as in a field.
- **Media held only in JSON rows have no database foreign key.** Deleting a media record does not clear a
  row that names it; the row shows it as unavailable, as a rich text image does today.

## Decision: part C (vocabulary 26)

Part C lets a relationship row hold repeaters of its own: relationship repeaters of the child's relations (a
question's blanks, each with its choices) and JSON repeaters of the child's JSON fields. Decisions taken with
the owner on 2026-11-19 are marked (C1–C7).

### Nesting

- **What a relationship row may hold.**
  - A relationship repeater over a has-many relation **of the row's resource** (the child), declared in the
    child's `relations()`. The relation may be filtered.
  - A JSON repeater over a JSON field of the child (C6). Its rules are declared on its inputs, as in a form
    (part B), and checked on every write through the repeater. The child's own form may declare other rules
    for that field: each form checks its own rows, as two forms of one resource already may.
- **Depth.** Three levels of repeaters below the form at most (Q2), whatever their kind: `MAX_DEPTH`, shared
  with JSON repeaters. A JSON repeater in a relationship row counts as one level.
- **Boot refuses:**
  - a nested relationship repeater whose relation is not a has-many of the row's resource, or a belongs-to-many;
  - a nested child that is also the resource of an enclosing level (a resource editing itself through its rows);
  - `.table()` on a repeater whose rows hold a repeater (as in part A);
  - two nested repeaters in one row on the same child resource whose relations may share a row (part A's
    disjointness rule);
  - everything refused in rows today (`clear_when_hidden`, a `deletable(false)` repeater under a condition,
    rules of the row's inputs, the parent key, filter or order columns as inputs).

### Keys of ancestors: `inherit` (Q7)

- `Repeater::make("choices").relationship().inherit("quiz_question_id")` on a nested relationship repeater
  names a field of its child that the server fills with an **ancestor's key**: the record's (the form's
  level) or an enclosing row's.
- **Boot checks:**
  - the field is `belongs_to` the resource of exactly one enclosing level, with that level's key type;
  - it is not the relation's field, a filter column, the order column or a row input;
  - `inherit` is refused on top-level repeaters and on JSON repeaters.
- **The parent key of a nested row is the server's too**: written after the row's input is accepted, so the
  child may keep it `guarded()` (a blank's key on its choices, which clients never move).
- **On create** the value is the ancestor's key once that ancestor is written (a new row's key is known
  before its rows are written). The user needs no write rule on the field. A client sending it gets 422 (`is
  not allowed`, as any undeclared input); a hook changing it is a 500, as for the parent key.
- **On update** the field is never written by the repeater. A stored row whose inherited field does not hold
  the ancestor's key is not one of the rows (the relation's rows are those of the parent row; the database's
  double foreign key, if any, keeps them consistent).
- A required ancestor key that is not inherited is not detected at boot: the row's create fails with the
  child's `is required`, at the row's path.

### Locks (ADR 0018 and ADR 0041 point 3 amended) (C1)

- **The family serializes on the root.** Every writer of rows at any level holds, before locking them, a row
  that every other writer of that family also locks. For a nested repeater, the child's `lock_parent` must go
  to one of:
  - **its direct parent, through the nested relation's field** (a chain: the parent resource locks its own
    parent, as a top-level repeater's child does). ADR 0018's "no chains" is lifted: a `lock_parent` target may
    declare its own. A write still locks its direct parent only; ranks put each resource after the one it
    locks, and the lock graph stays acyclic (checked at boot). The demo's section links lock their section,
    which locks its category.
  - **an ancestor of the repeater, through an inherited field.** The LMS quiz's choices lock the question
    through `quiz_question_id`, as its top-level options do: the options table is edited by the question's
    `options` repeater and by the blanks' `choices`, and one resource has one `lock_parent`.

  Anything else is refused at boot.
- **Lock graph.** For each nested repeater, the outer child gains an edge to the nested child (as a parent to
  its `lock_parent` children): a write locks the blanks before their choices, and a question's phases run in
  rank order (blanks and their choices before the options, when the ranks say so).
- **Locking passes.** ADR 0041 point 6 becomes one locking pass **per parent row and relation**: the named rows
  of one row's nested change (and its live children when it orders them, at most 1,000), in key order, under
  the lock of that row (locked by its own level's pass, or written by this write). Passes of different parent
  rows run in the order the rows are written, so keys of one child resource are not in key order across them.
  That is safe for the reason of part A's several repeaters: every writer that locks rows under more than one
  parent row holds the root (the root's write) or the ancestor every child locks, so such writers already
  wait for each other on that row.
- Media stay last, once, for the whole write (part B).

### Wire (C7: refs)

- A row's `data`, in `create` and `update`, may hold a nested repeater's change under the repeater's name,
  with the shape of ADR 0040 (`create`, `update`, `delete`, `order`; `clear` is the server's own).
  - In a `create` row, the nested change holds `create` (and `order`) only, as a create form's.
  - An `update` row may carry only a nested change (its own inputs unchanged).
- **Refs** stay unique within one change: a nested change belongs to the row it is in, so no ref names another
  level's row. The panel keeps them unique in the page (`n1`, `n2`, …) for its own tracking.
- **Shape checks** recurse: admission of the parts sent at every level (`create`, `update`, `delete` on each
  child, 403) before any key is read, then the shape (422 at the repeater's path).
- **At most 1,000 rows per write.** Every row a write creates, updates or deletes counts, at every level,
  including rows removed with their parent row or because a repeater was hidden. Beyond: 422
  `too_many_rows_changed` ("changes too many rows at once") on the form, nothing saved. Each part of one change
  still holds 100 entries at most.
- **Error paths** name each level's row by key or ref: `blanks.n1.choices.n2.content`,
  `blanks.8.choices.12.content`; a nested repeater's own errors at `blanks.8.choices`.

### Writing a level

For one relationship repeater of one parent row (the record, or a row written above):

1. **Lock** the named rows (and live children to order) in one pass, rechecked under the lock (part A).
2. **Delete** rows, each with its nested rows first (C2):
   - for each relationship repeater the row declares, the row's live children through that relation are
     locked through `lock_visible`; when one of them is not visible to the user, the write is a 403 and
     nothing is saved: the framework never removes rows the user cannot see, and never relies on
     `ON DELETE CASCADE`;
   - each child is deleted under the child's policy, with its history (to the trash with soft deletes),
     bottom level first, then the row;
   - relations the repeater does not declare, and the row's children outside a filtered relation, are left
     to the database (its foreign keys: cascade, restrict, or a trashed row's children kept live), as for any
     delete of that row.
3. **Update** rows: the row's inputs (accepted, conditions, hooks, write), then its nested changes with its
   key, then its `after_save` and history (C3).
4. **Create** rows: the row (the parent key, inherited keys and filter values from the server), then its
   nested changes with its new key, then its `after_save` and history.
5. **Bounds** (`min_items`, `max_items`) on the parent row's rows, then the **order**.

New rows of an ordered nested repeater take positions after every row of the parent row and every row sharing
an ancestor key the repeater inherits (trashed included), so a unique `(question, position)` holds across the
blanks; an order moves rows among the places they hold, as in part A.

A row's `after_save` sees its nested rows, as the record's sees its rows; it runs after them, bottom level
first. A row whose inputs did not change but whose nested rows did still runs its `after_save`.

### Conditions across levels

- `when_parent(name)` in a relationship row of a nested repeater reads the enclosing relationship row: its
  values as this write leaves them (stored values the user may read, the row's change over them, after its own
  conditions; a new row's declared defaults with its data). In rows of a top-level repeater it stays refused
  (the parent is the form: `when_root`).
- `when_root(name)` reads the record's form at any depth.
- In a JSON repeater inside a relationship row, `when_parent` reads the relationship row and `when_root` the
  form.
- **A nested repeater's own condition (or its layouts') reads its row only** (`when`; boot refuses `when_root`
  and `when_parent` there): it is settled when its row is written, and a row the save does not touch would
  otherwise keep rows a change of the form hides (review, 2026-11-19).
- **Hiding a nested repeater** (its condition in the row, or a layout's) follows part A at the row's level:
  shown before and hidden after on the row's values, its visible rows are removed (under the 1,000 rows of the
  write); changes sent for a hidden nested repeater are dropped; `min_items` does not apply while hidden. A
  removed row of a hidden repeater removes its own nested rows, as a deleted row does (step 2).

### History

- Each row records its own entries, at every level.
- A row whose nested rows changed records a `rows_changed` entry per nested relation (counts only), when its
  resource is audited. The record's `rows_changed` counts its direct rows only.

### Panel

- **Nested rows are a tree.** A row's nested relationship repeater holds its own state (rows, removed keys,
  order) as the form's does; saving sends it inside the row's change (only when something changed).
- **Reading (C4).** When the form opens, each stored row reads its nested rows through the row's relation
  list (`GET …/{child}/records/{key}/relations/{relation}`, filtered, sorted by the order column), one request
  per row, at most 100 rows each (beyond: read-only, as today). New rows start with the nested repeater's
  `default_items`. No new route.
- **Removing a row** that holds nested rows removes them on save; the hidden-repeater notice of part A says
  so when the hidden repeater's rows hold repeaters, counting the nested rows the form read ("…, and the 4
  rows inside them"; uncounted when some were not read). A nested repeater its row's condition hides (shown
  when the form opened) says in that row how many of its rows saving removes.
- **Errors** at nested paths show under the nested row's input; uploads and editors in nested rows are tracked
  by their full path (part B).
- **Limits on the client** (`min_items`, `max_items`, row problems) are checked per level, as the server does.

### View pages (C5)

- A row of a relationship entry may hold a relationship entry of the child's has-many relations (checked at
  boot with the target's fields, three levels at most), read through the row's relation list (`GET
  …/{child}/records/{key}/relations/{relation}`, one request per row, the list's order), left out for a user who
  may not list its target; more rows than `limit` link to the row's page. The default view page shows nested
  relationship repeaters this way (2026-11-20, after part E).

### Wire (vocabulary 26)

- `RepeaterSchema.components` of a relationship repeater may hold relationship repeaters (their
  `relationship.resource` the nested child, `relation` a relation of the row's resource) and JSON repeaters.
- Conditions in those rows may have `scope: "parent"`.
- New error code `too_many_rows_changed`.
- An SPA on vocabulary 25 sees vocabulary 26 as unknown and shows forms read-only.

## Consequences (part C)

- **Deleting a row deletes its nested rows** through policies and history; a user who cannot see one of them
  cannot delete the row (403). A row whose nested repeater is not declared in the form keeps the database's
  behaviour.
- **Trashing a row trashes its nested rows**: restoring the row from its own page does not restore them.
- **ADR 0018's "no chains" is lifted.** A `lock_parent` target may have its own; resources that relied on the
  refusal see no change, and a chain's middle resource ranks between its two ends.
- **Hooks of a row's `after_save`** now run after its nested rows are written.
- **A JSON repeater in a relationship row** checks the rules the row declares, not the child form's; the
  child's own form checks its own.
- **Large trees.** A form whose rows hold many nested rows reads one request per row when it opens.

## Decision: part D (vocabulary 27)

Part D lets an application keep an invariant over a repeater's rows (a single-answer question has exactly one
correct option) and lets form reactions edit relationship rows. Decisions taken with the owner on 2026-11-19
are marked (D1–D6). Server closures, planned here, are part E (D1).

### `validate_rows` (R8)

```rust
Repeater::make("options")
    .relationship()
    .validate_rows(|_ctx, check| {
        let correct = check.rows().iter().filter(|r| r.get("correct") == Some(&json!(true))).count();
        match check.parent().get("kind").and_then(|k| k.as_str()) {
            Some("single") if correct != 1 => Err(RowsError::list("must have exactly one correct option")),
            _ => Ok(()),
        }
    })
```

- **Where (D4).** On relationship repeaters at any level, and on JSON repeaters of a resource's form (its top
  level). Boot refuses it on JSON repeaters nested in rows, on settings pages and in action forms (later).
- **A plain function (D5)**: `Fn(&Ctx, &RowsCheck) -> Result<(), RowsError> + Send + Sync`. It reads what it is
  given; it does not query the database inside the write.
- **What it reads.** `RowsCheck` holds:
  - `operation()` (`Create` or `Update` of the record);
  - `parent()`: the values of what holds the rows as this write leaves them: the record's stored values after
    its row is written (a relationship repeater of the form), the enclosing row's (a nested repeater), or the
    form's values as the save stores them (a JSON repeater);
  - `rows()`: the final rows, in order, each with `id()` and `get(name)`/`values()`.
- **The final rows of a relationship repeater (D2)** are every live row of the relation (filtered, not
  trashed), read after the repeater's rows are written, **including rows the user cannot see**: the invariant
  holds on the data, not on one user's view. The read takes no lock of its own: every writer of those rows
  locks the parent first (ADR 0041 point 3, part C). Rows come in the order column's order (else key order),
  with every field of the child except encrypted ones. A row's `id()` is the ref of a row this save created, its
  key otherwise. **The function must not put values of rows the user cannot see in its messages** (the guide
  says so); it is application code, as hooks are.
- **The final rows of a JSON repeater** are the list as the save writes it, after the `mutate_before_*` hooks
  (hidden inputs left out), ids being row indexes; `parent()` is the stored record with what the save writes
  over it (review, 2026-11-19: it first ran before the hooks, on the form's values).
- **When (D3).**
  - On create, when the repeater is shown, whether or not rows were sent (a question created without options
    meets "exactly one correct" too).
  - On update, when the save changes the repeater's rows: a relationship repeater whose change created,
    changed, removed or reordered rows; a JSON repeater whose value was sent.
  - Never for a hidden repeater (its rows are removed, or kept untouched), and not on saves that leave the rows
    alone: an old record whose rows break a newer rule stays editable. A save changing only the parent (a
    question made single-answer) does not run it either: an application keeps such a rule on that path in the
    parent's `mutate_before_update` (review, 2026-11-19, kept as decided in D3).
  - On every path that writes through the form's input: the panel, the API, imports. Writes to a child on its
    own page do not run its parent's repeater rules (as `min_items`, ADR 0041).
- **Order in the write.** A relationship repeater's check runs after its rows are written (and, nested, after
  the rows of each of them), before its parent's `after_save`; a JSON repeater's with the form's conditions,
  before the hooks. A failure rolls back the whole write.
- **Errors.** `RowsError::list(message)` at the repeater (`options`), `RowsError::row(id, message)` at a row
  (`options.n1`), `RowsError::input(id, name, message)` at a row's input (`options.n1.correct`); `.and(…)`
  gathers several. Nested, the path holds every level (`blanks.4.choices`). Messages are the application's,
  translated with `ctx.t()` by the application, code `custom` (as other application validation).
- **`before_row_write` is not added (D6).** The child's `mutate_before_create`/`update` already run for every row
  on every path, and `validate_rows` keeps the set's invariant: a repeater's own row hook would be a second
  place changing the same row.

### Reactions on relationship rows (R9, amends ADR 0043)

- **Top-level relationship repeaters only (D6).** Rows of nested repeaters are left for later.
- **State.** The panel sends a relationship repeater's rows as it holds them (stored rows it read, with the
  user's unsaved changes; new rows by ref): `state[name] = [{"key": "8", "data": {…}}, {"ref": "n1", "data": {…}}]`,
  each row's data its inputs' values in wire format. A repeater with more rows than the panel edits (100) is
  read-only there: the panel does not send it, and a request sending more leaves it out of the state (the
  reaction still runs; its rows are not patched). They are user input, as every value of the state.
  - `FormState::relation_rows(name) -> &[StateRow]` (`key()`, `reference()`, `get(name)`, `text(name)`).
  - The server keeps the rows of a repeater the form shows the user (the child listable) and the user may
    change (child `create` allowed with the parent bound, `update` or `delete` admitted), and in each row the
    values of the repeater's inputs that decode; anything else is left out. A patch of a repeater not in the
    state is dropped.
- **A relationship repeater may react** (`.reacts()`): a change of its rows asks for its reaction. Boot no
  longer refuses it.
- **Patches**, by row identity, never by place:

  | Patch | Effect |
  |---|---|
  | `add_relation_row(field, data)` (`.with_ref(r)`) | adds a new row; the server gives it a ref (`r1`, …) unless the reaction did |
  | `update_relation_row(field, id, data)` | sets inputs of a row (`id`: a key or a ref) |
  | `remove_relation_row(field, id)` | removes a row |
  | `flag_relation_row(field, id, message)` | a warning under a row; changes nothing |

- **Kept only when the user could make the change** (ADR 0043's rule), else dropped and logged:
  - the repeater is an input of the form the user may change, not hidden by its condition on the state as the
    previous patches left it;
  - adding needs the child's `create` (admitted and allowed, with the parent bound) and `addable`, under
    `max_items`; updating needs `update` admitted and a row of the state the user may change (a stored row the
    panel marked updatable, or a new row); removing needs `delete` admitted, `deletable` and stays above
    `min_items`;
  - the row's data names inputs of the repeater's rows (the create inputs for a new row), never the parent key,
    an inherited key, a filter or order column; each value passes the child field's input checks (as a
    save's), hidden inputs (the row's conditions, reading the state) are left out; values canonical;
  - an id that names no row of the state is dropped; a patch that changes nothing is dropped.
- **Nothing is written.** As in ADR 0043, the panel applies the patches to its form; the save sends them as any
  change, and the server checks them again.

### Wire (vocabulary 27)

- `FormPatch` gains `add_relation_row` (`field`, `ref`, `data`), `update_relation_row` (`field`, `id`, `data`),
  `remove_relation_row` (`field`, `id`), `flag_relation_row` (`field`, `id`, `message`).
- `FormReactRequest.state` may hold a relationship repeater's rows (`[{key?, ref?, data}]`).
- `InputSchema.reacts` may be set on a relationship repeater.
- An SPA on vocabulary 26 sees vocabulary 27 as unknown and shows forms read-only.

## Consequences (part D)

- **Invariants over rows are declared once** and hold on the panel, the API and imports; a save that does not
  change the rows does not check them.
- **`validate_rows` reads rows the user may not see.** Its messages are the application's responsibility.
- **Reactions can fill relationship rows** (blanks from a prompt); the panel shows them as unsaved rows.
- ADR 0043's "left for later: row patches for relationship repeaters" is done for the top level.

## Decision: part E (vocabulary 28)

Part E lets a form's rule be application code run on the server (Q5): whether an input is shown, whether it is
required, which options a select offers. Declarative conditions (`when(..)`, ADR 0037) stay the way to write
rules the panel can evaluate itself; a closure is for rules that need code or data (a topic select offering the
topics of the chosen subject, a field required for some accounts). Decisions taken with the owner on
2026-11-19 are marked (E1–E6).

### Declaring (E1, E2, E6)

```rust
SelectInput::make("topic_id")
    .depends_on(["subject_id"])
    .options_fn(|ctx, state| Box::pin(async move { topics_of(ctx, state.get("subject_id")).await }))
    .shown_when_fn(|_ctx, state| Box::pin(async move { Ok(!state.text("subject_id").is_empty()) })),
TextInput::make("tax_id")
    .depends_on(["country"])
    .required_fn(|_ctx, state| Box::pin(async move { Ok(state.text("country") == "VN") })),
```

- `.shown_when_fn(f)`, `.required_fn(f)` (`f` answers `bool`) and `.options_fn(f)` (`f` answers
  `Vec<SelectOption>`, values in wire format), each
  `for<'a> Fn(&'a Ctx, &'a FormState) -> BoxFuture<'a, AppResult<T>>`: async, with the request's `Ctx`, so it may
  read the database; it should not write. `FormState` is the reactions' (ADR 0043): what the user's form holds.
- **`.depends_on([...])` is required** with any of them: the inputs whose change asks the server again. Each is an
  input of the form (boot).
- `.rules_timeout(d)`: 5 seconds by default, at most 30 (one limit for an input's closures).
- **Where (E6):** inputs at the form's level of a resource (inside sections, grids, tabs). Boot refuses them in
  repeater rows, on relationship repeaters (hiding one removes rows: never on arbitrary code), on hidden,
  encrypted and link inputs, on settings pages and action forms. `options_fn` is for selects and checkbox lists
  that are not `belongs_to` (those read the target's options).

### With declarative conditions (E4)

- An input may have `shown_when` and `shown_when_fn`: it is shown when both say so.
- Declarative conditions may not name an input that has `shown_when_fn` (boot): the panel evaluates them without
  waiting for the server, and no cycle can form.
- Closures read the state as the declarative conditions leave it: inputs they hide read `null`.

### The panel (E3)

- **Schema.** An input with closures carries `serverRules: {dependsOn, shown?, required?}` and, with `options_fn`,
  `options` computed: the closures' answers for the form's values when the schema is read (create: the defaults;
  edit: the record as the user's form holds it). A closure failing then counts as shown, not required, with the
  declared options; the failure is logged; so does a user (or panel) without a free turn of the reactions'
  limits, never waiting. Never on view pages. A create form opened through a relation and a blind edit ask
  `form/rules` once when they open (the schema did not know the relation's key, or the record).
- **Route `POST {admin}/api/resources/{slug}/form/rules` (E5).** Authorized as `form/react` (before the body:
  the user may create or update; `create` with `via` through a relation, `edit` with `key` on a record the user
  may update), the same state, the same limits (the `form-react` bucket, 120 per user per minute, 2 at once per
  user, 8 across users), and a `seq`. It answers `{seq, rules: {input: {shown, required, options?}}}` for every
  input with closures, `failed: true` on an error or timeout (the panel keeps what it had). Listed in
  `http::routes` and `fixtures/authz/admin.toml`.
- **Asking.** When an input named by some `depends_on` changes (typed, or patched by a reaction) and typing
  pauses (about 400 ms), the panel sends the form's values; an answer is applied only if it is the latest and no
  input it depends on changed since; one arriving while a save is in flight is asked for again afterwards.
- **Applying.** An input `shown: false` is hidden as by a condition (its value not sent); `required` adds the
  required mark and the client check; options replace the declared ones (a value no longer among them stays
  until the user changes it; the save refuses it).

### On save (Q5)

- **Every path** that writes through the form runs the closures again: the panel, the API, imports. They are
  rules of the form, not of the panel.
- **When.** After the declarative conditions, under the record's lock on update, on the state those conditions
  read (ADR 0037: what the user's form holds, the input over it); each closure with its time limit, with the
  request's `Ctx` (not the write's transaction).
- **Effect**, as a condition's:
  - hidden: the value sent is dropped (never stored), not required;
  - not hidden by a `when(..)` condition (those come first), shown and required (`required_fn`, or, under
    `shown_when_fn`, the field's own `required` or a column that is not nullable): no value (`null`, `""` or
    `[]`, the input over the stored record) is `is required`, for a user who may read and write the field, when
    the save could change that: on create, or when the input or an input it depends on is sent (an old record
    is not refused for an input the save leaves alone; review, 2026-11-19);
  - options: a value sent outside the answer is `is not one of the options` (each value of a list).
- **Failure.** A closure that fails or runs out of time refuses the save: a form error `form_rules_failed`
  ("could not check the form; try again"), nothing saved, the failure logged. Saves never wait for the route's
  limits; each input's time limit, and 30 seconds for the whole form, bound them.
- An import's check before it runs does not run the rules (it reads no database); the run does.

### Wire (vocabulary 28)

- `InputSchema.serverRules` (`dependsOn`, `shown?`, `required?`); `options` holds the computed options.
- Route `form/rules`: `FormRulesRequest` (`operation`, `key?`, `via?`, `state`, `seq`), `FormRulesResponse`
  (`seq`, `rules`, `failed?`).
- New issue code `form_rules_failed`.
- An SPA on vocabulary 27 sees vocabulary 28 as unknown and shows forms read-only.

## Consequences (part E)

- **Rules that need code or data are declared on the input** and hold on every write path.
- **Saves run application code** (bounded by its time limit): a slow or failing closure refuses saves of that
  form. Closures should be light.
- **Forms with closures ask the server while the user types** (one request per pause in a dependency), within
  the reactions' limits.
- **Declarative conditions stay first:** they cannot read inputs a closure shows or hides.
