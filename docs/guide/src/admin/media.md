# Media, uploads & rich text

The media library stores uploaded files and lets records reference them;
rich text fields hold documents that may embed images from it.

## Enabling

```rust,ignore
app.provider(rustcms::storage::StorageProvider);          // `[storage]`
app.provider(AdminProvider::new().media().resource::<PostResource>());
```

`media()` adds the **Media** resource (permissions `media.view_any`, `media.view`,
`media.create`, `media.delete`, `media.restore`, `media.force_delete`,
`media.view_all`, `media.manage_all`), the upload route and `media:prune`.

## Whose files a user sees

| Permission | Sees and references | Trashes, restores, deletes (with `media.delete`, `media.restore`, `media.force_delete`) |
|---|---|---|
| neither | own uploads | own uploads |
| `media.view_all` | every file | own uploads |
| `media.manage_all` | every file | every file |

`media.view_all` suits editors who put other people's images in their
records (covers, galleries, images in rich text) and readers of those
records, without the right to remove them. The scope decides what is listed,
downloaded and accepted as a reference; the policy refuses the trash,
restore and delete of another user's file (403) unless `media.manage_all`,
and the trash lists only the files the user may restore or delete.
`media.manage_all` includes everything `media.view_all` grants. The Roles
page shows them as *See every file* and *Manage every file* (under
Resources, in English and Vietnamese); an application's `permission_meta`
for them takes precedence.

Files are kept on the configured [storage](../digging-deeper/storage.md)
(local disk or S3-compatible).

## Uploads

`POST {admin}/api/media`, `multipart/form-data` with exactly one part named
`file`. It needs `media.create`, CSRF and an allowed `Origin`, and may carry an
`Idempotency-Key`.

- The type is detected from the content (magic bytes), never from the name or
  the declared type. SVG, HTML, XML and scripts are never accepted.
- Images must be readable (dimensions within `max_pixels`).
- Answer: `201 {data: {id, name, mime, size, width?, height?, inline}}`. Storage
  paths are never sent.
- An upload is recorded before its file is stored and committed with its row; a
  crash in between is cleaned by `media:prune` (also run every minute by `serve`).

## Downloads

`GET {admin}/api/media/{id}/file` serves the bytes through the application (no
presigned URLs) to users who may view the media record (scope: own uploads
unless `media.view_all` or `media.manage_all`). Trashed media is not served. Headers:
`X-Content-Type-Options: nosniff`, `Content-Security-Policy: sandbox;
default-src 'none'`, `Cache-Control: private, no-store`, and
`Content-Disposition: inline` for png/jpeg/gif/webp, `attachment` otherwise.

Deleting media for good queues its file's removal in the same transaction; the
worker retries until it succeeds.

## Media fields

```rust,ignore
Field::column(post::Column::Cover).media().accept(&["image/*"]).max_size(2_000_000),  // Uuid column
Field::column(post::Column::Gallery).media_list(10),                                   // JSON array of ids
FileUpload::make("cover"),
```

On every write (hooks included, checked after them, and on restore) each id
must exist, not be trashed, be visible to the user (`media.view` and scope), and
match `accept` / `max_size`; the rows are locked (shared) meanwhile. Media
trashed later shows as unavailable in records that reference it.

Media and rich text columns may not declare a default on the entity (any
`default_value`, checked when the panel starts). A default written in a
migration is stored without these checks: do not give these columns one.

## Rich text

```rust,ignore
Field::column(post::Column::Content).rich_text(),   // JSON column
RichEditor::make("content"),
```

Rich text is stored as a **document** (JSON), not HTML. The panel's editor
saves version 2; version 1 documents (0.2–0.9) stay valid and are read as
they are (version 1 is a subset of version 2: nothing to migrate).

```json
{"type": "doc", "version": 2, "content": [
  {"type": "heading", "attrs": {"level": 2}, "content": [{"type": "text", "text": "Giới thiệu"}]},
  {"type": "image", "attrs": {"media": "0199d3a4-…", "alt": "A cat", "caption": "Our cat", "width": "wide"}},
  {"type": "table", "content": [
    {"type": "tableRow", "content": [{"type": "tableHeader", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "Plan"}]}]}]},
    {"type": "tableRow", "content": [{"type": "tableCell", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "Pro"}]}]}]}
  ]},
  {"type": "embed", "attrs": {"provider": "youtube", "id": "dQw4w9WgXcQ"}},
  {"type": "callout", "attrs": {"tone": "info"}, "content": [{"type": "paragraph", "content": [{"type": "text", "text": "Note"}]}]}
]}
```

| Node | Version | Content |
|---|---|---|
| `paragraph`, `heading` (levels 2–4) | 1 | text and line breaks |
| `bulletList`, `orderedList`, `listItem`, `blockquote` | 1 | blocks |
| `codeBlock` | 1 | plain text |
| `horizontalRule` | 1 | |
| `image` | 1 | `media` (a media id), `alt`; version 2 adds `caption` (plain text) and `width` (`normal`, `wide` or `full`) |
| `table` | 2 | `tableRow`s of `tableHeader` or `tableCell`; every row as long as the first; header cells make up the first row (the header row) or nothing; cells hold paragraphs only; no spans, widths or styles |
| `embed` | 2 | `provider` (`youtube`, `vimeo`, `x`) and `id` in that provider's format (YouTube: 11 characters `A-Z a-z 0-9 - _`; Vimeo: 1–15 digits; X: 1–20 digits); never a URL or HTML |
| `callout` | 2 | `tone` (`info` or `warn`), paragraphs |

Marks: bold, italic, strike, code and link (`http`, `https`, `mailto`).
Anything else is refused (422), and so are version 2 nodes and attributes in
a version 1 document. Limits (`rustcms::admin::rich_text::MAX_*`): 256 KiB,
10,000 elements (table rows and cells count), 16 levels, 100 images, 50
embeds, tables of at most 50 rows (the header row included) of 20 cells,
500 characters of image description and 1,000 of caption. Images follow
the media rules above (images only).

### In the panel

The editor is TipTap (no injected styles; its nodes render no `style`
attribute, so the CSP is unchanged). Its toolbar has headings 2–4, lists,
quote, code block, horizontal rule, info and warning callouts, links, images,
image details, tables and embeds.

- **Images**: *Insert image* opens the media library picker (below) to
  choose images or upload several at once. Image files dropped on the
  editor or pasted into it are uploaded too; pasted HTML images that are not
  this panel's media (another site's, `data:` URLs) are dropped. Each file
  shows its status, its error and a retry (the same `Idempotency-Key`); at
  most `max_concurrent_uploads` are sent at a time, files over
  `max_upload_bytes` are refused before sending, and the record cannot be
  saved while an upload is pending. Images go after the block holding the
  cursor, in the order they were added. *Image details* (with an image
  selected) edits its alternative text, caption and width.
- **Tables** start with 3 × 3 cells and a header row; when the cursor is in a
  table, a second toolbar adds and removes rows and columns (within 50 × 20)
  and toggles the header row.
- **Embeds**: paste a YouTube (`watch?v=`, `youtu.be/`, `embed/`, `shorts/`,
  `live/`), Vimeo (`vimeo.com/123`, `player.vimeo.com/video/123`) or X
  (`x.com/user/status/123`, `twitter.com/…`) address; only the provider and
  the id are kept (an unlisted Vimeo video's hash is not). The panel shows a
  card with the provider and its page, never an iframe or a third-party
  image.

The read-only view renders the same nodes with React elements.

### The media library picker

*Insert image* and the **Media library** button of `FileUpload` /
`media_list` fields open the same dialog:

- **Library** (with `media.view_any` and `media.view`): the files the user
  may reference (own uploads, or every file with `media.view_all`), newest
  first, 24 a page, searched by name. Thumbnails are the file's own
  authorized URL, scaled by the browser (no resized copies). Only the
  field's `accept` types are listed; files over its `max_size` and files
  already in the field are shown but cannot be chosen; a list field offers
  as many as it has room for.
- **Upload** (with `media.create`): drop or choose files; they upload as
  above and are chosen once stored.

The server checks every chosen reference again on save (scope, trash,
`accept`, `max_size`), as it does for any write.

### Headless rendering

```rust,ignore
use rustcms::admin::rich_text::{self, ImageInfo};

let doc = rich_text::parse(&post.content)?;                 // version 1 or 2
// Sizes from the media rows (`rustcms::admin::media::entity`), so browsers reserve the space.
let html = rich_text::to_html_with(&doc, &|id| {
    let row = rows.get(&id)?;                               // rows you loaded for rich_text::media_ids(&doc)
    let info = ImageInfo::new(my_public_url(id));
    Some(match (row.width, row.height) {
        (Some(w), Some(h)) => info.size(w as u32, h as u32),
        _ => info,
    })
});
let toc = rich_text::outline(&doc);                        // [Heading { level, text, id }]
```

`to_html_with` escapes every text and attribute. Its output:

- headings carry stable ids, `<h2 id="gioi-thieu">`: the heading's text
  with diacritics folded (Vietnamese included: `Đà Nẵng` → `da-nang`), lower
  case, other characters as `-`, at most 80 characters; repeats get `-2`,
  `-3`, …; a heading without letters or digits is `section`. `outline`
  returns the same ids for a table of contents, and `rich_text::slug` the
  rule alone;
- images: `<figure data-width="wide"><img src="…" width="800" height="600"
  alt="…"><figcaption>…</figcaption></figure>` (`data-width` only for `wide`
  and `full`, `width`/`height` only when the callback returns both, no
  `figcaption` without a caption); images whose callback returns `None` are
  left out;
- tables: `<table><thead><tr><th>…</th></tr></thead><tbody>…</tbody></table>`;
- embeds: `<figure class="embed" data-provider="youtube" data-id="…"><a href="https://www.youtube.com/watch?v=…" rel="noopener noreferrer nofollow">…</a></figure>`,
  a plain link. A public frontend may replace it with the provider's player
  (an iframe built from `data-provider` and `data-id`, which are checked
  formats), with its own CSP for that provider;
- callouts: `<aside class="callout" data-tone="info">`.

`to_html(&doc, &|id| Some(url))` keeps its 0.9 signature (figures without
sizes). `rich_text::upgrade(&value)` returns a version 1 document as
version 2 (only `version` changes), for applications that want to rewrite
stored documents; it is never needed to read them.

## Limits and operations

- Referencing media needs `media.view_any` and `media.view` (the same check as
  any relationship). Records a user cannot see are never locked.
- A write keeps the permissions it started with: revoking `media.create` while
  an upload is being received does not stop that upload (the rule for
  writes in flight).
- Uploads must finish within an hour: pruning reclaims older unfinished ones, and
  such an upload then fails with 409 instead of committing.
- S3: configure an *abort incomplete multipart uploads* lifecycle rule on the
  bucket (a cancelled request cannot always abort its upload). The bucket is not
  probed at boot; the first upload reports a misconfiguration.
- Local disk: files are synced before their row commits; staged and partial files
  of interrupted uploads are removed by `media:prune` after an hour.
- Downloads are rate limited per user (600 per minute). A library page in
  the picker loads up to 24 thumbnails.
