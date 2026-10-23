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

Rich text is stored as a **document** (JSON), not HTML: paragraphs, headings
2–4, lists, quotes, code blocks, rules, line breaks, images (media ids) and the
marks bold, italic, strike, code and link (`http`, `https`, `mailto`). Anything
else is refused (422); documents are limited to 256 KiB, 10,000 elements,
16 levels and 100 images. Images follow the media rules above (images only).

The panel edits it with TipTap (no injected styles) and shows it with React
elements. Headless apps render it with `rustcms::admin::rich_text::parse` and
`to_html(&doc, &|id| Some(my_public_url(id)))`, which escapes all text and
attributes.

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
- Downloads are rate limited per user (600 per minute).
