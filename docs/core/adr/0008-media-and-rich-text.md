# 0008 — Media storage and rich text documents

- Status: accepted; amended in 0.9.0, 0.10.0 and 0.10.1 (below)
- Date: 2026-10-14

## Context

0.2 adds uploads (local disk and S3-compatible storage) and rich text. Files
are attacker-controlled bytes served from the admin origin; rich text is
attacker-controlled markup shown in the panel, whose CSP forbids inline styles
and scripts (ADR 0006).

## Decision

- **Storage** is a narrow trait (`put_file`, `open`, `delete`, `exists`) with a
  local driver (canonical root, symlinks refused, atomic rename) and an S3
  driver (`object_store`, feature `s3`). Keys are generated, never client input.
- **Downloads always go through the app** (no presigned URLs): authorization
  (policy, scope, trash) and headers apply to every byte; access ends when a
  permission is revoked. Only png/jpeg/gif/webp are served inline; everything
  else as an attachment with `nosniff` and a sandbox CSP.
- **Types come from content** (magic bytes) and a fixed allowlist; SVG/HTML/XML
  are never accepted. Images must be decodable within a pixel budget.
- **Crash safety**: an upload intent is committed before the object is written
  and removed with the media row; a force delete queues the object's removal in
  its transaction. A worker and `media:prune` reconcile both.
- **References** (media fields, rich text images) are checked after hooks on
  every write and on restore, with the media rows locked (shared).
- **Rich text is a JSON document**, validated structurally (closed set of
  nodes, marks and attributes; bounded), rendered with React elements in the
  panel (no HTML sink) and exported as escaped HTML by `rich_text::to_html`.
  The editor (TipTap, `injectCSS: false`) passed the CSP check in the E2E suite.

## Consequences

- No public media URLs, thumbnails or range requests in 0.2: apps serve public
  files with their own routes on top of `Storage`.
- Document format changes need a new version and a migration path.

## Amendment (0.9.0): seeing every file without managing it

The owner scope had one way out, `media.manage_all`, which also let its
holder trash and delete other users' files (the scope opens them to every
write). `media.view_all` opens the scope only: every file is listed,
downloaded and referenced. `MediaPolicy` refuses `delete`, `restore` and
`force_delete` of a file whose `owner_id` is not the user's unless
`media.manage_all`, so the decision stays server-side on every path (the
record routes, row abilities, the trash listing). `view` stays independent of
the record (`view_depends_on_record` is false), so lists keep their single
query.

## Amendment (0.10.0): document version 2

RBlog needs tables, embedded videos, captions and stable heading anchors.
Following "format changes need a new version and a migration path":

- **Version 2** adds `table` (`tableRow` of `tableHeader`/`tableCell`,
  rectangular, header cells only as the whole first row, cells of
  paragraphs only, no spans; at most 50 rows of 20 cells, 1,000 cells),
  `embed` (`provider` from an allowlist — `youtube`, `vimeo`; X was
  considered and left out of 0.10 — and an `id` checked against a strict
  per-provider pattern; never a URL, never HTML; addresses are parsed with
  one allowlist on the client and the server, `Embed::from_url`), `callout`
  (`tone` `info` or `warn`, paragraphs and lists, never a table or a callout
  inside), and the image attributes `caption` (plain text, 1,000
  characters) and `width` (closed enum `normal | wide | full`). Limits stay
  bounded: rows and cells count as elements (10,000), 50 embeds.
- **Version 1 is a subset of version 2**: no node or attribute changes
  meaning, so v1 documents are read as they are and nothing is migrated.
  `parse` accepts both and refuses v2 parts in a v1 document (a document
  never claims less than it uses); `upgrade_value` changes only `version`.
  The editor writes the lowest version the content needs (v1 unless it holds
  a v2 part), so documents edited without v2 parts stay readable by 0.9
  (rollbacks, older readers of the same database). The schema vocabulary
  (`VOCABULARY_VERSION`) is unchanged: no input, column or filter type
  changed, and the document format has its own version, which older clients
  already gate on: a 0.9 SPA shows a v2 document read-only and never
  rewrites it (it requires `version: 1`), so a cached old panel cannot drop
  nodes it does not know.
- **Heading ids are bare slugs** (no prefix): frontends rendering several
  documents on one page, or with their own ids (`comments`, `main`), must
  prefix or scope them; ids may start with a digit (`CSS.escape` them in
  selectors). Repeats are numbered in linear time (each base remembers its
  last suffix).
- **Embeds without third parties in the panel**: the panel shows a card (the
  provider and its page), never an iframe or a remote thumbnail, so the CSP
  is unchanged. `to_html` writes a plain link inside
  `<figure data-embed="youtube|vimeo" data-id>` (`rel="noopener noreferrer"`); a public frontend may
  swap it for the provider's player under its own CSP. The ids are checked
  formats, so they are safe in an attribute or a player URL.
- **Images in HTML** are figures; `to_html_with` takes a map from media id
  to `ImageInfo { url, width, height }` (the media rows' sizes, loaded by the
  application in its own scope) and `to_html` keeps its signature. Only a
  closed set of attributes is written. **Heading ids** are derived (slug with diacritics
  folded to ASCII, `-2`, `-3` for repeats), not stored: they stay stable for the same
  text and order, and `outline` lists them.
- **Tables in the editor** use `@tiptap/extension-table` with the node views,
  column widths, resizing and cell alignment removed: they render `style`
  attributes, which the panel's CSP forbids.

## Amendment (0.10.1): editable names and alternative text

- A file's `name` and `alt` (nullable column, ≤ 500 characters, blank stored
  as `null`) are the only columns an update may write; the stored key, disk,
  type and size never change. `MediaPolicy` allows `update` with
  `media.update`, and for another user's file `media.manage_all` as well
  (`media.view_all` sees, never edits). Names are cleaned as uploads are.
- The media schema has an `upload` ability (`media.create`, the upload
  route's own check); `create` stays refused (files arrive only by upload).
- Every upload list of the panel (the Media page, file fields, the editor,
  the picker) shares one queue implementation: at most 2 in flight, a 429
  sent again after `Retry-After` with the same key.
- Media references in settings (`SettingField::media`, `media_list`) are
  checked by the same `media::check_references` as resource fields.
