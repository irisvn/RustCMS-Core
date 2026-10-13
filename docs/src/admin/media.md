# Media, uploads & rich text

## Enabling

```rust,ignore
app.provider(rustcms::storage::StorageProvider);          // `[storage]`
app.provider(AdminProvider::new().media().resource::<PostResource>());
```

`media()` adds the **Media** resource (permissions `media.view_any`, `media.view`,
`media.create`, `media.delete`, `media.restore`, `media.force_delete`,
`media.manage_all`), the upload route and `media:prune`.

## Storage

`[storage]` (`config/storage.toml`):

| Key | Default | |
|---|---|---|
| `disk` | `local` | `local`, or `s3` with the `s3` feature |
| `root` | `storage/app` | local disk root (outside the web root; symlinks inside it are refused) |
| `max_upload_bytes` | 10 MiB | per upload (the route's body limit follows) |
| `max_pixels` | 40,000,000 | images, width × height |
| `max_concurrent_uploads` | 4 | more answer 429 |
| `allowed_types` | png, jpeg, gif, webp, pdf | subset of those five |
| `s3.bucket`, `s3.region`, `s3.endpoint`, `s3.allow_http` | | private bucket; credentials from `AWS_*` variables |

Object keys are generated (`yyyy/mm/<uuid>.<ext>`), never taken from the client.

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
unless `media.manage_all`). Trashed media is not served. Headers:
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
elements. Headless apps render it with `rustcms_admin::rich_text::parse` and
`to_html(&doc, &|id| Some(my_public_url(id)))`, which escapes all text and
attributes.

## Limits and operations

- Referencing media needs `media.view_any` and `media.view` (the same check as
  any relationship). Records a user cannot see are never locked.
- A write keeps the permissions it started with: revoking `media.create` while
  an upload is being received does not stop that upload (the 0.1 rule for
  writes in flight).
- Uploads must finish within an hour: pruning reclaims older unfinished ones, and
  such an upload then fails with 409 instead of committing.
- S3: configure an *abort incomplete multipart uploads* lifecycle rule on the
  bucket (a cancelled request cannot always abort its upload). The bucket is not
  probed at boot; the first upload reports a misconfiguration.
- Local disk: files are synced before their row commits; staged and partial files
  of interrupted uploads are removed by `media:prune` after an hour.
- Downloads are rate limited per user (600 per minute).
