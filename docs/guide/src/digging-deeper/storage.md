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
| `max_concurrent_uploads` | 4 | uploads processed at once by each instance (one process-wide limit shared by all users; several instances each have their own); more answer 429. Each upload list of the admin panel sends at most 2 at a time and retries a 429 after `Retry-After` |
| `allowed_types` | `["image/png", "image/jpeg", "image/gif", "image/webp", "application/pdf"]` | MIME types, a subset of these five |
| `s3.bucket`, `s3.region`, `s3.endpoint`, `s3.allow_http` | | private bucket; credentials from `AWS_*` variables |
| `s3.timeout_secs` | `30` | longest wait for a control request (exists, delete, opening a download), retries included (1 to 600) |
| `s3.transfer_timeout_secs` | `600` | longest single request moving data (an upload part, a download body); from `timeout_secs` to 86400 |

Object keys are generated (`yyyy/mm/<uuid>.<ext>`), never taken from the client.

## Drivers

- **`local`**: files under `storage.root`. Every operation is anchored to that
  directory: no path, symbolic link or concurrent rename leads outside it.
  Keep the directory out of the web root, back it up, and share it (one
  volume) when you run several instances.
- **`s3`** (feature `s3`): a private bucket on AWS S3 or any S3-compatible
  service (Cloudflare R2, MinIO, …). Credentials come from the usual `AWS_*`
  variables; set `s3.endpoint` for non-AWS services. Configure an *abort
  incomplete multipart uploads* lifecycle rule on the bucket (large files go
  up in parts; an upload interrupted by a crash leaves parts behind).
  Uploads are staged in `<storage.root>/.s3-staging/` (created private to
  the app's user; a symbolic link there is refused) before they are sent;
  `media:prune` removes the `*.upload` files interrupted uploads left there.

### Cloudflare R2

```toml
# config/storage.toml
disk = "s3"

[s3]
bucket = "my-app-media"
region = "auto"
endpoint = "https://<account-id>.r2.cloudflarestorage.com"
```

```sh
AWS_ACCESS_KEY_ID=<R2 access key id>
AWS_SECRET_ACCESS_KEY=<R2 secret access key>
```

Create an R2 API token with *Object Read & Write* on that bucket only.

### Other S3-compatible services

Set `endpoint` to the service's URL (path-style requests are used) and its
`region` (often `us-east-1`). `allow_http = true` only for a local test
server; production endpoints use HTTPS.

### Failures

A bucket that does not answer fails a control request within
`s3.timeout_secs` (retries included). Data moves under
`s3.transfer_timeout_secs` per request: size it for your largest part
(uploads go in 10 MiB parts) or download over the slowest link you accept.
`http.timeout_secs` (408) bounds the whole HTTP request separately; keep it
above `s3.timeout_secs` so a slow bucket shows as a 500, not a 408.

- **Before the response starts** (an upload, or opening a download): the
  client gets 500, and the error's details (endpoint, key, credentials) go to
  the log only.
- **While a download streams**: the connection is closed before the announced
  `Content-Length` is reached, so the client sees an incomplete download,
  never an error page inside the file.

`media:prune` stops taking new work after five minutes; the rest waits for
the next run.

Files are always served through the application (no presigned URLs), so
authorization and safe response headers apply to every byte.
