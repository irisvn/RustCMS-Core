//! Media library: uploads stored through [`rustcms_core::storage`], served
//! only through the application (`GET {admin}/api/media/{id}/file`).
//!
//! Enable with [`crate::AdminProvider::media`] (requires the
//! [`StorageProvider`](rustcms_core::storage::StorageProvider)).
//!
//! An upload is recorded as an *intent* (committed) before its object is
//! written, and the intent is removed in the transaction that inserts the media
//! row: a crash in between leaves a record, and `media:prune` deletes the
//! object. Deleting a media record for good queues its object's removal in the
//! same transaction; a worker retries it until done.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use axum::extract::{Multipart, Path as UrlPath, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use chrono::{Duration, Utc};
use rustcms_core::storage::{Storage, StorageConfig, StorageKey};
use rustcms_core::{AppError, AppResult, AppState, Json, ValidationErrors};
use sea_orm::entity::prelude::*;
use sea_orm::{ActiveValue::Set, Condition, DatabaseTransaction, QueryOrder, QuerySelect, TransactionTrait};
use serde::Serialize;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use crate::context::Ctx;
use crate::field::Field;
use crate::http::AdminCtx;
use crate::policy::Policy;
use crate::resource::{Resource, ResourceMeta};
use crate::ui::{Table, TextColumn};
use crate::{Data, components};

/// Slug of the media resource.
pub const SLUG: &str = "media";

/// MIME types the panel may show inline, with their file extensions. Anything
/// else is served as an attachment; nothing outside this map is accepted.
const TYPES: [(&str, &str, bool); 5] = [
    ("image/png", "png", true),
    ("image/jpeg", "jpg", true),
    ("image/gif", "gif", true),
    ("image/webp", "webp", true),
    ("application/pdf", "pdf", false),
];

/// Uploads older than this without a media row are abandoned.
const ABANDONED_AFTER_MINUTES: i64 = 60;
/// Longest kept original file name, in characters.
const MAX_NAME: usize = 255;

pub mod entity {
    //! The `media` table.
    use sea_orm::entity::prelude::*;

    /// A stored file.
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "media")]
    pub struct Model {
        /// Id (UUID v7).
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        /// Storage driver (`local`, `s3`).
        pub disk: String,
        /// Object key in the storage.
        pub key: String,
        /// Original file name, cleaned.
        pub name: String,
        /// Detected MIME type.
        pub mime: String,
        /// Size in bytes.
        pub size: i64,
        /// SHA-256 of the content, hex.
        pub sha256: String,
        /// Image width in pixels.
        pub width: Option<i32>,
        /// Image height in pixels.
        pub height: Option<i32>,
        /// Uploader.
        pub owner_id: i64,
        /// Upload time.
        pub created_at: DateTimeUtc,
        /// Trash marker (soft deletes).
        pub deleted_at: Option<DateTimeUtc>,
    }

    /// No relations.
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

mod uploads {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "media_uploads")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        pub disk: String,
        pub key: String,
        pub created_at: DateTimeUtc,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

mod deletions {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "media_deletions")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        pub disk: String,
        pub key: String,
        pub attempts: i32,
        pub next_at: DateTimeUtc,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// The media library resource: listed, viewed, trashed and restored like any
/// resource; created only by uploads and never edited.
pub struct MediaResource;

/// `media.{ability}`; uploads need `media.create` (checked by the upload
/// route); the generic create and update endpoints are always refused.
pub struct MediaPolicy;

#[async_trait]
impl Policy for MediaPolicy {
    async fn allows(&self, ctx: &Ctx, ability: &str, _record: Option<&Data>) -> bool {
        !matches!(ability, "create" | "update") && ctx.can(&format!("{SLUG}.{ability}"))
    }

    async fn admits(&self, ctx: &Ctx, ability: &str) -> bool {
        self.allows(ctx, ability, None).await
    }

    fn view_depends_on_record(&self) -> bool {
        false
    }
}

#[async_trait]
impl Resource for MediaResource {
    type Entity = entity::Entity;
    const SLUG: &'static str = SLUG;

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Media").plural("Media").title("name").icon("image").group("Content").soft_deletes()
    }

    fn fields() -> Vec<Field> {
        let shown = |column: entity::Column| Field::column(column).guarded().readable(|_| true);
        vec![
            shown(entity::Column::Name).searchable().sortable(),
            shown(entity::Column::Mime).filterable(),
            shown(entity::Column::Size).sortable(),
            shown(entity::Column::Width),
            shown(entity::Column::Height),
            shown(entity::Column::OwnerId),
            shown(entity::Column::CreatedAt).sortable(),
            Field::column(entity::Column::DeletedAt).guarded().readable(|_| true),
        ]
    }

    fn form() -> crate::Form {
        crate::Form::new().schema(components![crate::TextInput::make("name")])
    }

    fn table() -> Table {
        Table::new()
            .columns(vec![
                TextColumn::make("name"),
                TextColumn::make("mime"),
                TextColumn::make("size"),
                crate::DateColumn::make("created_at"),
            ])
            .default_sort("-created_at")
    }

    fn policy() -> Arc<dyn Policy> {
        Arc::new(MediaPolicy)
    }

    /// Own uploads only, unless `media.manage_all`.
    fn scope(query: sea_orm::Select<entity::Entity>, ctx: &Ctx) -> sea_orm::Select<entity::Entity> {
        if ctx.can("media.manage_all") {
            return query;
        }
        let owner = ctx.principal.id.parse::<i64>().unwrap_or(-1);
        query.filter(entity::Column::OwnerId.eq(owner))
    }

    async fn after_force_delete(_ctx: &Ctx, txn: &DatabaseTransaction, record: &entity::Model) -> AppResult<()> {
        queue_deletion(txn, &record.disk, &record.key).await
    }
}

async fn queue_deletion(txn: &DatabaseTransaction, disk: &str, key: &str) -> AppResult<()> {
    deletions::ActiveModel {
        id: Set(Uuid::now_v7()),
        disk: Set(disk.to_owned()),
        key: Set(key.to_owned()),
        attempts: Set(0),
        next_at: Set(Utc::now()),
    }
    .insert(txn)
    .await?;
    Ok(())
}

/// A media item as returned to the panel (never the storage key).
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct MediaItem {
    /// Id (UUID).
    pub id: String,
    /// Original file name.
    pub name: String,
    /// MIME type.
    pub mime: String,
    /// Size in bytes.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub size: i64,
    /// Image width.
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<i32>,
    /// Image height.
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<i32>,
    /// Shown inline (an image) rather than downloaded.
    pub inline: bool,
}

impl From<&entity::Model> for MediaItem {
    fn from(m: &entity::Model) -> Self {
        Self {
            id: m.id.to_string(),
            name: m.name.clone(),
            mime: m.mime.clone(),
            size: m.size,
            width: m.width,
            height: m.height,
            inline: inline(&m.mime),
        }
    }
}

fn inline(mime: &str) -> bool {
    TYPES.iter().any(|(t, _, inline)| *t == mime && *inline)
}

/// Services of the media library (bound by [`crate::AdminProvider::media`]).
pub(crate) struct MediaState {
    pub(crate) storage: Arc<dyn Storage>,
    pub(crate) config: StorageConfig,
    pub(crate) uploads: tokio::sync::Semaphore,
}

/// The media library's services; 404 when it is not enabled.
fn state_of(app: &AppState) -> AppResult<Arc<MediaState>> {
    app.service::<MediaState>().ok_or(AppError::NotFound)
}

/// A file name safe to keep and to send back: no path, controls, bidi marks or
/// quotes; at most 255 characters; never empty.
pub fn clean_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or_default();
    let cleaned: String = base
        .chars()
        .filter(|c| {
            !c.is_control()
                && !matches!(c, '"' | '\\' | ';' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
        .take(MAX_NAME)
        .collect();
    let trimmed = cleaned.trim().trim_start_matches('.');
    if trimmed.is_empty() { "file".to_owned() } else { trimmed.to_owned() }
}

/// `Content-Disposition` for a stored file: inline for images, attachment otherwise,
/// with an ASCII fallback and the UTF-8 name (RFC 6266 / 5987).
fn disposition(name: &str, inline: bool) -> HeaderValue {
    let ascii: String = name
        .chars()
        .map(|c| if (c.is_ascii_graphic() || c == ' ') && c != '"' && c != '\\' { c } else { '_' })
        .collect();
    let encoded: String = name
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect();
    let kind = if inline { "inline" } else { "attachment" };
    HeaderValue::from_str(&format!("{kind}; filename=\"{ascii}\"; filename*=UTF-8''{encoded}"))
        .unwrap_or_else(|_| HeaderValue::from_static("attachment"))
}

/// The detected type of `head` (magic bytes), if accepted.
fn detect(head: &[u8], allowed: &[String]) -> Option<(&'static str, &'static str)> {
    let kind = infer::get(head)?;
    TYPES
        .iter()
        .find(|(mime, _, _)| *mime == kind.mime_type() && allowed.iter().any(|a| a == mime))
        .map(|(mime, ext, _)| (*mime, *ext))
}

/// Whether the file ends as its type requires (a header alone is not a file):
/// PNG with `IEND`, JPEG with `FFD9`, GIF with `;`, WebP with a RIFF size
/// matching the file, PDF with `%%EOF` near the end.
fn complete(mime: &str, path: &Path, size: u64) -> AppResult<bool> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path).map_err(AppError::internal)?;
    let mut head = [0u8; 12];
    let n = file.read(&mut head).map_err(AppError::internal)?;
    let tail_len = size.min(1024);
    file.seek(SeekFrom::End(-(tail_len as i64))).map_err(AppError::internal)?;
    let mut tail = vec![0u8; tail_len as usize];
    file.read_exact(&mut tail).map_err(AppError::internal)?;
    let ends = |suffix: &[u8]| tail.ends_with(suffix);
    Ok(match mime {
        "image/png" => ends(b"IEND\xaeB`\x82"),
        "image/jpeg" => ends(b"\xff\xd9"),
        "image/gif" => ends(b";"),
        "image/webp" => n >= 8 && u64::from(u32::from_le_bytes([head[4], head[5], head[6], head[7]])) + 8 == size,
        "application/pdf" => tail.windows(5).any(|w| w == b"%%EOF"),
        _ => false,
    })
}

/// A staged upload: removed when dropped.
struct Staged {
    path: PathBuf,
}

impl Drop for Staged {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// `POST {admin}/api/media` (multipart, one `file` part).
pub(crate) async fn upload(
    State(app): State<AppState>,
    AdminCtx(ctx): AdminCtx,
    request: axum::extract::Request,
) -> AppResult<(StatusCode, Json<crate::http::DataResponse<MediaItem>>)> {
    // Admission first: nothing is parsed or read for users who may not upload.
    if !ctx.can("media.create") {
        return Err(AppError::Forbidden);
    }
    let media = state_of(&app)?;
    let key = crate::http::idempotency_key(request.headers())?;
    let auth = app.service::<rustcms_auth::AuthState>().ok_or_else(|| AppError::internal("auth not registered"))?;
    auth.limiter.reserve(&[(&format!("media-upload:{}", ctx.principal.id), 60)])?;
    let _slot = media.uploads.try_acquire().map_err(|_| AppError::TooManyRequests { retry_after: 5 })?;
    let mut multipart = <Multipart as axum::extract::FromRequest<AppState>>::from_request(request, &app)
        .await
        .map_err(|_| ValidationErrors::field("file", "must be sent as multipart/form-data"))?;

    let (staged, name, sha256, size, head) = receive(&media, &mut multipart).await?;
    let mut errors = ValidationErrors::new();
    let Some((mime, ext)) = detect(&head, &media.config.allowed_types) else {
        errors.add("file", "is not an accepted file type");
        return Err(errors.into());
    };
    if !complete(mime, &staged.path, size)? {
        return Err(ValidationErrors::field("file", "is not a complete file of its type").into());
    }
    let (width, height) = if mime.starts_with("image/") {
        let size =
            imagesize::size(&staged.path).map_err(|_| ValidationErrors::field("file", "is not a valid image"))?;
        if size.width == 0 || size.height == 0 {
            return Err(ValidationErrors::field("file", "is not a valid image").into());
        }
        if (size.width as u64).saturating_mul(size.height as u64) > media.config.max_pixels {
            return Err(ValidationErrors::field("file", "has too many pixels").into());
        }
        (i32::try_from(size.width).ok(), i32::try_from(size.height).ok())
    } else {
        (None, None)
    };

    let claim = key.as_deref().map(|key| {
        let request = serde_json::json!({ "sha256": sha256, "size": size, "name": name });
        crate::idempotency::Claim::new(&ctx.principal, SLUG, "upload", key, &request)
    });
    // A retry of a committed upload: answer with it, store nothing.
    if let Some(claim) = &claim
        && let Some(existing) = replay(&ctx, claim).await?
    {
        return Ok((StatusCode::CREATED, Json(crate::http::DataResponse { data: existing })));
    }

    // Intent first (committed), then the object, then the row (with the claim).
    let storage_key = StorageKey::generate(ext)?;
    let intent = Uuid::now_v7();
    uploads::ActiveModel {
        id: Set(intent),
        disk: Set(media.storage.disk().to_owned()),
        key: Set(storage_key.as_str().to_owned()),
        created_at: Set(Utc::now()),
    }
    .insert(&ctx.db)
    .await?;
    media.storage.put_file(&storage_key, &staged.path, mime).await?;

    let txn = rustcms_core::db::begin_write(&ctx.db).await?;
    if claim.is_some() {
        crate::idempotency::bound_claim_wait(&txn).await?;
    }
    if let Some(claim) = &claim
        && !crate::idempotency::acquire(&txn, claim).await?
    {
        // A concurrent identical upload won: drop ours (its intent stays until
        // the file is gone, so a failed removal is retried by pruning).
        txn.rollback().await?;
        if media.storage.delete(&storage_key).await.is_ok() {
            uploads::Entity::delete_by_id(intent).exec(&ctx.db).await?;
        }
        let existing = replay(&ctx, claim).await?.ok_or(AppError::NotFound)?;
        return Ok((StatusCode::CREATED, Json(crate::http::DataResponse { data: existing })));
    }
    let owner = ctx.principal.id.parse::<i64>().map_err(AppError::internal)?;
    let row = entity::ActiveModel {
        id: Set(Uuid::now_v7()),
        disk: Set(media.storage.disk().to_owned()),
        key: Set(storage_key.as_str().to_owned()),
        name: Set(name),
        mime: Set(mime.to_owned()),
        size: Set(i64::try_from(size).map_err(AppError::internal)?),
        sha256: Set(sha256),
        width: Set(width),
        height: Set(height),
        owner_id: Set(owner),
        created_at: Set(Utc::now()),
        deleted_at: Set(None),
    }
    .insert(&txn)
    .await?;
    // The intent must still be ours: pruning may have reclaimed (and deleted)
    // the file of an upload that took too long.
    if uploads::Entity::delete_by_id(intent).exec(&txn).await?.rows_affected != 1 {
        txn.rollback().await?;
        return Err(AppError::Conflict { code: "write_conflict", message: "The upload took too long; send it again." });
    }
    if let Some(claim) = &claim {
        crate::idempotency::complete(&txn, claim, &serde_json::json!({ "key": row.id.to_string() })).await?;
    }
    txn.commit().await?;
    Ok((StatusCode::CREATED, Json(crate::http::DataResponse { data: MediaItem::from(&row) })))
}

/// The committed upload for `claim`. Answered as the first response was: to
/// its uploader (the claim is theirs), while it is not trashed.
async fn replay(ctx: &Ctx, claim: &crate::idempotency::Claim) -> AppResult<Option<MediaItem>> {
    let txn = ctx.db.begin().await?;
    let found = crate::idempotency::existing(&txn, claim).await?;
    txn.commit().await?;
    let Some(receipt) = found else { return Ok(None) };
    let id = receipt["key"].as_str().and_then(|id| Uuid::parse_str(id).ok());
    let id = id.ok_or_else(|| AppError::internal("upload receipt without a key"))?;
    let owner = ctx.principal.id.parse::<i64>().unwrap_or(-1);
    let model = entity::Entity::find_by_id(id)
        .filter(entity::Column::OwnerId.eq(owner))
        .filter(entity::Column::DeletedAt.is_null())
        .one(&ctx.db)
        .await?
        .ok_or(AppError::NotFound)?;
    Ok(Some(MediaItem::from(&model)))
}

/// Reads the one `file` part into a staged file: its clean name, SHA-256, size
/// and first bytes. Any other part, or more than one file, is refused.
async fn receive(media: &MediaState, multipart: &mut Multipart) -> AppResult<(Staged, String, String, u64, Vec<u8>)> {
    let bad = |message: &str| AppError::from(ValidationErrors::field("file", message));
    // Over the body limit is a 413 wherever the parser notices it.
    let read_error = |e: axum::extract::multipart::MultipartError| {
        if e.status() == StatusCode::PAYLOAD_TOO_LARGE { AppError::PayloadTooLarge } else { bad("could not be read") }
    };
    let mut field = multipart.next_field().await.map_err(read_error)?.ok_or_else(|| bad("is required"))?;
    if field.name() != Some("file") {
        return Err(bad("must be the only part, named `file`"));
    }
    let original = field.file_name().unwrap_or("file");
    if original.chars().count() > MAX_NAME {
        return Err(bad("has a name longer than 255 characters"));
    }
    let name = clean_name(original);
    let staged = Staged { path: media.storage.staging_dir().join(format!("{}.upload", Uuid::now_v7())) };
    tokio::fs::create_dir_all(staged.path.parent().unwrap_or(Path::new("."))).await.map_err(AppError::internal)?;
    let mut out = tokio::fs::File::create(&staged.path).await.map_err(AppError::internal)?;
    let mut hasher = Sha256::new();
    let mut size = 0u64;
    let mut head = Vec::with_capacity(8192);
    while let Some(chunk) = field.chunk().await.map_err(read_error)? {
        size += chunk.len() as u64;
        if size > media.config.max_upload_bytes {
            return Err(AppError::PayloadTooLarge);
        }
        if head.len() < 8192 {
            head.extend_from_slice(&chunk[..chunk.len().min(8192 - head.len())]);
        }
        hasher.update(&chunk);
        out.write_all(&chunk).await.map_err(AppError::internal)?;
    }
    out.flush().await.map_err(AppError::internal)?;
    drop(field);
    if multipart.next_field().await.map_err(read_error)?.is_some() {
        return Err(bad("must be the only part, named `file`"));
    }
    if size == 0 {
        return Err(bad("is empty"));
    }
    Ok((staged, name, hex::encode(hasher.finalize()), size, head))
}

/// `GET {admin}/api/media/{id}/file`: the bytes, for users who may view the
/// record (scope and policy; trashed records are not served).
pub(crate) async fn file(
    State(app): State<AppState>,
    AdminCtx(ctx): AdminCtx,
    UrlPath(id): UrlPath<String>,
) -> AppResult<Response> {
    let media = state_of(&app)?;
    let auth = app.service::<rustcms_auth::AuthState>().ok_or_else(|| AppError::internal("auth not registered"))?;
    auth.limiter.reserve(&[(&format!("media-download:{}", ctx.principal.id), 600)])?;
    let resource = ctx.registry.get(SLUG).ok_or(AppError::NotFound)?;
    resource.find(&ctx, &id).await?;
    let id = Uuid::parse_str(&id).map_err(|_| AppError::NotFound)?;
    // Trashed since the check above: not served either.
    let model = entity::Entity::find_by_id(id)
        .filter(entity::Column::DeletedAt.is_null())
        .one(&ctx.db)
        .await?
        .ok_or(AppError::NotFound)?;
    if model.disk != media.storage.disk() {
        return Err(AppError::internal(format!("media on disk `{}` is not available", model.disk)));
    }
    let (body, len) = media.storage.open(&StorageKey::parse(&model.key)?).await?;
    let mut response = body.into_response();
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_str(&model.mime).map_err(AppError::internal)?);
    headers.insert(header::CONTENT_LENGTH, len.into());
    headers.insert(header::CONTENT_DISPOSITION, disposition(&model.name, inline(&model.mime)));
    headers.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    headers.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static("sandbox; default-src 'none'"));
    Ok(response)
}

/// Removes queued objects and abandoned uploads; returns how many objects were deleted.
pub async fn prune(db: &DatabaseConnection, storage: &dyn Storage) -> AppResult<u64> {
    let mut deleted = 0;
    let due = deletions::Entity::find()
        .filter(deletions::Column::NextAt.lte(Utc::now()))
        .filter(deletions::Column::Disk.eq(storage.disk()))
        .order_by_asc(deletions::Column::NextAt)
        .limit(100)
        .all(db)
        .await?;
    for job in due {
        let result = match StorageKey::parse(&job.key) {
            Ok(key) => storage.delete(&key).await.map_err(|e| e.to_string()),
            // Not a key this framework writes: nothing to delete safely.
            Err(err) => Err(err.to_string()),
        };
        match result {
            Ok(()) => {
                deletions::Entity::delete_by_id(job.id).exec(db).await?;
                deleted += 1;
            }
            Err(err) => {
                tracing::warn!(key = %job.key, %err, "media object deletion failed; will retry");
                let backoff = Duration::minutes(i64::from(job.attempts.clamp(0, 10) + 1).pow(2));
                deletions::Entity::update_many()
                    .col_expr(deletions::Column::Attempts, Expr::value(job.attempts.saturating_add(1)))
                    .col_expr(deletions::Column::NextAt, Expr::value(Utc::now() + backoff))
                    .filter(deletions::Column::Id.eq(job.id))
                    .exec(db)
                    .await?;
            }
        }
    }
    let abandoned = uploads::Entity::find()
        .filter(
            Condition::all()
                .add(uploads::Column::CreatedAt.lte(Utc::now() - Duration::minutes(ABANDONED_AFTER_MINUTES)))
                .add(uploads::Column::Disk.eq(storage.disk())),
        )
        .limit(100)
        .all(db)
        .await?;
    for intent in abandoned {
        // Reclaim the intent first: an upload finishing now then finds it gone
        // and fails instead of committing a row whose file is being deleted.
        let txn = rustcms_core::db::begin_write(db).await?;
        let reclaimed = uploads::Entity::delete_by_id(intent.id).exec(&txn).await?.rows_affected == 1;
        let used = entity::Entity::find().filter(entity::Column::Key.eq(&intent.key)).one(&txn).await?.is_some();
        txn.commit().await?;
        if reclaimed
            && !used
            && let Ok(key) = StorageKey::parse(&intent.key)
        {
            storage.delete(&key).await?;
            deleted += 1;
        }
    }
    // Staged and partial files of interrupted uploads.
    deleted += storage.sweep(std::time::Duration::from_secs(ABANDONED_AFTER_MINUTES as u64 * 60)).await?;
    Ok(deleted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_cleaned() {
        assert_eq!(clean_name("../../etc/passwd"), "passwd");
        assert_eq!(clean_name("C:\\Users\\me\\photo.png"), "photo.png");
        assert_eq!(clean_name("a\u{202e}gnp.exe"), "agnp.exe");
        assert_eq!(clean_name("x\"; y\n.png"), "x y.png");
        assert_eq!(clean_name("..."), "file");
        assert_eq!(clean_name(""), "file");
        assert_eq!(clean_name(&"é".repeat(300)).chars().count(), 255);
    }

    #[test]
    fn dispositions_are_safe() {
        let value = disposition("ảnh \"x\".png", true);
        let text = value.to_str().unwrap();
        assert!(
            text.starts_with("inline; filename=\"_nh _x_.png\"; filename*=UTF-8''%E1%BA%A3nh%20%22x%22.png"),
            "{text}"
        );
        assert!(disposition("doc.pdf", false).to_str().unwrap().starts_with("attachment;"));
    }

    #[test]
    fn types_come_from_content_only() {
        let allowed: Vec<String> = TYPES.iter().map(|(t, _, _)| (*t).to_owned()).collect();
        let png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";
        assert_eq!(detect(png, &allowed), Some(("image/png", "png")));
        assert_eq!(detect(b"%PDF-1.7\n", &allowed), Some(("application/pdf", "pdf")));
        for refused in
            [&b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>"[..], b"<!doctype html><script>", b"plain text", b""]
        {
            assert_eq!(detect(refused, &allowed), None, "{:?}", String::from_utf8_lossy(refused));
        }
        // A type outside the configured list is refused.
        assert_eq!(detect(png, &["application/pdf".to_owned()]), None);
        assert!(inline("image/png") && !inline("application/pdf") && !inline("image/svg+xml"));
    }
}
