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
    auth.limiter.reserve(&[(&format!("media-upload:{}", ctx.principal.id), 60)]).await?;
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

/// Reads the one `file` part into a staged file: its clean name, SHA-256, size
/// and first bytes. Any other part, or more than one file, is refused.
pub(super) async fn receive(
    media: &MediaState,
    multipart: &mut Multipart,
) -> AppResult<(Staged, String, String, u64, Vec<u8>)> {
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
