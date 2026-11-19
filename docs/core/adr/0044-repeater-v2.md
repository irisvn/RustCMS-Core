# ADR 0044: Repeaters, second round

- Status: accepted for parts A and B (unreleased, vocabularies 24 and 25). Parts C and D are written and
  reviewed before each is built.
- Date: 2026-11-18
- Amends ADR 0037 (conditions inside rows), ADR 0040 (repeaters), ADR 0041 (one phase per child resource) and,
  in part D, ADR 0043 (form reactions).

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
| D | `validate_rows`, row hooks, reactions on relationship rows, server closures | 27 |

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

## Parts C, D

To be written before each is built. Planned content:

- **C:**
  - nested relationship repeaters, three levels (Q2): nested changes, refs of new parent rows, `inherit`
    (Q7), chained `lock_parent`;
  - deletes from the bottom level up, through policies and history, never relying on `ON DELETE CASCADE`;
  - error paths `blanks.n1.choices.n2.content`;
  - at most 1,000 rows in a write;
  - `when_parent` across levels.
- **D:**
  - `validate_rows` over the final set of rows, and a row hook before the write;
  - reaction patches on relationship rows by key or ref (amends ADR 0043);
  - server closures for visibility, options and required, re-run on save (Q5).
