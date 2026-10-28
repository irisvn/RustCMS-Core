# Import and export

A resource can be exported as CSV (the list as the user sees it, in a file a
spreadsheet opens) and imported from CSV files. Use export to hand data to a
spreadsheet or another system, and import to load or update many records at
once with the same rules as the panel's forms.

![The posts list with the Export CSV button and the Exports panel](/img/admin/export-list.webp)
*A RustCMS demo application: the posts list, exporting in the background.*

## Quick start

```rust
fn meta() -> ResourceMeta {
    ResourceMeta::new("Post").title("title").exportable()          // Export CSV
}

fn meta() -> ResourceMeta {
    ResourceMeta::new("Tag").title("name")
        .unique("tags_name_key", &["name"], "has already been taken")
        .import_key("name")                                         // Import CSV; a known name is updated
}
```

Background exports and every import keep their files in the application's
[storage](../digging-deeper/storage.md): register
`rustcms::storage::StorageProvider` before the panel. Without it, the
background export and import routes answer 404.

## Full example

The demo's posts are exportable:

<<< @/../../../examples/demo/src/admin/resources/post_resource.rs#meta [src/admin/resources/post_resource.rs]

Its tags can be imported, and a row naming a known tag updates it:

<<< @/../../../examples/demo/src/admin/resources/tag_resource.rs#meta [src/admin/resources/tag_resource.rs]

The LMS imports learners matched by email:

<<< @/../../../examples/lms/src/admin/resources/learner_resource.rs#meta [src/admin/resources/learner_resource.rs]

## Exporting

`ResourceMeta::exportable()` gives the list an **Export CSV** button for
users who may export: the `export` ability, which the default policy grants
with `{slug}.export` (`posts.export`) on top of listing the resource. The
schema's `abilities.export` says whether the current user has it.

### What a file holds

- The rows the list shows with the same query: scope, filters, search, sort,
  trash, and the policy checked on each record (no page: every matching row).
- The fields the user may read, in the order of `fields()`, with a header of
  field names. Columns that are not declared never leave the server.
  Encrypted fields are never exported.
- Values in the [wire format](../reference/wire-format.md): 64-bit integers
  and decimals as written, booleans `true` / `false`, JSON as its text. An
  empty cell is `null`, `""` an empty text.
- Text that a spreadsheet would run as a formula (starting with `=`, `+`,
  `-`, `@`, also after spaces, or with a tab or line break) is written with a
  leading `'`. Numbers and dates are written as they are. Such a cell keeps
  its `'` if the file is read back elsewhere.

### At once or in the background

- **At once** (`GET {admin}/api/resources/{slug}/export?…`, the list's query
  string): up to 1,000 rows and 10 MiB. The file is built in full before
  anything is sent. Beyond that the answer is 422 (`too_many_to_export`,
  `too_large_to_export`). The panel offers to export in the background only
  for `too_many_to_export`; a file too large shows its error alone.
- **In the background** (`POST {admin}/api/resources/{slug}/exports?…`, with
  no body, whitespace or `{}`; any other body is 422 `body: is not allowed`):
  up to 10,000 rows and 50 MiB. A user may have at most three exports
  waiting or running; a fourth answers 429 with `Retry-After: 60`. An
  `Idempotency-Key` header is honoured: a retry answers with the same export.
  A queued job (kind `admin.export`) builds the file; the list shows
  **Exports** with their state and a **Download** button.
- `GET {admin}/api/exports` lists the user's background exports, newest
  first, at most 20, expired ones left out. An export can be downloaded for
  24 hours after it was asked for; its file is deleted by a later cleanup
  pass, at least one hour after that.

A background export is the user's alone, and stays theirs only while they may
see it:

- The job loads the user again at each batch of 500 rows. A disabled
  account, changed credentials (password, sign-out everywhere), a withdrawn
  `export` permission or a field they may no longer read stops it. The export
  is then `failed`, with a reason: `revoked`, `account_changed`,
  `fields_changed`, `too_many_to_export`, `too_many_matches`, `too_large`,
  `unfinished` or `failed`.
- The background route does not count rows before it queues the job. When
  the query matches more than 10,000 rows, the job stops before writing
  anything and the export fails as `too_many_to_export`; when a per-record
  policy would have to check more than 10,000 candidates, as
  `too_many_matches` (below). Narrow the list (filters, search) and export
  again. Any other refusal of the query or of the user's access is
  `revoked`.
- The fields are fixed when it is asked for: the export fails rather than
  leaving one out silently.
- Each run of the job writes its own file; the first run to finish
  publishes its file together with the list of records in it.
- Downloading (`GET {admin}/api/exports/{id}/file`) checks again:
  - someone else's export, an expired one, or one of a resource that is no
    longer exportable answers 404, like one that does not exist;
  - an export that is not finished answers 409 `export_not_ready`;
  - without the `export` ability (or `view_any`) the answer is 403;
  - changed credentials, a field the user may no longer read, or a record in
    the file that the user may no longer view **or that no longer matches the
    saved query** (scope, filters, search, trash) answers 409 `export_stale`:
    export again. A record can stay viewable and still make the export stale
    once it stops matching a filter.

Records found when the job ran are written as they were then; records deleted
or hidden while it runs are left out.

::: warning Policies that decide per record
When the policy or the trash view decides per record, the server checks at
most 10,000 candidate records before it counts the rows to export, even when
fewer would be visible. Beyond that, an export at once answers 422 ("Too many
matching records to check individually; narrow the search or filters."), and
a background export fails as `too_many_matches`.
:::

### Audit

Each export at once and each download records `exported` on the resource:
how many rows and the shape of the query (which fields were filtered, whether
there was a search, the sort), never the searched or filtered values.

## Importing

![The import dialog: column mapping, preview and the outcome report](/img/admin/import-dialog.webp)
*A RustCMS demo application: importing tags from a CSV file.*

`ResourceMeta::importable()` (or `import_key`, which implies it) adds
**Import CSV** to the list for users with the `import` ability
(`{slug}.import`, e.g. `learners.import`) and `create`, or `update` when the
resource has an import key.

`import_key(field)` matches rows to existing records. The field needs a
`ResourceMeta::unique` constraint on that column alone: the panel refuses to
start without one.

1. **Upload** (`POST {admin}/api/resources/{slug}/imports`, the CSV file as the
   body, `text/csv`). The user is admitted before the file is read. The file
   is UTF-8 (a BOM is skipped), at most 5 MiB (413 beyond), 10,000 rows, 100
   columns and 10,000 characters per cell, with a header of distinct names.
   A user may have at most three unexpired imports uploaded or running
   (429, `Retry-After: 60`),
   and the server reads at most four files at once (429, `Retry-After: 5`).
   With an `Idempotency-Key` header, sending the same file again (the answer
   was lost) answers with the same import.
   The answer lists the columns, the fields the user may set (the import key
   marked) and a suggested mapping (column = field name or label, any case).
2. **Check** (`POST {admin}/api/imports/{id}/preview` `{mapping}`): every row
   converted and checked as the create (or update) path would before any
   hook: field rules, types, required fields. Nothing is written and no hook
   runs, so a row may still fail when it runs (a hook, a unique value, a
   reference). With an import key, a row whose key cell is filled is
   previewed as an update without looking it up: if no record has that key,
   the run creates one, and a rule only creation checks (a required field
   left out) can still fail then.
3. **Import** (`POST {admin}/api/imports/{id}/run` `{mapping}`): once (a
   second run answers 409 `import_started`); the mapping is fixed then. A job
   (kind `admin.import`) writes each row in a transaction of its own through
   the usual create or update path (field rules, hooks, policy, scope, unique
   constraints, audit). Each row's transaction checks the user's permissions
   as they are then (a role change waits for it) and records that the row was
   written: if the job runs again after a crash, rows already written are
   skipped, and counted as before. An import is kept 24 hours from the time
   it started; nothing is written after that. An import still running 6
   hours after it started is marked failed (`unfinished`) by the cleanup.
4. **Outcome** (`GET {admin}/api/imports/{id}`): created, updated, failed, and
   the problems of the rows that failed: row, field, an English message with
   its code and parameters, which the panel translates. Only known messages
   whose parameters are numbers are kept; any other (your own text, a value
   of the file quoted back) becomes "is invalid", so values of the file never
   appear. The report keeps at most 1,000 problems; the counts
   are always complete. Imports are deleted after 24 hours.

### How cells are read

- Cells are read by the field's type like query strings (`7`, `true`,
  `2026-10-22`).
- An empty cell is `null` (an empty text for optional text fields that may
  not be null). A quoted empty cell `""` of an optional text field is the
  empty text, as exports write it. A required field is never set empty: the
  row is refused.
- With an import key, a row whose key cell is filled updates the record of
  the user's scope with that value (the key itself is not written), creates
  one when there is none, and fails when several match. A row without a key
  value is created. A trashed record with the same key is not matched (the
  unique constraint then refuses the row).
- Values are checked as in the forms, including the static options of a
  select or checkbox list ([options](forms.md#options-are-not-validated)): a
  row out of them fails (`not_option`, `given_twice`). A checkbox list cell is
  a JSON list (`["red","blue"]`).
- Encrypted fields are never offered for import.

The job loads the user again at each batch of 100 rows (active, same
credentials, `import` still allowed); otherwise it stops (`revoked`,
`account_changed`). Each row's write checks the user's permissions as any
write does. An `imported` audit entry records the counts; each record
created or updated has its own entry as usual.

## Reference

| Builder | Effect |
|---|---|
| `ResourceMeta::exportable()` | Export CSV, at once and in the background; ability `export` |
| `ResourceMeta::importable()` | Import CSV, creating records; ability `import` plus `create` |
| `ResourceMeta::import_key(field)` | implies `importable()`; rows matching `field` update the record (also needs `update`) |
| `ResourceMeta::unique(name, columns, message)` | the unique constraint an import key needs |

| Limit | Export at once | Export in the background | Import |
|---|---|---|---|
| Rows | 1,000 | 10,000 | 10,000 |
| Size | 10 MiB | 50 MiB | 5 MiB |
| At once per user | | 3 waiting or running | 3 uploaded or running |
| Available for | | 24 hours | 24 hours |
| User checked again every | | 500 rows | 100 rows |

A cleanup pass runs every 10 minutes in every process that boots the
application (a loop the admin provider starts). It deletes the files of
exports expired for more than an hour, and expired imports. Each pass
cleans one disk (`[storage] disk`): it deletes that disk's files, then their
records, and marks that disk's abandoned exports and imports `unfinished`;
exports and imports stored on another disk (after a change of disk,
or when two applications with different disks share a database) are left to
a pass on that disk.

## Security notes

- Exports and imports go through the resource's policy, `scope()` and field
  rules on every row, like the list and the forms
  ([Authorization](authorization.md)).
- Undeclared and encrypted columns are never written to a file and never
  read from one.
- Formula-like text is neutralized in exports, but a file opened elsewhere is
  out of the panel's control: treat exported files as the data they hold.

## Pitfalls

- An import key without its own unique constraint stops the boot.
- A preview that passes does not guarantee the run: hooks, unique values and
  references are checked only when rows are written.
- Background exports above 10,000 rows, or above 10,000 candidates when the
  policy decides per record, fail as `revoked` (see above).

## Related pages

- [Resources](resources.md): `ResourceMeta` and abilities
- [Tables](tables.md): the list a file is built from
- [File storage](../digging-deeper/storage.md)
- [Queues](../digging-deeper/queues.md): the jobs that build exports and run imports
- [Admin API](../reference/admin-api.md)
