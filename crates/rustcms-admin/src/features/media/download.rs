//! `GET {admin}/api/media/{id}/file`: authorized download with safe headers.

use super::*;

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
