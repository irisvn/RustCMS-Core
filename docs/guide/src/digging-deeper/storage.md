# File storage

`StorageProvider` binds the configured storage: a local directory, or a
private S3-compatible bucket. The [media library](../admin/media.md), exports
and imports keep their files there, and your own code can store files through
the same `Storage` trait.

## Quick start

Register the provider (the application template does):

<<< @/../../../examples/demo/src/bootstrap/providers.rs#register [src/bootstrap/providers.rs]

With no `[storage]` section, files go to `storage/app` on the local disk. To
use a bucket, build with the `s3` feature and set:

```toml
# config/storage.toml
disk = "s3"

[s3]
bucket = "my-app-media"
region = "eu-west-1"
```

## Configuration

`[storage]` (`config/storage.toml`; every key can be overridden from the
environment, e.g. `RUSTCMS_STORAGE__DISK=s3`, `RUSTCMS_STORAGE__S3__BUCKET=…`):

| Key | Default | |
|---|---|---|
| `disk` | `local` | `local`, or `s3` with the `s3` feature |
| `root` | `storage/app` | local disk root (outside the web root; symlinks inside it are refused); also holds the S3 staging folder |
| `max_upload_bytes` | 10 MiB | per upload (the route's body limit follows) |
| `max_pixels` | 40,000,000 | images, width × height |
| `max_concurrent_uploads` | 4 | uploads processed at once by each instance (one process-wide limit shared by all users; several instances each have their own); more answer 429. Each upload list of the admin panel sends at most 2 at a time and retries a 429 after `Retry-After` |
| `allowed_types` | `["image/png", "image/jpeg", "image/gif", "image/webp", "application/pdf"]` | MIME types, a subset of these five |
| `s3.bucket` | | private bucket |
| `s3.region` | `us-east-1` | |
| `s3.endpoint` | AWS | for S3-compatible services |
| `s3.allow_http` | `false` | plain HTTP endpoint (a local test server only) |
| `s3.timeout_secs` | `30` | longest wait for a control request (exists, delete, opening a download), retries included (1 to 600) |
| `s3.transfer_timeout_secs` | `600` | longest single request moving data (an upload part, a download body); from `timeout_secs` to 86400 |

S3 credentials come from the usual `AWS_*` variables (`AWS_ACCESS_KEY_ID`,
`AWS_SECRET_ACCESS_KEY`), never from the TOML files.

- The type of an upload is detected from its content, not from its name or
  the client's `Content-Type`, and must be in `allowed_types`. SVG, HTML,
  XML and scripts are always refused, whatever `allowed_types` says.
- Object keys are generated (`yyyy/mm/<uuid>.<ext>`), never taken from the
  client.
- Boot panics on an invalid `[storage]` section, an unknown `disk`, or a
  disk that cannot be opened (a missing `s3` feature, a bad root).

## Drivers

- **`local`**: files under `storage.root`. Every operation is anchored to that
  directory: no path, symbolic link or concurrent rename leads outside it.
  A file is written next to its target, synced, then renamed, so readers
  never see a partial file. Keep the directory out of the web root, back it
  up, and share it (one volume) when you run several instances.
- **`s3`** (feature `s3`): a private bucket on AWS S3 or any S3-compatible
  service (Cloudflare R2, MinIO, …). Set `s3.endpoint` for non-AWS services.
  Configure an *abort incomplete multipart uploads* lifecycle rule on the
  bucket (large files go up in parts; an upload interrupted by a crash leaves
  parts behind). Uploads are staged in `<storage.root>/.s3-staging/` (created
  private to the app's user; a symbolic link there is refused) before they
  are sent; `media:prune` removes the `*.upload` files interrupted uploads
  left there.

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

## Storing your own files

Use the bound storage from your own code: `rustcms::storage::of(&state)`
returns it as an `Arc<dyn Storage>` (or `None` without `StorageProvider`).
A sketch, storing a file you already wrote to disk and serving it later:

```rust
use rustcms::storage::{self, StorageKey};

let storage = storage::of(&state).ok_or_else(|| AppError::internal("no storage bound"))?;

// Store: a fresh key, then the file's bytes under it (copied; delete your file afterwards).
let key = StorageKey::generate("pdf")?;
storage.put_file(&key, &path, "application/pdf").await?;
// Save `key.as_str()` in your row.

// Serve: validate the stored key again, then stream the object.
let (body, length) = storage.open(&StorageKey::parse(&row.file_key)?).await?;
```

`StorageError` converts to `AppError` with `?`: a missing object is a 404,
anything else a 500 whose details go to the log only.

| `Storage` method | |
|---|---|
| `disk()` | the driver name (`local`, `s3`), stored with each object |
| `staging_dir()` | where to stage a file before `put_file` (same filesystem as the objects for `local`) |
| `put_file(&key, &path, content_type)` | stores the file at `path` under `key` |
| `open(&key)` | the object as an `axum::body::Body` and its length |
| `delete(&key)` | deletes it; deleting a missing object succeeds |
| `exists(&key)` | whether it exists |
| `sweep(older_than)` | removes staging and partial files older than `older_than`; returns how many |

| Other items | |
|---|---|
| `StorageKey::generate(ext)` | a new `yyyy/mm/<uuid>.<ext>` key (`ext`: 1 to 5 lowercase letters or digits) |
| `StorageKey::parse(text)` | validates a stored key; anything else is refused |
| `storage::connect(&config)` | builds a storage from a `StorageConfig` (tools, tests) |
| `StorageConfig` | the `[storage]` section, also bound by the provider |

::: warning Serving files is your responsibility
Your route decides who may read a file: check access before `open`, as for
any record. Set the response headers yourself: a fixed `Content-Type` you
detected when storing it, `X-Content-Type-Options: nosniff`, and
`Content-Disposition: attachment` for anything a browser could render.
Never build a key from client input: parse the key you stored.
:::

## Pruning

- Files of deleted media and abandoned uploads are removed by a background
  pass every minute, and by `rustcms media:prune` (it stops taking new work
  after five minutes; the rest waits for the next run).
- Exports and imports are removed by a background pass every 10 minutes; no
  command does it.

These passes start when the application boots (`serve`, `queue:work`). See
[Data retention](../operations/retention.md).

Files are always served through the application (no presigned URLs), so
authorization and safe response headers apply to every byte.

## Related

- [Media library](../admin/media.md)
- [Backup and restore](../operations/backup-restore.md)
- [Several instances](instances.md)
- [Configuration](../getting-started/configuration.md)
