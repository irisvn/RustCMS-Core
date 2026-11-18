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
`file`, and optionally one part named `collection` (a declared
[collection](#collections), before or after the file; empty means none). It needs `media.create`, the CSRF token and an allowed `Origin`, and
may carry an `Idempotency-Key`.

- The type is detected from the content (magic bytes), never from the name or
  the declared type. Only PNG, JPEG, GIF, WebP and PDF are accepted (narrowed
  by `allowed_types`, see [storage](../digging-deeper/storage.md)); SVG, HTML,
  XML and scripts never are.
- Images must be readable, with dimensions within `max_pixels`.
- Answer: `201 {data: {id, name, mime, size, width?, height?, inline, alt?, collection?}}`
  (`rustcms::admin::media::MediaItem`). Storage paths are never sent.
- An upload is recorded before its file is stored and committed with its row.
  A crash in between is cleaned by `media:prune` and by the
  `rustcms.media-prune.<disk>` [schedule](../digging-deeper/scheduling.md#framework-schedules),
  every minute.

| Answer | When |
|---|---|
| 403 | without `media.create` (checked before anything is read) |
| 413 | the file is larger than `max_upload_bytes`, or the whole body larger than that plus 64 KiB of multipart framing |
| 429, `Retry-After: 5` | `max_concurrent_uploads` uploads are already being received on this instance |
| 429 | more than 60 uploads in a minute by the same user |
| 422 `file` | "is not an accepted file type", "is not a complete file of its type", "is not a valid image", "has too many pixels", "must be one part named `file`, with at most one `collection`" |
| 422 `collection` | "is not a media collection": a name the panel does not declare |
| 409 `write_conflict` (reason `upload_expired`) | the file was stored, but pruning reclaimed it before its row committed (stored files not yet committed become reclaimable after an hour): send it again |

## The Media page

The Media resource is shown as a grid of thumbnails. Each image is its
download URL (so the same authorization applies), scaled by the browser;
other files show an icon. A toggle switches to the usual table; the choice is
kept per user in the browser. The grid shows at most 24 thumbnails a page.
Clicking an image opens the [image viewer](#the-image-viewer) over the page's
images.

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
dimensions). Its edit form changes its name and alternative text (and its
[collection](#collections), when the panel declares some):

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

## Collections

Collections sort the library into named groups: the covers of posts, the
images in their content, the site's logo. A file is in one collection at
most. Declare them on the panel, with the label the panel shows (translated
like any label), then name one on the media fields and settings whose uploads
go into it:

<<< @/../../../examples/demo/src/providers/admin_panel_provider.rs#media-collections [src/providers/admin_panel_provider.rs]

```rust
Field::column(post::Column::Cover).media().accept(&["image/*"]).collection("post-covers"),
Field::column(post::Column::Content).rich_text().collection("post-content"),
SettingField::media("logo").accept(&["image/*"]).collection("branding"),
```

| Builder | Effect |
|---|---|
| `AdminProvider::media_collection(name, label)` | declares a collection; `name` is 1 to 64 of `a-z`, `0-9`, `.`, `_`, `-`, declared once; needs `media()` |
| `Field::collection(name)` | on a `media()`, `media_list()` or `rich_text()` field: its uploads go into `name`, its picker opens on it; several fields may share one |
| `SettingField::collection(name)` | the same on `SettingField::media` / `media_list` |

A field that names a collection that is not declared, or a field that is not
media or rich text, stops the boot. Fields without `.collection(..)` upload
files without one, as before.

- **Uploads.** The panel sends the input's collection as a `collection` part
  next to the file (see [Uploads](#uploads)); the server stores it on the
  file. A name the panel does not declare is refused (422 `collection`).
- **The Media page** gets a **Collection** column and filter (with
  **Unassigned** for files without one). Uploading while the list is filtered
  on exactly one collection puts the new files into it.
- **The picker** opens on the field's collection, with a choice of every
  file, each collection, or the unassigned files; its uploads always go into
  the field's collection.
- **The media record**'s edit form moves a file to another collection, or to
  none, with the same rights as a rename (`media.update`, and
  `media.manage_all` for another user's file). Media hooks see the change.

Collections only sort and filter. They never limit access: who sees a file
and who may reference it is decided by the [scope](#whose-files-a-user-sees)
alone, and a field accepts files of any collection (choosing one from
another collection leaves it where it is).

Files uploaded before collections existed, or through fields without one,
have none (`collection` is `NULL`). A collection that is no longer declared
stays on its files: it shows as its name, is kept when other fields of the
record are edited, and is no longer offered. Without any declared collection
the panel shows no column, filter or choice at all. The column comes with the
framework migration `m20261202_000023_add_collection_to_media` (nullable,
indexed with the upload time).

## The image viewer

A click on an image opens it over the page, wherever the panel shows one: the
fields of a form, `MediaEntry` on a view page, images in rich text read on a
view page, the Media page and its records, and the picker's magnifier. The
viewer:

- shows the image fitted to the screen; it zooms in with the buttons, the
  mouse wheel, a double click or two fingers, and pans when zoomed;
- goes through the other images of the same place (a field's or an entry's
  files, a document's images, the Media page shown) with the arrows and the
  ← and → keys, and counts them (`2 / 5`);
- shows the file's name, alternative text, dimensions and size (looked up in
  the media library when the place knows only the id) and a **Download**
  button;
- closes with Escape, the close button or a click beside the image, and
  gives the focus back.

Other files (a PDF) still open in a new tab. The viewer loads the same
authorized URL as the thumbnail. `MediaEntry::make("cover").preview(false)`
and `FileUpload::make("cover").preview(false)` keep a field's images still;
images in rich text and on the Media page always open.

## Media hooks

The media library is the framework's resource, so its hooks are not yours to
override. `AdminProvider::media_hook` adds code that runs on every change to a
media record instead:

<<< @/../../../examples/demo/src/admin/media_hooks.rs#media-hook [src/admin/media_hooks.rs]

Register it after `.media()`:

```rust
AdminProvider::new().media().media_hook(MediaChanges)
```

| `change.event` | When | `change.media` | `change.previous` |
|---|---|---|---|
| `Uploaded` | an upload, in the transaction that inserts its row | the new record | — |
| `Updated` | a rename or new alternative text, before the row is written | the record as it will be stored | the record as it is |
| `Trashed` | moved to the trash | as stored | — |
| `Restored` | restored from the trash | as stored | — |
| `Deleted` | deleted for good (its file is queued for deletion in the same transaction) | as it was | — |

- Hooks run in the change's transaction, with the acting user's context
  (`ctx`), in the order they were added. An error rolls the change back and
  is answered to the user (a validation error as a field error). A refused
  upload leaves its stored file to [pruning](#limits-and-operations).
- Write only through `txn`, and keep hooks short: the media row is locked.
  To tell something outside (a public site, a CDN), dispatch a
  [queue job](../digging-deeper/queues.md) with
  `rustcms::queue::dispatch(txn, &job)`: it exists only if the change
  commits.
- `MediaEvent` may gain variants: match it with a `_` arm.

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
| `FileUpload::make(name)` | the form input for both: upload or pick from the library, remove, reorder, view |
| `.preview(false)` | on `FileUpload`: images do not open the [image viewer](#the-image-viewer), in the form and on the default view page |

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
needs**:

- version 1 by default;
- version 2 when the document holds a table, an embed, a callout, or an image
  with a caption or a width;
- version 3 when it holds underline, superscript, subscript or an
  [application element](#application-elements).

Older versions stay valid and are read as they are (1 ⊂ 2 ⊂ 3: nothing to
migrate). A document edited without the newer parts stays readable by older
releases (a rollback, an older reader of the same database): 0.9 reads
version 1, 0.10 to 0.12 read versions 1 and 2. An older panel shows a newer
document read-only and never rewrites it.

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
| `app.…` | 3 | an [application element](#application-elements): `attrs` only, as declared |

Marks:

- version 1: bold, italic, strike, code and link (`http`, `https`, `mailto`;
  at most 2,048 bytes);
- version 3: `underline`, `superscript` and `subscript`. Superscript and
  subscript never go on the same text.

Each mark goes at most once on a text (at most 7). Anything else is refused
(422), and so are nodes, marks and attributes of a newer version than the
document's.

| Limit (`rustcms::admin::rich_text::…`) | Value |
|---|---|
| `MAX_BYTES` | 256 KiB of JSON |
| `MAX_NODES` | 10,000 elements (table rows and cells count) |
| `MAX_DEPTH` | 16 levels |
| `MAX_IMAGES` | 100 |
| `MAX_EMBEDS` | 50 |
| `MAX_APP_NODES` | 500 application elements |
| `MAX_TABLE_ROWS`, `MAX_TABLE_COLUMNS` | 50 rows (the header row included), 20 columns |
| `MAX_ALT`, `MAX_CAPTION` | 500 and 1,000 characters (plain text, no marks) |
| `MAX_HREF` | 2,048 bytes |

The server's validator (`rich_text::parse`) owns every bound; the editor only
follows them. Images follow the media rules above (images only). The issue
codes of refused documents (`document_*`, `max_images`, `max_embeds`,
`max_app_nodes`) are listed in [Error codes](../reference/error-codes.md).

### In the panel

![The rich text editor: toolbar, underline, superscript, a keyboard shortcut element, a table, an embed card and a callout](/img/admin/rich-text-editor.webp)
*The RustCMS demo app (examples/demo): the post editor.*

The editor is TipTap. It injects no styles and its nodes render no `style`
attribute, so the panel's Content Security Policy is unchanged. Its toolbar
has bold, italic, strike, underline (`Mod-U`), superscript (`Mod-.`),
subscript (`Mod-,`), headings 2–4, lists, quote, code block, horizontal rule,
info and warning callouts, links, images, image details, tables and embeds,
and **Insert an element** when the field accepts
[application elements](#application-elements).

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

### Application elements

An application can declare its own elements: a blank in a fill-in-the-blank
question, a product card, a keyboard shortcut. An element is an **atom**: it
holds no text, only attributes of declared kinds, checked by the server on
every write like the rest of the document. It is never HTML.

Declare the elements in the admin provider, then name the ones each rich
text field accepts (none by default):

<<< @/../../../examples/demo/src/providers/admin_panel_provider.rs#rich-text-nodes [src/providers/admin_panel_provider.rs]

```rust
Field::column(post::Column::Content).rich_text().nodes(["app.kbd", "app.note_ref"]),
```

- **Names** are `app.` then `[a-z][a-z0-9_]{0,31}`; the framework's own nodes
  never start with `app.`.
- `RichTextNode::inline_atom(name)` goes inside text (paragraphs, headings,
  table cells, callouts; never code blocks). `block_atom(name)` goes where
  any block goes: the top level, list items and quotes; never in a table
  cell or anywhere inside a callout (lists in it included).
- `.label(text)` names it in the editor's menu and on placeholders
  (translated like labels; `lang:missing` lists it).
- `.text(|attrs| …)` gives its plain text, for exported HTML and heading ids
  (below). Keep it fast and pure: it runs while rendering.
- **Attributes** (at most 16 per element, names `[a-z][a-z0-9_]{0,31}`, never
  `node`, which HTML uses for the element's name):

  | `Attr::…` | JSON value | Options |
  |---|---|---|
  | `text()` | string | `.min(n)`, `.max(n)` characters (default at most 1,000; up to 10,000) |
  | `integer()` | number | `.range(a..=b)` (within ±(2^53 − 1)) |
  | `boolean()` | `true`/`false` | |
  | `one_of(["a", "b"])` | string | 1 to 50 options |
  | `uuid()` | string | stored in lower case |

  An attribute is required unless it has `.default(value)`. The server fills
  in missing defaults before storing a document, so changing a default later
  never changes stored documents. Defaults count towards the document's
  256 KiB once filled in, and take at most 1 KiB per element together.
  `.label(text)` names the attribute's input in the panel's form (translated;
  default: its name). A `uuid()` is not a media reference: it is not checked
  against the media library; use it for your own records.
- **Boot fails** on a malformed or repeated name or attribute, a default or
  bounds that do not fit, `.nodes(...)` on a field that is not rich text, or a
  name no `rich_text_node` declares.

In a document an element is `{"type": "app.kbd", "attrs": {"keys": "Ctrl+S"}}`
(version 3). The server refuses (422) an element the field does not accept, an
unknown, missing or ill-typed attribute, an element out of place, `content`,
`text` or `marks` on it, and more than 500 elements. Whoever writes a document
(the panel, the API, an import, a hook), the server stores it in the lowest
version its parts need, so readers of older versions keep reading documents
without version 3 parts.

**In the panel**, *Insert an element* lists the field's elements. Without an
extension, an element shows as a labelled placeholder, and its dialog is a
form made from the attribute kinds. A React extension can draw and edit it
(SDK `ui` 4; see [React pages](react-pages.md)):

<<< @/../../../examples/demo/resources/admin/src/demo/react.tsx#rich-text-node [resources/admin/src/demo/react.tsx]

- `view` gets `{ name, attrs, inline, locale, t }`; `edit` gets
  `{ name, attrs, onChange, locale, t }`. Both receive copies. The panel
  applies the attributes `edit` reports only when they fit the declaration.
- A view that throws shows the placeholder. Double-click an element, or select
  it and use *Edit the element*, to change it.
- A document holding an element the field no longer accepts (removed from
  the field, or whose attributes no longer fit) opens read-only in the form:
  the editor never drops it on its own. A button removes those elements and
  opens the rest in the editor, when what is left is a document the editor
  can hold. Its view page shows *Unknown element*, and saving it unchanged is
  refused. Keep a declaration while documents still use it.

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
  above (into the field's [collection](#collections)) and are chosen once
  stored.
- **Collection** (when the panel declares collections): the library opens on
  the field's collection; every file, another collection or the unassigned
  files are a choice away.

The server checks every chosen reference again on save (scope, trash,
`accept`, `max_size`), as it does for any write.

Clicking a file chooses it. The magnifier on an image's corner opens the
[image viewer](#the-image-viewer) over the dialog instead; Escape closes the
viewer, not the dialog.

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

let doc = rich_text::parse(&post.content)?;                 // version 1 to 3, no application element
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
- callouts: `<aside class="callout" data-tone="info">`;
- underline, superscript, subscript: `<u>`, `<sup>`, `<sub>`;
- application elements: `<span data-node="app.kbd" data-keys="Ctrl+S"></span>`
  (a `div` for block elements), one `data-{attribute}` per attribute, every
  value escaped. A frontend may replace the element with its own rendering.

No attribute outside this closed set is written. `to_html(&doc, &|id|
Some(url))` keeps its 0.9 signature (figures without sizes).

Documents with application elements: `rich_text::parse` refuses them (it knows
none). Parse with the panel's own table, `registry.rich_nodes()` (the
`AdminPanel` service), or, in a separate program, an `AppNodes` built from the
same declarations as the provider (keep them in one function). Render with
`Html` to get each element's `.text` inside it and in heading ids:

```rust
use rustcms::admin::rich_text::{AppNodes, Html};

let panel = state.service::<rustcms::admin::AdminPanel>().expect("the admin provider");
let nodes = panel.registry.rich_nodes();                    // or AppNodes::new(my_rich_text_nodes())
let doc = nodes.parse(&post.content)?;                      // or nodes.parse_for(&value, &["app.kbd"])
let page = Html::new(&doc).images(&images).nodes(&nodes);
let html = page.render();   // <span data-node="app.kbd" data-keys="Ctrl+S">Ctrl+S</span>
let toc = page.outline();   // heading ids count the elements' texts
```

`AppNodes::complete(&value)` returns a document with the defaults filled in,
as the panel stores it.

Other public items of `rustcms::admin::rich_text`:

| Item | Use |
|---|---|
| `VERSION`, `OLDEST_VERSION` | the newest (3) and oldest (1) document versions the server reads |
| `media_ids(&doc)` | the media ids a document references |
| `upgrade_value(&value)` | rewrites a stored version 1 document as version 2 (only `version` changes; never to version 3, which older releases cannot read). Never needed to read one |
| `RichTextNode`, `Attr`, `AttrKind`, `AppNodes`, `AppNode`, `NodeAttrs` | application elements: declarations, the parsed element and its attributes |
| `Html` | renders with images and application elements (`to_html_with` is `Html::new(&doc).images(&images).render()`) |
| `safe_href(href)` | whether a link address is allowed (`http`, `https`, `mailto`) |
| `Embed::from_url(url)`, `Embed::url()` | parse a provider address; the canonical address of an embed |
| `EmbedProvider::{as_str, from_name, valid_id}` | provider names and id formats |
| `CalloutTone`, `DocumentError` | a callout's tone; the error `parse` returns |
| `Node`, `Mark` | the parsed tree and its marks. Both are `#[non_exhaustive]`: a `match` needs a `_` arm, since later versions add nodes and marks |

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
  files of interrupted uploads are removed after an hour by the instance that
  staged them (`serve` and `queue:work` sweep every 10 minutes,
  `media:prune` once).
- Pruning runs every minute as the `rustcms.media-prune.<disk>` schedule (`serve`
  with the scheduler, once per slot across instances). `rustcms media:prune`
  runs it once by hand.

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
