# File storage

`StorageProvider` binds the configured storage; the [media library](../admin/media.md)
keeps its files there.

```rust,ignore
app.provider(rustcms::storage::StorageProvider);   // reads `[storage]`
```

## Configuration

`[storage]` (`config/storage.toml`):

| Key | Default | |
|---|---|---|
| `disk` | `local` | `local`, or `s3` with the `s3` feature |
| `root` | `storage/app` | local disk root (outside the web root; symlinks inside it are refused) |
| `max_upload_bytes` | 10 MiB | per upload (the route's body limit follows) |
| `max_pixels` | 40,000,000 | images, width × height |
| `max_concurrent_uploads` | 4 | more answer 429 |
| `allowed_types` | `["image/png", "image/jpeg", "image/gif", "image/webp", "application/pdf"]` | MIME types, a subset of these five |
| `s3.bucket`, `s3.region`, `s3.endpoint`, `s3.allow_http` | | private bucket; credentials from `AWS_*` variables |

Object keys are generated (`yyyy/mm/<uuid>.<ext>`), never taken from the client.

## Drivers

- **`local`**: files under `storage.root`. Every operation is anchored to that
  directory: no path, symbolic link or concurrent rename leads outside it.
  Keep the directory out of the web root, back it up, and share it (one
  volume) when you run several instances.
- **`s3`** (feature `s3`): a private bucket on AWS S3 or any S3-compatible
  service (MinIO, Cloudflare R2, …). Credentials come from the usual `AWS_*`
  variables; set `s3.endpoint` for non-AWS services. Configure an *abort
  incomplete multipart uploads* lifecycle rule on the bucket.

Files are always served through the application (no presigned URLs), so
authorization and safe response headers apply to every byte.
