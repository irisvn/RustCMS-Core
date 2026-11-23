# Plan: PR #4 part 6 (repeaters, second round) for 0.14.0

Status: v2.1, 2026-11-23: Codex reviewed v1, then v2 ("ready with three amendments", folded in below). Decisions: `repeater-v2-decisions.vi.md`. Review:
`../reports/2026-11-22-pr4-review.vi.md`.

Branch `integrate/pr4-part6` (on `integrate/pr4-0.14`). It holds the author's parts A (`d032004`) and B
(`001ca75`), cherry-picked unchanged (vocabularies 24 and 25). One release (0.14.0) carries the security fix,
parts 1–5 and this.

## Scope change from v1

Codex found that the part D port (reactions on relationship rows) and the part E port (`options_fn`) each
need a design of their own. Open questions it listed:
- options freshness against the locked state, replay and imports;
- child fields edited through rows;
- operation-aware row patches, tagged key/ref identities.

Both are **moved to a later release**, with a plan and a Codex review each. 0.14.0 takes A (reworked) and B
(fixed) only.

Left out, with their docs, tests, fixtures and example code:
- part C;
- `validate_rows`;
- server closures and `form/rules`;
- reactions on relationship rows;
- rows of rows on view pages;
- the LMS quiz built on them.

## 1. Part A rework

1. **Hiding keeps rows (Q1 rejected).** Done on the branch (uncommitted).
   - Removed: `RowChange.clear`, `clear_rows`, the `too_many_rows_to_remove` code, the panel's "saving
     removes N rows" notice.
   - A change sent for a repeater hidden on the server's state is ignored. `min_items` does not apply while
     hidden.
   - The boot refusal of a condition on a `deletable(false)` repeater goes.
   - The refusal of `clear_when_hidden` on a relationship repeater **stays** (ADR 0040's contract; its message
     now says the rows are kept).
   - Imports and API updates never delete rows.
2. **Row conditions, two explicit modes (Q6).**
   - A relationship repeater whose rows declare no condition is in **legacy mode**: rows are checked exactly
     as 0.13 did. The child form's conditions are evaluated with the child form's full inputs: the 0.13 code
     path, `self.apply_conditions` on the child without a `RowLevel`. The panel does not evaluate them in rows,
     as in 0.13.
   - Rows that declare conditions (`when`, `when_root`, or a layout's) are in **explicit mode**: part A's
     `RowLevel` evaluation, also in the panel.
   - The mode is decided at boot per repeater and stored in `RowRules`.
3. **Filtered relations (Q3)**, one shared engine check (`relation_access(ctx, relation) -> {readable,
   creatable}`) used by every path:
   - **Paths covered:**
     - the schema (`RelationSchema`, row repeaters);
     - relation routes (list, count, create);
     - `counts`/count columns, `RepeatableEntry::relationship()`;
     - repeater changes in `take_rows`, before any parent lookup;
     - reaction state and `via` (form/react) for a filtered relation.
   - **Read:** every filter field readable by the user (child `field_access`), else the relation is absent
     from the schema, its routes answer 404 before reading records, its repeater input is left out of the
     form, a change sent for it is 403, and count columns over it are left out.
   - **Create:** `can_write(Operation::Create)` on every injected filter value (as the foreign key in
     `relation_create`), else the section's New is absent, `addable` false in the schema, a create sent 403.
   - **Required injected fields:** in both the section's create and repeater creates, the injected values are
     in the input before `required` and conditions are checked, and re-applied after conditions (as the
     section's create does today). A client sending them stays a 422.
   - **Hooks keep membership.**
     - On create, every injected key must survive the mutating hook, with an equal canonical value;
       otherwise 500.
     - On update, the effective row (stored values merged with the hook's patch) must still match the
       filters; otherwise 500.
     - Comparisons use canonical wire values; decimals are rescaled to the column scale.
   - **Kinds:** `where_eq` is accepted on booleans, integers, UUIDs and dates only. Strings, JSON, floats,
     decimals and encrypted fields are refused at boot, because SQL collations or rounding would make SQL
     membership differ from the engine's exact check. `where_null` is accepted on any nullable, non-encrypted
     field. Disjointness at boot is then exact. String filters are left for later.
   - **Counts** with a record-dependent view policy apply the relation filters (`aggregate.rs` builds its
     select from the filtered query). Tested with such a policy.
4. **Public API (Q6).**
   - `ListQuery.relation_filters` stays `#[doc(hidden)] pub` with `#[serde(skip)]`, as `through` (M16): set
     by the engine, ignored from requests. Struct-update construction (`..Default::default()`) keeps working;
     upgrade.md notes the field.
   - `DynResource` is implemented only by the engine (`ResourceAdapter`), so its signature changes break no
     application implementation; listed in upgrade.md for callers.
   - `Condition`'s leaves keep the `scope` field. Builders are unchanged; code matching a variant by all its
     fields adds `..` (upgrade.md).
5. **Rows per parent: configurable, default 50 (owner, 2026-11-23).** `[admin] repeater_rows` (default 50,
   1–500, checked at boot) replaces `MAX_ORDERED_ROWS` (1,000) and `MAX_ROW_ITEMS` (100): the most rows of one
   parent a relationship repeater reads, locks to order, and the most entries in each part of a change. Over
   it: 422 `too_many_rows` naming the setting. Sent to the panel (`RepeaterSchema`) so it reads that many rows.
   The ceiling 500 equals `limits.keys`, so no key chunking is needed (v2's 501–1,000 problem goes away).
   Upgrade note: apps with larger repeaters raise the setting.
6. **Delete admission first:** a repeater's delete part is admitted before any key is read (already, checked
   by test).
7. **Hidden changes (v2 review):** a change sent for a repeater hidden on the server's state is still
   admitted (child abilities, 403) and shape-checked (422) as today; only an admitted, well-formed change is
   then discarded. Tested: a hidden repeater with a forbidden or malformed change still fails.
8. **Unreadable relations impose no bounds (v2 review):** a relationship repeater left out for the user (an
   unreadable filter field, or a child the user may not list) applies no `min_items`/`max_items` on create,
   as a JSON repeater the user may not write. Regression: a parent created without such a repeater.

## 2. Part B fixes

1. **Media checks in batches, in the shared checker.**
   - `media::check_references` keeps every `(path, rule, ids)` association.
   - Database lookup and locking run over the deduplicated, sorted ids, in chunks of `limits.keys`.
   - At most 2,000 distinct media per write: 422 `too_many_media` ("refers to too many files at once"), new
     template.
   - Errors stay at each row's path. Resource writes and settings saves share it.
2. **Hook timing, documented, not changed.**
   - Mutating hooks run before references are gathered (they may add references), and the media check runs
     last (ADR 0041 point 5 as amended by B).
   - A row's `after_save` may therefore run before the write's media check. If the check fails, the whole
     write rolls back, including the jobs the hook queued in the transaction.
   - The guide's hooks page says to do outside side effects through jobs dispatched in the write's
     transaction (`ctx.dispatch(txn, …)` / `queue::dispatch(txn, …)`), never `queue::enqueue(db, …)` nor
     direct calls, in `after_save`.
   - Regression: a row's `after_save` dispatches a job, a later media check fails, and neither the job nor
     the rows nor the audit entries are committed.
3. **Restores do not check JSON rows' media.** Neither did 0.13: rows had no media. The check part B added
   is removed:
   - one column may carry the rules of several forms;
   - the compiled `Rows` description is per column;
   - a strict check can refuse a valid restore.

   Field media are still checked as before. A row's file missing after a restore shows as missing, as a
   deleted media does today.

## 3. Contract and versions

- Vocabulary 24 (A reworked) and 25 (B). Bump each where needed:
  - `VOCABULARY_VERSION` in Rust and `types.ts`;
  - the hard-coded tests;
  - contract fixtures and `types.gen.ts`.
- ADR 0044 rewritten to cover A and B as accepted:
  - no "taken with the owner";
  - D, E and C listed as "not accepted in 0.14 / later".
- The amendments of ADR 0018, 0041 and 0043 are limited to what lands. 0018 is untouched: no chains. 0043
  is untouched.

## 4. Tests

**Server** (`db_test!`, 3 databases):
- Hiding keeps rows (panel save, API, import); a change sent for a hidden repeater is ignored.
- Legacy and explicit row-condition modes.
- Filtered relations:
  - an unreadable filter field → absent from the schema, 404 on the relation routes, 403 on a repeater
    change, no count column;
  - a filter field not writable on create → no New, 403;
  - a required filter field (section and repeater);
  - a hook dropping or changing a filter (create and update) → 500;
  - boot refusal of string/json/float/decimal `where_eq`;
  - a record-dependent count.
- Ordering 501 and 1,000 rows.
- Media:
  - more than 500 distinct media in one write (batches);
  - more than 2,000 → 422;
  - errors at row paths;
  - settings saves.
- Restore with JSON-row media.
- `fixtures/authz/admin.toml` updated if routes change (none planned). Contract fixtures, vocabulary tests.

**SPA:** shared condition cases; legacy rows not evaluated; no hidden-rows notice; filtered relation absent.

**E2E:** repeater specs updated (hiding keeps rows), the demo's Drafts section and table layout.

**Gates:** SQLite while coding, `just test-matrix` once, E2E, `just generated`, admin assets rebuilt
(separate commit), guide screenshots for changed screens. Then a Codex verify.

## 5. Docs

- Guide: forms, relationships, infolists, hooks, error codes, upgrade "From 0.13.1 to 0.14.0".
- Protocol; ADR 0044 rewritten; CHANGELOG with attribution to @giangittb112000.
- Report; a follow-up plan stub for D and E.
