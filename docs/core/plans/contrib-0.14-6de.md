# Plan: parts D and E of PR #4 in 0.14.0 (reactions on relationship rows, `options_fn`)

Status: v1, 2026-11-23, for Codex review. The owner asked to ship them in the same release as parts 1–5, A and B
(one full gate). Branch `integrate/pr4-part6` (HEAD `f02e765` + assets). The contributor's originals:
`9b428756` (D) and `bb2e68bf` (E) in the `RustCMS-pr4` worktree. Inputs: Codex's plan review of
`contrib-0.14-part6.md` v1, findings 2, 3, 4 and 9 and the "missing items", and the review report.

Out of scope, as decided:
- `validate_rows`, `shown_when_fn`, `required_fn` and the all-rules route `form/rules`;
- nested rows (part C).

## E. `options_fn`: options computed on the server

### Declaring
- `SelectInput::options_fn(f)` / `CheckboxList::options_fn(f)`.
  - Signature: `for<'a> Fn(&'a Ctx, &'a FormState) -> BoxFuture<'a, AppResult<Vec<SelectOption>>>`.
  - It needs `.depends_on([...])`: inputs of the same form (boot), and the triggers of a refresh.
  - `.options_timeout(d)`: 5 s default, 30 s at most.
- **Only on inputs at a resource form's own level.** Boot refuses it:
  - on `belongs_to` selects;
  - on inputs in JSON or relationship rows;
  - on settings pages and action forms;
  - on hidden/encrypted/link inputs.
- At `Registry::finish`, boot also refuses a child resource whose field has `options_fn` when a parent's
  relationship repeater edits that field in its rows. Such writes go through `create_row`/`update_row` inside
  the parent's transaction, where no preflight runs (Codex v1 finding 3).
- **Contract with the application** (documented):
  - The closure gets the request's `Ctx` with **no transaction** and the form's state (readable values only).
  - `depends_on` names what refreshes the options and what the save compares under the lock.
  - Other reads the closure makes are not atomic with the write: options are a form rule, not a data
    invariant. Invariants belong in hooks.

### Asking: `POST {admin}/api/resources/{slug}/form/options`
- **Request:** `{operation, key?, via?, input, state}`. **Answer:** `{options}` or `{failed: true}`.
- **Authorized as `form/react`, before the body:**
  - create or update admitted; `via` needs the relation's `creatable` (part A's `relation_access`); `key`
    needs a record the user may update;
  - the `form-react` bucket (120 per user per minute) and the reaction turns (per user, global).
- **The state is built as reactions build it:**
  - stored values only for a record the user may view;
  - typed values only for writable fields;
  - readable fields only;
  - the bound key and the relation's fixed values over what the client sent.
- **The input must be one the user's form holds:**
  - readable and writable for the operation, with `options_fn`, not hidden by the declarative conditions
    on the state;
  - otherwise 404 (same answer for an unknown input).
- **The answer is checked:**
  - at most 500 options and 64 KiB;
  - each value decodes to the column's kind and is made canonical; no duplicate values;
  - labels translated.

  A failure (error, timeout, a bad answer) is logged and answered `failed: true`; the panel keeps the declared
  options.
- Listed in `http::routes`, with cases in `fixtures/authz/admin.toml` mirroring `form/react`'s, plus:
  - an input the user cannot read → 404;
  - an input without `options_fn` → 404.

### Schema and panel
- `InputSchema.optionsFrom: {dependsOn}`. The declared options are sent as they are; no closure runs while a
  schema is built.
- The panel asks `form/options` when the form opens and when a dependency changes. Requests are debounced
  (about 400 ms); only the latest answer for the current dependency values is applied.
- A value no longer among the options stays shown, disabled (part 1's stray option), until the user changes it.

### On save (resource create and update; panel, API, import execution)
1. **When it runs.** For each input with `options_fn` whose effective value after the write is not empty, and
   that the write could change:
   - on create;
   - or when the input or one of its dependencies is sent.

   An old record is never refused for an input the save leaves alone.
2. **Order.** The idempotency replay is checked first: create's `replay_create` before anything, update's
   claim lookup. A replayed request never runs closures.
3. **Preflight, before `begin()`.**
   - Build the state as the route does, from the record read **without a lock** (update) and the input over
     it, and run the closure.
   - One save-wide deadline covers waiting for a global turn, the closure and the answer's checks. Saves
     wait for a turn up to the deadline: no per-user immediate refusal.
   - Failure: a form error `options_failed` ("could not check the options; try again"; new template), nothing
     saved.
4. **Under the lock.**
   - Rebuild the **authorized, normalized projection** of the dependency inputs: readable values, the input
     over the locked record, conditions applied.
   - If it differs from the one the closure read: 409 `stale`. Only values the user may read are compared,
     so no hidden value is disclosed.
   - Then check the **effective locked value** (sent, or stored when omitted) against the answer: each value
     of a list, `is not one of the options` (`not_option`).
   - The declared static options are not checked for that input.
5. **Imports.** The dry-run (synchronous, no database) skips these checks. The run executes each row as a
   save, with the same steps.
6. No closure ever runs with the write's transaction or a lock held.

## D. Reactions on relationship rows (top-level repeaters)

### State
- `state[name] = [{"key": "8", "data": {…}}, {"ref": "n1", "data": {…}}]`. At most `repeater_rows` rows;
  beyond that, the repeater is left out of the state.
- **Kept only for a repeater the user's form holds:**
  - the relation readable (part A's `relation_access`) and the child listable;
  - shown on the state.
- In each row, only the repeater's row inputs, readable child fields, values that decode, canonical.
- **Identities stay tagged.** `RowId::Key(String)` and `RowId::Ref(String)` are distinct in the server
  (`FormState::relation_rows(name) -> &[StateRow]`, `StateRow::id() -> &RowId`) and in the panel.
  - A ref matches `^n[0-9]{1,9}$`.
  - A row given twice (the same tagged id) drops the repeater from the state.
  - A stored key and a ref never compare equal (Codex v1 finding 9).

### Patches
`FormPatch` gains four variants:
- `add_relation_row(field, data)` (with an optional ref);
- `update_relation_row(field, id, data)`;
- `remove_relation_row(field, id)`;
- `flag_relation_row(field, id, message)`.

`id` is a tagged `RowId` (`{"key": "8"}` / `{"ref": "n1"}` on the wire).

A patch is kept only when the user could make the change: ADR 0043's rule, checked **against trusted records**
(Codex v1 finding 4).

**Every patch:**
- The repeater is an input the user may change, shown on the state as the previous patches left it.

**`add_relation_row`:**
- the child's `create` admitted and allowed with the parent bound;
- the relation `creatable`, `addable`, under `max_items` and `repeater_rows`;
- each value: the child field **readable and writable on create**, `accept_row_value(Operation::Create)`;
- never the parent key, a filter or the order column.

**`update_relation_row`:**
- A **Ref** names a new row of the state: values checked as for an add.
- A **Key** names a stored row, loaded read-only through the child's scope, view policy, parent membership
  (the bound parent key) and the relation's filters.
- `update` must be admitted and allowed **on that record**.
- Each value: the field **readable and writable on update**, `accept_row_value(Operation::Update)`.
- Row conditions are evaluated on the state's row with the patch applied: hidden inputs are dropped.

**`remove_relation_row`:**
- A **Key**: `delete` admitted and allowed on the loaded record, `deletable`, and the state stays at or above
  `min_items`.
- A **Ref**: removes a new row of the state.

**`flag_relation_row`:**
- The id names a row of the state. Message at most 500 characters.

**Any id that names no row of the state, or a record not found as above:** the patch is dropped (logged).

**Rows of a state hold at most `repeater_rows` rows.** Records loaded for keyed patches are read in one query
(key order), bounded by the state.

The reaction limits and ADR 0043's single deadline cover the reaction and its patch checks. Nothing is written;
the save checks everything again.

### Panel
- Sends a repeater's rows (stored rows read, with unsaved edits; new rows by ref) as state.
- Applies the four patches to its form by tagged id.
- Shows a flag under its row.

## Contract, versions, docs
- Vocabulary 26 (D) and 27 (E), with the usual updates: Rust and TS constants, hard-coded tests, contract
  fixtures, `types.gen.ts`.
- `FormPatch` is not `#[non_exhaustive]`: the four variants are a source break for exhaustive matches
  (upgrade.md).
- Issue templates: `options_failed`.
- ADR 0044: sections D and E rewritten to this design; ADR 0043 amended (row patches, the save-time options
  check outside the transaction).
- Guide: forms (`options_fn`, reactions on rows), error codes, upgrade; protocol; CHANGELOG with attribution.

## Tests
**E, server:**
- route authz: create/update admission, `via` without `creatable`, an unreadable input, no `options_fn`, the
  bucket and turns;
- the answer's checks: kinds, duplicates, over 500;
- save: a value outside the answer, omitted value with a changed dependency, a stale dependency → 409;
- an unreadable dependency never causes a 409 that discloses it;
- no transaction or lock during the closure: the closure takes a second pooled connection on a 1-connection
  pool and still succeeds;
- replayed create and update run no closure;
- import execution checks, dry-run skips;
- boot refusals: rows, settings, actions, `belongs_to`, a child field edited through a parent's repeater.

**D, server:**
- state building: unreadable fields dropped, tagged identities, duplicates, over `repeater_rows`;
- each patch kind kept and dropped:
  - a field readable but not writable, writable but not readable;
  - a create-only vs update-only field;
  - a key of another parent, outside the filter, out of scope, not updatable/deletable;
  - a ref unknown;
  - `min_items`/`max_items`;
- a key `"n1"` vs a ref `n1`;
- the deadline.

**SPA:** applying row patches, flags, options refresh (debounce, latest only), stray values.

**E2E:** a demo select with `options_fn` (a category's topics), a reaction filling a row.

`fixtures/authz/admin.toml`: `form/options`.

**One full gate at the end** for all of 0.14.0 (owner's request): matrix, E2E, `just generated`, then Codex
verify.

## v2: changes after Codex's review of v1 (these override the sections above where they differ)

### E

1. **No oracle over unreadable values.** A save checks:
   - a value **the client sent** for the input;
   - an omitted stored value only when the user may read the field and view the record (the same rule as
     the reaction state, `input.rs` `condition_values`).

   Otherwise the input is skipped: never accepted or refused because of a value the caller cannot observe.
   Blind updates (no `view`) check only sent values. Tested for an unreadable field and for an update
   without `view`.
2. **Child resources.** Boot refuses `options_fn` on any resource that is the child of a relationship
   repeater whose rows are writable: its row writes run inside the parent's transaction.
3. **One state builder.**
   - Preflight and the locked check build the state with the same function:
     - the reaction state's authorization (readable fields, viewable record, sent values for writable
       fields, bound key and fixed values);
     - then the declarative conditions' normalization (`shown_values`: hidden inputs `null`).
   - Every triggered input the user may access is evaluated, whatever its value. A trigger is: create, the
     input sent, or a dependency sent.
   - Under the lock, the state is rebuilt. If an input's eligibility or its dependency projection differs
     from the preflight's: 409 `stale`. An input is never checked without an answer.
   - Dependencies are inputs of the same form level, with scalar or list values.
   - The save-wide deadline is the largest `options_timeout` of the triggered inputs (at most 30 s), covering
     turns, every closure and the answers' checks. Each closure also keeps its own timeout.
4. **Replay order:**
   1. admission;
   2. the claim from the original input;
   3. a committed-receipt lookup (new fast path at the create and update entry points, before links, rows
      and validation);
   4. the authorized preflight;
   5. begin and transactional acquire (replay as today if another request committed meanwhile);
   6. the locked checks.

   Already-committed retries run no closure. Concurrent duplicates may run read-only closures (documented).
   Relation create keeps its replay before the parent lookup.
5. **Imports.**
   - `stale` and `options_failed` are retryable: the import job retries the row as it does for
     `write_conflict`, within its retry bound; reported as their own codes if retries run out.
   - Receipts are still skipped before any evaluation.
   - Locked checks use the principal refreshed by `begin`.
6. **Membership** uses the existing single/many checker (`Options::Single/Many`) with the evaluated options:
   checkbox lists keep the array shape and the no-repeat rule.
   - Boot refuses two inputs bound to one field when one has `options_fn`.
   - The static-option exemption applies in the shared input checks and in reaction patch checks for that
     input.

### D

7. **One row-patch cleaner** for adds and updates, using the repeater's condition mode (legacy: the child
   form's conditions over its full inputs; explicit: the row rules):
   - the condition state is built from authorized values: for a stored row, the readable stored values of the
     record loaded as in "Patches"; for a new row, the child's declared defaults;
   - the accepted state row and the patch are overlaid, with the normalized root;
   - inputs hidden there are dropped, for adds as well as updates.
8. **Triggers and parent create:**
   - A relationship repeater may declare `.reacts()`. Boot accepts it. The schema says `reacts: true` when
     the user may change the repeater's rows. The panel asks when rows are added, removed, reordered or
     edited.
   - On a parent create, row patches are `add`, `update`/`remove` of refs, and flags.
   - Keyed patches are dropped, since there are no stored rows.
   - Adds need the child's `create` admitted and allowed without a parent record, as the panel's form for a
     new parent does today, plus field rules and `creatable`.
   - Removing a ref honours `deletable` and `min_items` too, as the panel does for new rows.
   - The save's wire format (`order` with `{"new": ref}`) is unchanged; tagged ids are only in reaction
     state and patches.

### Tests added by v2
- E:
  - the oracle cases;
  - a child resource refused at boot;
  - eligibility changing between preflight and lock → 409;
  - a committed retry runs no closure;
  - import retry on `stale`;
  - checkbox shape;
  - two inputs on one field refused.
- D:
  - legacy-mode conditions reading child inputs outside the row;
  - defaults on adds;
  - parent-create patches;
  - ref removal honouring `deletable`/`min_items`;
  - `.reacts()` on a repeater.

### v2.1 (Codex round 2: "nearly ready", two clarifications)
- **Row cleaner baseline:** the condition state of a row patch also carries the server's authoritative values:
  - the bound parent key, `null` on a parent create (no key yet; a condition reading it sees `null`, as the
    save's conditions do before the row is written);
  - the relation's fixed filter values.

  These are put over the defaults, the state row and the patch, as `create_row` does before conditions.
- **Shape before membership:** for an input with `options_fn`, the exemption only skips membership in the
  declared options. A checkbox list's value must still be an array without repeats, in saves and in reaction
  patches, with or without an evaluated answer. The checks are kept as their own step (not by clearing
  `rule.options`).

Status: ready to implement (Codex round 2).
