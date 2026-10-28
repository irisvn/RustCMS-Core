# Media, uploads & rich text

The media library stores uploaded files (images and PDFs) and lets records and
settings reference them. Rich text fields hold documents, not HTML, that may
embed images from it. Use media fields for covers, galleries and attachments,
and rich text for articles and descriptions.

![The Media page: a grid of thumbnails and the upload queue](/img/admin/media-library.webp)
*A RustCMS demo application: the Media page in grid view.*

## Enabling

Register the storage provider, then turn the library on in the admin panel:

```rust
app.provider(rustcms::storage::StorageProvider);   // `[storage]`, before the panel
app.provider(AdminProvider::new().media().resource::<PostResource>());
```

`StorageProvider` must be registered before the provider that calls
`.media()`: otherwise the application panics at boot with "register
StorageProvider before AdminProvider::media()".

`media()` adds:

- the **Media** resource, with the permissions `media.view_any`,
  `media.view`, `media.create`, `media.update`, `media.delete`,
  `media.restore`, `media.force_delete`, `media.view_all` and
  `media.manage_all`;
- the upload route `POST {admin}/api/media` and the download route
  `GET {admin}/api/media/{id}/file`;
- the `media:prune` command.

Files are kept on the configured [storage](../digging-deeper/storage.md)
(local disk or S3-compatible).

## Quick start

```rust
// The field: a UUID column for one file, a JSON column for a rich text document.
Field::column(post::Column::Cover).media().accept(&["image/*"]),
Field::column(post::Column::Content).rich_text(),

// The form.
FileUpload::make("cover"),
RichEditor::make("content"),
```

## Full example

The demo registers storage before the panel:

<<< @/../../../examples/demo/src/bootstrap/providers.rs#register [src/bootstrap/providers.rs]

and turns the library on:

<<< @/../../../examples/demo/src/providers/admin_panel_provider.rs#media [src/providers/admin_panel_provider.rs]

The posts resource has a cover image and a rich text body:

<<< @/../../../examples/demo/src/admin/resources/post_resource.rs#fields [src/admin/resources/post_resource.rs]

<<< @/../../../examples/demo/src/admin/resources/post_resource.rs#form [src/admin/resources/post_resource.rs]

A settings group can hold a media reference too
([Media settings](settings.md#media-settings)).

## Whose files a user sees

| Permission | Sees and references | Renames, edits the alt text (with `media.update`); trashes, restores, deletes (with `media.delete`, `media.restore`, `media.force_delete`) |
|---|---|---|
| neither | own uploads | own uploads |
| `media.view_all` | every file | own uploads |
| `media.manage_all` | every file | every file |

`media.view_all` suits editors who put other people's images in their
records (covers, galleries, images in rich text) and readers of those
records, without the right to remove them.

- The scope decides what is listed, downloaded and accepted as a reference.
- The policy (`MediaPolicy`) refuses editing (name, alternative text),
  trashing, restoring and deleting another user's file (403) unless the user
  has `media.manage_all`.
- The trash lists only the files the user may restore or delete.
- `media.manage_all` includes everything `media.view_all` grants.

The Roles page shows them as *See every file* and *Manage every file* (under
Resources, in English and Vietnamese); an application's `permission_meta`
for them takes precedence.

![The Roles page: See every file and Manage every file](/img/admin/media-roles.webp)
*A RustCMS demo application: the media permissions on the Roles page.*

## Uploads

`POST {admin}/api/media`, `multipart/form-data` with exactly one part named
`file`. It needs `media.create`, the CSRF token and an allowed `Origin`, and
may carry an `Idempotency-Key`.

- The type is detected from the content (magic bytes), never from the name or
  the declared type. Only PNG, JPEG, GIF, WebP and PDF are accepted (narrowed
  by `allowed_types`, see [storage](../digging-deeper/storage.md)); SVG, HTML,
  XML and scripts never are.
- Images must be readable, with dimensions within `max_pixels`.
- Answer: `201 {data: {id, name, mime, size, width?, height?, inline, alt?}}`
  (`rustcms::admin::media::MediaItem`). Storage paths are never sent.
- An upload is recorded before its file is stored and committed with its row.
  A crash in between is cleaned by `media:prune`. The admin provider also
  runs this cleanup every minute in every process that boots the application.

| Answer | When |
|---|---|
| 403 | without `media.create` (checked before anything is read) |
| 413 | the file is larger than `max_upload_bytes`, or the whole body larger than that plus 64 KiB of multipart framing |
| 429, `Retry-After: 5` | `max_concurrent_uploads` uploads are already being received on this instance |
| 429 | more than 60 uploads in a minute by the same user |
| 422 `file` | "is not an accepted file type", "is not a complete file of its type", "is not a valid image", "has too many pixels", "must be the only part, named `file`" |
| 409 `write_conflict` (reason `upload_expired`) | the file was stored, but pruning reclaimed it before its row committed (stored files not yet committed become reclaimable after an hour): send it again |

## The Media page

The Media resource is shown as a grid of thumbnails. Each image is its
download URL (so the same authorization applies), scaled by the browser;
other files show an icon. A toggle switches to the usual table; the choice is
kept per user in the browser. The grid shows at most 24 thumbnails a page.

Users with `media.create` get an **Upload files** button. Several files go
through the panel's upload queue:

- at most 2 at a time;
- files over `max_upload_bytes` are refused before sending;
- a 429 is sent again after `Retry-After`;
- each pending file shows its state and the server's error, with a retry
  (when another try can succeed) that reuses the file's idempotency key, so a
  file is never stored twice.

The media schema reports this as the `upload` ability (`create` stays
refused: files enter the library only by upload).

## Names and alternative text

A file's record page shows the file and what was detected (type, size,
dimensions). Its edit form changes exactly two fields:

- `name`: a rename, cleaned as uploaded names are, at most 255 characters,
  never empty. Downloads use the new name.
- `alt`: alternative text, at most 500 characters, `null` when empty.

Every other column is set by the upload and refused on update. Editing needs
`media.update` and, for another user's file, `media.manage_all`
(`MediaPolicy`, as for trashing).

`alt` is part of the media record (`GET {admin}/api/resources/media/records/{id}`,
field `alt`) and of upload answers (`alt`, when set), so an editor inserting
an image can pre-fill its alternative text. The column comes with the
framework migration `m20261201_000022_add_alt_to_media` (nullable; existing
files have none). Code that reads media rows itself uses
`rustcms::admin::media::entity::Model`.

![A media record: the file, its details, and the name and alt text form](/img/admin/media-record.webp)
*A RustCMS demo application: a media record page.*

## Downloads

`GET {admin}/api/media/{id}/file` serves the bytes through the application (no
presigned URLs) to users who may view the media record (scope: own uploads
unless `media.view_all` or `media.manage_all`). Trashed media is not served.
Downloads are rate limited to 600 per minute per user.

The response carries `X-Content-Type-Options: nosniff`,
`Content-Security-Policy: sandbox; default-src 'none'`, and
`Content-Disposition: inline` for PNG, JPEG, GIF and WebP, `attachment`
otherwise. Like every admin API response, it is also `Cache-Control: private,
no-store`.

Deleting media for good queues its file's removal in the same transaction; the
worker retries until it succeeds.

## Media fields

```rust
Field::column(post::Column::Cover).media().accept(&["image/*"]).max_size(2_000_000),  // Uuid column
Field::column(post::Column::Gallery).media_list(10),                                   // JSON array of ids
FileUpload::make("cover"),
```

| Builder | Effect |
|---|---|
| `Field::media()` | one file; the column holds its id (UUID) |
| `Field::media_list(max)` | up to `max` files in order; the column holds a JSON array of ids |
| `.accept(&["image/*", "application/pdf"])` | MIME types (or `type/*`) the field takes |
| `.max_size(bytes)` | largest file the field takes |
| `FileUpload::make(name)` | the form input for both: upload or pick from the library, remove, reorder |

On every write (hooks included, checked after them, and on restore) each id
must exist, not be trashed, be visible to the user (`media.view` and scope),
and match `accept` and `max_size`. The rows are locked (shared) meanwhile.
Media trashed later shows as unavailable in records that reference it.

A referenced file the user cannot load (missing, trashed, or not visible
to them) shows as a "File unavailable" chip, in file fields and in rich
text alike.

Media and rich text columns may not declare a default on the entity (any
`default_value`, checked when the panel starts). A default written in a
migration is stored without these checks: do not give these columns one.

## Rich text

```rust
Field::column(post::Column::Content).rich_text(),   // JSON column
RichEditor::make("content"),
```

Rich text is stored as a **document** (JSON), not HTML. The schema is closed
and versioned. The panel's editor writes the **lowest version a document
needs**: version 1 unless the document holds a table, an embed, a callout, or
an image with a caption or a width; version 2 otherwise. Version 1 documents
stay valid and are read as they are (version 1 is a subset of version 2:
nothing to migrate), so documents edited without the version 2 nodes stay
readable by 0.9 (a rollback, an older reader of the same database).

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
| `table` | 2 | `tableRow`s of `tableHeader` or `tableCell`; every row as long as the first (rectangular); header cells make up the first row (the header row) or nothing; cells hold paragraphs of text with marks only; no `colspan`, `rowspan`, widths or styles; at most 50 rows × 20 columns (1,000 cells) |
| `embed` | 2 | `provider` (`youtube` or `vimeo`) and `id` matching that provider (YouTube `[A-Za-z0-9_-]{11}`, Vimeo `[1-9][0-9]{0,14}`); never a URL or HTML |
| `callout` | 2 | `tone` (`info` or `warn`), paragraphs and lists; never a table or a callout inside, at any depth |

Marks: bold, italic, strike, code and link (`http`, `https`, `mailto`; at
most 2,048 bytes). Anything else is refused (422), and so are version 2 nodes
and attributes in a version 1 document.

| Limit (`rustcms::admin::rich_text::…`) | Value |
|---|---|
| `MAX_BYTES` | 256 KiB of JSON |
| `MAX_NODES` | 10,000 elements (table rows and cells count) |
| `MAX_DEPTH` | 16 levels |
| `MAX_IMAGES` | 100 |
| `MAX_EMBEDS` | 50 |
| `MAX_TABLE_ROWS`, `MAX_TABLE_COLUMNS` | 50 rows (the header row included), 20 columns |
| `MAX_ALT`, `MAX_CAPTION` | 500 and 1,000 characters (plain text, no marks) |
| `MAX_HREF` | 2,048 bytes |

The server's validator (`rich_text::parse`) owns every bound; the editor only
follows them. Images follow the media rules above (images only). The issue
codes of refused documents (`document_*`, `max_images`, `max_embeds`) are
listed in [Error codes](../reference/error-codes.md).

### In the panel

![The rich text editor: toolbar, a table, an embed card and a callout](/img/admin/rich-text-editor.webp)
*RBlog, a blog built on RustCMS: the post editor.*

The editor is TipTap. It injects no styles and its nodes render no `style`
attribute, so the panel's Content Security Policy is unchanged. Its toolbar
has headings 2–4, lists, quote, code block, horizontal rule, info and warning
callouts, links, images, image details, tables and embeds.

- **Images**: *Insert image* opens the media library picker (below) to
  choose images or upload several at once. Image files dropped on the
  editor or pasted into it are uploaded too; pasted HTML images that are not
  this panel's media (another site's, `data:` URLs) are dropped.
  - Each file shows its status, its error and a retry (the same
    `Idempotency-Key`).
  - Each upload list sends at most 2 at a time (never more than
    `max_concurrent_uploads`, a limit shared by every user of the server).
  - A file refused because the server is busy (429) is sent again by itself
    after `Retry-After`, up to 3 times.
  - Files over `max_upload_bytes` are refused before sending, and the record
    cannot be saved while an upload is pending.
  - Images go after the block holding the cursor, in the order they were
    added. *Image details* (with an image selected) edits its alternative
    text, caption and width.
- **Tables** start with 3 × 3 cells and a header row. When the cursor is in a
  table, a second toolbar adds and removes rows and columns (within 50 × 20)
  and toggles the header row.
- **Embeds**: paste a YouTube (`watch?v=`, `youtu.be/`, `embed/`, `shorts/`,
  `live/`, `v/`; `www.`, `m.`, `mobile.` and `youtube-nocookie.com` hosts) or
  Vimeo (`vimeo.com/123`, `player.vimeo.com/video/123`) address. Only the
  provider and the id are kept (an unlisted Vimeo video's hash is not). The
  server parses addresses with the same allowlist (`rich_text::Embed::from_url`,
  e.g. for importers). The panel shows a text card (provider, id, page), never
  an iframe or a remote thumbnail.
- **Callouts** wrap the selected paragraphs, or a list as a whole; the
  table button is off inside a callout.

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

![The media library picker with its Library and Upload tabs](/img/admin/media-picker.webp)
*A RustCMS demo application: choosing a cover image.*

### Headless rendering

A public site built on the same database renders documents with
`rustcms::admin::rich_text` (no example application does this yet; the code
below is a sketch: `rows` and `my_public_url` are yours):

```rust
use std::collections::HashMap;
use rustcms::admin::rich_text::{self, ImageInfo};
use uuid::Uuid;

let doc = rich_text::parse(&post.content)?;                 // version 1 or 2
// The media rows of rich_text::media_ids(&doc), loaded by your app (its own scope),
// from `rustcms::admin::media::entity`, so browsers reserve the space.
let images: HashMap<Uuid, ImageInfo> = rows
    .iter()
    .map(|row| {
        let info = ImageInfo::new(my_public_url(row.id));
        let info = match (row.width, row.height) {
            (Some(w), Some(h)) => info.size(w as u32, h as u32),
            _ => info,
        };
        (row.id, info)
    })
    .collect();
let html = rich_text::to_html_with(&doc, &images);
let toc = rich_text::outline(&doc);                        // [Heading { level, text, id }]
```

The only built-in route for files is the authorized
`{admin}/api/media/{id}/file`; serving media to anonymous readers
(`my_public_url`) is up to your application.

`to_html_with` escapes every text and attribute. Its output:

- paragraphs, lists, quotes as their usual tags; code blocks as
  `<pre><code>`, horizontal rules as `<hr>`, line breaks as `<br>`; marks as
  `<strong>`, `<em>`, `<s>` and `<code>`; links as
  `<a href="…" rel="noopener noreferrer nofollow">`;
- headings carry stable ids, `<h2 id="gioi-thieu">`: the heading's text
  with diacritics folded (Vietnamese included: `Đà Nẵng` → `da-nang`), lower
  case, ASCII letters and digits only (anything else is a `-`), at most 80
  characters; repeats get `-2`, `-3`, …; a heading without letters or digits
  is `section`. The fold uses Unicode decomposition (the
  `unicode-normalization` crate, 0.1.25) plus `đ`, `ø`, `ł`, `ß`, `æ`, `œ`.
  `outline` returns the same ids for a table of contents, and
  `rich_text::slug` the rule alone;
- images: `<figure data-width="wide"><img src="…" width="800" height="600"
  alt="…"><figcaption>…</figcaption></figure>` (`data-width` only for `wide`
  and `full`, `width`/`height` only when the `ImageInfo` has both, no
  `figcaption` without a caption); images missing from the map (or whose
  `to_html` callback returns `None`) are left out;
- tables: `<table><thead><tr><th>…</th></tr></thead><tbody>…</tbody></table>`;
- embeds, a plain link:

  ```html
  <figure data-embed="youtube" data-id="ID"><a href="https://www.youtube.com/watch?v=ID" rel="noopener noreferrer">https://www.youtube.com/watch?v=ID</a></figure>
  <figure data-embed="vimeo" data-id="ID"><a href="https://vimeo.com/ID" rel="noopener noreferrer">https://vimeo.com/ID</a></figure>
  ```

  A public frontend may replace it with the provider's player (an iframe
  built from `data-embed` and `data-id`, which are checked formats, e.g.
  `https://www.youtube-nocookie.com/embed/ID` or
  `https://player.vimeo.com/video/ID`), with its own CSP for that provider;
- callouts: `<aside class="callout" data-tone="info">`.

No attribute outside this closed set is written. `to_html(&doc, &|id|
Some(url))` keeps its 0.9 signature (figures without sizes).

Other public items of `rustcms::admin::rich_text`:

| Item | Use |
|---|---|
| `VERSION`, `OLDEST_VERSION` | the newest (2) and oldest (1) document versions the server reads |
| `media_ids(&doc)` | the media ids a document references |
| `upgrade_value(&value)` | rewrites a stored version 1 document as version 2 (only `version` changes). Never needed to read one |
| `safe_href(href)` | whether a link address is allowed (`http`, `https`, `mailto`) |
| `Embed::from_url(url)`, `Embed::url()` | parse a provider address; the canonical address of an embed |
| `EmbedProvider::{as_str, from_name, valid_id}` | provider names and id formats |
| `CalloutTone`, `DocumentError` | a callout's tone; the error `parse` returns |
| `Node` | the parsed tree. It is `#[non_exhaustive]`: a `match` needs a `_` arm, since later versions add nodes |

## Limits and operations

- Referencing media needs `media.view_any` and `media.view` (the same check as
  any relationship). Records a user cannot see are never locked.
- A write keeps the permissions it started with: revoking `media.create` while
  an upload is being received does not stop that upload (the rule for
  writes in flight).
- Once an upload's file starts being stored, its record must commit within
  an hour: pruning may reclaim older stored files that have no row yet, and
  such an upload then fails with 409 `write_conflict` instead of committing.
  Receiving the request body does not count toward that hour.
- S3: configure an *abort incomplete multipart uploads* lifecycle rule on the
  bucket (a cancelled request cannot always abort its upload). The bucket is
  not probed at boot; the first upload reports a misconfiguration.
- Local disk: files are synced before their row commits; staged and partial
  files of interrupted uploads are removed by `media:prune` after an hour.
- Pruning runs every minute in each process that boots the application (with
  Redis, one instance at a time per storage disk). `rustcms media:prune` runs
  it once by hand.

| `[storage]` key | Used for |
|---|---|
| `allowed_types` | the types uploads may have (a subset of PNG, JPEG, GIF, WebP, PDF) |
| `max_upload_bytes` | the largest upload |
| `max_pixels` | the largest image, width × height |
| `max_concurrent_uploads` | uploads received at once per instance (429 beyond) |

Defaults and the other keys are on [File storage](../digging-deeper/storage.md).

## When a referenced file is trashed or deleted

References are never cascaded. Trashing or deleting a file for good leaves
its id in the records (media fields, rich text) and settings
(`SettingField::media`, `media_list`) that reference it:

- the file is no longer served (404), so the panel shows it as unavailable;
  restoring it from the trash brings it back everywhere;
- a write that **sets** a reference to it is refused (422 "refers to a file
  that is not available"), for resource fields and settings alike;
- a reference **kept as it was** is not checked again: a resource write
  checks every id of the media and rich text fields it sends (the panel sends
  only the fields that changed), so reordering a `media_list` that holds a
  trashed file is refused; a settings save checks only the ids it adds. So an
  unrelated edit still saves; to drop the reference, clear or replace the
  file;
- restoring a record from the trash checks its media and rich text
  references again: a record that points to a trashed file cannot be
  restored (422) until the file is.

Applications that render references themselves (`rich_text::to_html`, their
own pages) should treat an id they cannot resolve the same way.

![A post whose cover was trashed: the File unavailable chip](/img/admin/media-unavailable.webp)
*A RustCMS demo application: a reference to a trashed file.*

## Security notes

- Uploads are typed by their content; SVG, HTML and scripts are never
  accepted, and downloads are sandboxed by their own CSP.
- Every reference is checked server-side on every write (scope, trash,
  `accept`, `max_size`), whatever the client sends
  ([Authorization](authorization.md)).
- Rich text never stores HTML, remote images or iframes: embeds are a
  provider and an id, and the panel never loads the provider's player.

## Related pages

- [File storage](../digging-deeper/storage.md): drivers and `[storage]`
- [Settings](settings.md#media-settings): media in settings
- [Forms](forms.md): `FileUpload`, `RichEditor`
- [Relationships](relationships.md)
- [Error codes](../reference/error-codes.md)
