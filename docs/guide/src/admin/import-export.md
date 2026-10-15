# Exporting lists

A resource can be exported as CSV: the list as the user sees it, in a file a
spreadsheet opens.

```rust,ignore
fn meta() -> ResourceMeta {
    ResourceMeta::new("Post").title("title").exportable()
}
```

The list gets an **Export CSV** button for users who may export
(`posts.export` with the default policy, on top of listing posts: the
`export` ability; the schema's `abilities.export` says so).

## What a file holds

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

## At once or in the background

- **At once** (`GET {admin}/api/resources/{slug}/export?…`, the list's query
  string): up to 1,000 rows. The file is built in full before anything is
  sent; more rows answer 422 (`too_many_to_export`) and the panel offers to
  export in the background.
- **In the background** (`POST …/exports?…`, needs the
  [storage provider](media.md)): up to 10,000 rows, at most three waiting per
  user. A job builds the file; the list shows **Exports** with their state
  and a **Download** button. Files are deleted after 24 hours.

A background export is the user's alone, and stays theirs only while they may
see it:

- The job loads the user again at each batch of 500 rows: a disabled
  account, changed credentials (password, sign-out everywhere) or a withdrawn
  `export` permission stops it (`failed`, with the reason).
- The fields are fixed when it is asked for; if the user may no longer read
  one of them, the export fails instead of leaving it out silently.
- Downloading (`GET {admin}/api/exports/{id}/file`) checks again: same user
  and credentials, `export`, the fields, and that **every record in the file**
  is one they may still view. Otherwise 409 `export_stale`: export again.
  Someone else's export answers 404, like one that does not exist.

Records found when the job ran are written as they were then; records deleted
or hidden while it runs are left out.

## Audit

Each export at once and each download records `exported` on the resource:
how many rows and the shape of the query (which fields were filtered, whether
there was a search, the sort), never the searched or filtered values.

Importing CSV files is planned next.
