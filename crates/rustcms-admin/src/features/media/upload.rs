//! `POST {admin}/api/media`: multipart upload, admission first, idempotent.

use super::*;

/// A staged upload: removed when dropped.
pub(super) struct Staged {
    path: PathBuf,
}

impl Drop for Staged {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// `POST {admin}/api/media` (multipart: one `file` part, and an optional
/// `collection` part naming a declared media collection).
pub(crate) async fn upload(
    State(app): State<AppState>,
    AdminCtx(ctx): AdminCtx,
    request: axum::extract::Request,
) -> AppResult<(StatusCode, Json<crate::http::DataResponse<MediaItem>>)> {
    // Admission first: nothing is parsed or read for users who may not upload.
    if !ctx.can(UPLOAD) {
        return Err(AppError::Forbidden);
    }
    let media = state_of(&app)?;
    let key = crate::http::idempotency_key(request.headers())?;
    let auth = app.service::<rustcms_auth::AuthState>().ok_or_else(|| AppError::internal("auth not registered"))?;
    auth.limiter.reserve(&[(&format!("media-upload:{}", ctx.principal.id), 60)]).await?;
    let _slot = media.uploads.try_acquire().map_err(|_| AppError::TooManyRequests { retry_after: 5 })?;
    let mut multipart = <Multipart as axum::extract::FromRequest<AppState>>::from_request(request, &app)
        .await
        .map_err(|_| ValidationErrors::field("file", "must be sent as multipart/form-data"))?;

    let Received { staged, name, sha256, size, head, collection } = receive(&media, &mut multipart).await?;
    // Collections sort the library: a name the panel does not declare is refused.
    if let Some(collection) = &collection
        && ctx.registry.media_collection_named(collection).is_none()
    {
        return Err(ValidationErrors::field("collection", "is not a media collection").into());
    }
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
        let mut request = serde_json::json!({ "sha256": sha256, "size": size, "name": name });
        // The collection is part of the request: the same key with another one is another request.
        if let Some(collection) = &collection {
            request["collection"] = serde_json::Value::String(collection.clone());
        }
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
        alt: Set(None),
        collection: Set(collection),
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
        return Err(AppError::Conflict {
            code: "write_conflict",
            reason: "upload_expired",
            message: "The upload took too long; send it again.",
        });
    }
    if let Some(claim) = &claim {
        crate::idempotency::complete(&txn, claim, &serde_json::json!({ "key": row.id.to_string() })).await?;
    }
    // The application's hooks, in the upload's transaction (an error undoes
    // it; the stored file is left to pruning, its intent still recorded),
    // with a context whose `conn()` is that transaction, as other hooks get.
    let txn = if ctx.registry.media_hooks().is_empty() {
        txn
    } else {
        let txn = std::sync::Arc::new(txn);
        let within = ctx.within(txn.clone());
        let change = MediaChange { event: MediaEvent::Uploaded, media: &row, previous: None };
        super::hooks::run(&within, &txn, change).await?;
        drop(within);
        std::sync::Arc::try_unwrap(txn).map_err(|_| AppError::internal("a media hook kept the upload's transaction"))?
    };
    let entry = rustcms_core::audit::AuditEntry::new("uploaded", SLUG)
        .key(row.id)
        .meta(UploadMeta { mime: &row.mime, size: row.size });
    ctx.audit(&txn, entry).await?;
    txn.commit().await?;
    Ok((StatusCode::CREATED, Json(crate::http::DataResponse { data: MediaItem::from(&row) })))
}

/// The committed upload for `claim`. Answered as the first response was: to
/// its uploader (the claim is theirs), while it is not trashed.
pub(super) async fn replay(ctx: &Ctx, claim: &crate::idempotency::Claim) -> AppResult<Option<MediaItem>> {
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

/// What an upload sends besides its file.
pub(super) struct Received {
    pub(super) staged: Staged,
    pub(super) name: String,
    pub(super) sha256: String,
    pub(super) size: u64,
    pub(super) head: Vec<u8>,
    /// The `collection` part, when sent and not empty (checked by the caller).
    pub(super) collection: Option<String>,
}

/// Reads the one `file` part into a staged file (its clean name, SHA-256,
/// size and first bytes) and the optional `collection` part, in any order.
/// Any other part, or either part twice, is refused.
pub(super) async fn receive(media: &MediaState, multipart: &mut Multipart) -> AppResult<Received> {
    const PARTS: &str = "must be one part named `file`, with at most one `collection`";
    let bad = |message: &str| AppError::from(ValidationErrors::field("file", message));
    // Over the body limit is a 413 wherever the parser notices it.
    let read_error = |e: axum::extract::multipart::MultipartError| {
        if e.status() == StatusCode::PAYLOAD_TOO_LARGE { AppError::PayloadTooLarge } else { bad("could not be read") }
    };
    let mut file = None;
    let mut collection: Option<Option<String>> = None;
    while let Some(mut field) = multipart.next_field().await.map_err(read_error)? {
        match field.name() {
            Some("file") if file.is_none() => file = Some(stage(media, &mut field, &bad, &read_error).await?),
            Some("collection") if collection.is_none() => {
                // A name, never more than its longest (plus a byte to notice it).
                let mut bytes = Vec::new();
                while let Some(chunk) = field.chunk().await.map_err(read_error)? {
                    bytes.extend_from_slice(&chunk);
                    if bytes.len() > MAX_COLLECTION {
                        return Err(ValidationErrors::field("collection", "is not a media collection").into());
                    }
                }
                let text = String::from_utf8(bytes)
                    .map_err(|_| AppError::from(ValidationErrors::field("collection", "is not a media collection")))?;
                collection = Some(Some(text).filter(|t| !t.is_empty()));
            }
            // A second collection is the collection's problem; anything else, the file's.
            Some("collection") => return Err(ValidationErrors::field("collection", PARTS).into()),
            _ => return Err(bad(PARTS)),
        }
    }
    let (staged, name, sha256, size, head) = file.ok_or_else(|| bad("is required"))?;
    if size == 0 {
        return Err(bad("is empty"));
    }
    Ok(Received { staged, name, sha256, size, head, collection: collection.flatten() })
}

/// Streams the `file` part to a staged file.
async fn stage(
    media: &MediaState,
    field: &mut axum::extract::multipart::Field<'_>,
    bad: &impl Fn(&str) -> AppError,
    read_error: &impl Fn(axum::extract::multipart::MultipartError) -> AppError,
) -> AppResult<(Staged, String, String, u64, Vec<u8>)> {
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
    Ok((staged, name, hex::encode(hasher.finalize()), size, head))
}

/// What the audit log keeps of an upload (never its name or content).
#[derive(serde::Serialize)]
struct UploadMeta<'a> {
    mime: &'a str,
    size: i64,
}
