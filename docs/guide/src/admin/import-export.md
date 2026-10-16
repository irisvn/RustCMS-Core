# Import and export

A resource can be exported as CSV (the list as the user sees it, in a file a
spreadsheet opens) and imported from CSV files.

## Exporting

```rust,ignore
fn meta() -> ResourceMeta {
    ResourceMeta::new("Post").title("title").exportable()
}
```

The list gets an **Export CSV** button for users who may export
(`posts.export` with the default policy, on top of listing posts: the
`export` ability; the schema's `abilities.export` says so).

### What a file holds

- The rows the list shows with the same query: scope, filters, search, sort,
  trash, and the policy checked on each record (no page: every matching row).
- The fields the user may read, in the order of `fields()`; a header of
  field names. Columns that are not declared never leave the server.
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
  anything is sent; beyond, 422 (`too_many_to_export`, `too_large_to_export`)
  and the panel offers to export in the background.
- **In the background** (`POST …/exports?…` with no body, needs the
  [storage provider](media.md)): up to 10,000 rows and 50 MiB, at most three
  waiting per user, `Idempotency-Key` honoured (a retry answers with the same
  export). A job builds the file; the list shows **Exports** with their state
  and a **Download** button. Files are deleted after 24 hours.

A background export is the user's alone, and stays theirs only while they may
see it:

- The job loads the user again at each batch of 500 rows: a disabled
  account, changed credentials (password, sign-out everywhere), a withdrawn
  `export` permission or a field they may no longer read stops it (`failed`,
  with the reason: `revoked`, `account_changed`, `fields_changed`,
  `too_large`, `unfinished`, `failed`).
- The fields are fixed when it is asked for: the export fails rather than
  leaving one out silently.
- Each run of the job writes its own file; the first run to finish
  publishes its file together with the list of records in it.
- Downloading (`GET {admin}/api/exports/{id}/file`) checks again: same user
  and credentials, `export`, the fields, and that **every record in the file**
  is one they may still view. Otherwise 409 `export_stale`: export again.
  Someone else's export answers 404, like one that does not exist.

Records found when the job ran are written as they were then; records deleted
or hidden while it runs are left out.

### Audit

Each export at once and each download records `exported` on the resource:
how many rows and the shape of the query (which fields were filtered, whether
there was a search, the sort), never the searched or filtered values.

## Importing

```rust,ignore
fn meta() -> ResourceMeta {
    ResourceMeta::new("Learner").title("name")
        .unique("learners_email_key", &["email"], "is already registered")
        .import_key("email")      // rows naming a known email update it
}
```

`ResourceMeta::importable()` (or `import_key`) adds **Import CSV** to the
list for users with `import` (`learners.import`) and `create` (or `update`,
with an import key). Imports need the storage provider.

1. **Upload** (`POST {admin}/api/resources/{slug}/imports`, the CSV file as the
   body, `text/csv`): admitted before the file is read; UTF-8 (a BOM is
   skipped), at most 5 MiB, 10,000 rows, 100 columns, 10,000 characters per
   cell, a header of distinct names; at most three imports waiting per user.
   With an `Idempotency-Key` header, sending the same file again (the answer
   was lost) answers with the same import.
   The answer lists the columns, the fields the user may set (the import key
   marked) and a suggested mapping (column = field name or label, any case).
2. **Check** (`POST imports/{id}/preview` `{mapping}`): every row converted and
   checked as the create (or update) path would before any hook: field rules,
   types, required fields. Nothing is written and no hook runs, so a row may
   still fail when it runs (a hook, a unique value, a reference).
3. **Import** (`POST imports/{id}/run` `{mapping}`): once (a second run
   answers 409 `import_started`); the mapping is fixed then. A job writes each
   row in a transaction of its own through the usual create or update path
   (field rules, hooks, policy, scope, unique constraints, audit). Each
   row's transaction checks the user's permissions as they are then (a role
   change waits for it) and records that the row was written: if the job
   runs again after a crash, rows already written are skipped, and counted
   as before. An import is kept 24 hours from the time it started; nothing is
   written after that.
4. **Outcome** (`GET imports/{id}`): created, updated, failed, and the problems
   of the rows that failed (row, field, a message in the user's language;
   never the values of the file, nor a hook's own message: those show as
   "is invalid"). Imports are deleted after 24 hours.

Cells are read by the field's type like query strings (`7`, `true`,
`2026-10-22`); an empty cell is `null` (an empty text for optional text fields
that may not be null), and a quoted empty cell `""` of an optional text field
the empty text, as exports write them (a required field is never set empty:
the row is refused). With an import key, a row whose key cell is filled
updates the record of the user's scope with that value (the key itself is not
written), creates one when there is none, and fails when several match; a row
without a key value is created. A trashed record with the same key is not
matched (the unique constraint then refuses the row).

The job loads the user again at each batch of 100 rows (active, same
credentials, `import` still allowed); otherwise it stops (`revoked`,
`account_changed`). Each row's write checks the user's permissions as any
write does. An `imported` audit entry records the counts; each record
created or updated has its own entry as usual.
