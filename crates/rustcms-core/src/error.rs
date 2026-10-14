//! Application error type and its Laravel-compatible JSON rendering.

use std::collections::BTreeMap;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

/// Result alias used across rustcms.
pub type AppResult<T> = Result<T, AppError>;

/// Field-level and form-level validation messages (rendered as HTTP 422).
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationErrors {
    /// Messages keyed by field name.
    pub errors: BTreeMap<String, Vec<String>>,
    /// Messages that do not belong to a single field.
    pub form_errors: Vec<String>,
}

impl ValidationErrors {
    /// Creates an empty error bag.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a message for `field`.
    pub fn add(&mut self, field: impl Into<String>, message: impl Into<String>) {
        self.errors.entry(field.into()).or_default().push(message.into());
    }

    /// Adds a form-level message.
    pub fn add_form(&mut self, message: impl Into<String>) {
        self.form_errors.push(message.into());
    }

    /// Returns `true` when no message was recorded.
    pub fn is_empty(&self) -> bool {
        self.errors.is_empty() && self.form_errors.is_empty()
    }

    /// Shorthand for a bag holding a single field message.
    pub fn field(field: impl Into<String>, message: impl Into<String>) -> Self {
        let mut bag = Self::new();
        bag.add(field, message);
        bag
    }

    /// Converts into `Err(AppError::Validation)` when not empty.
    pub fn into_result(self) -> AppResult<()> {
        if self.is_empty() { Ok(()) } else { Err(AppError::Validation(self)) }
    }
}

/// Every error a rustcms handler can return.
///
/// Internal details (database messages, panics) are logged, never rendered.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    /// 400: malformed request (e.g. invalid JSON).
    #[error("bad request: {0}")]
    BadRequest(String),
    /// 401: no valid credential.
    #[error("unauthenticated")]
    Unauthenticated,
    /// 403: authenticated but not allowed.
    #[error("forbidden")]
    Forbidden,
    /// The CSRF token is missing or wrong (403): fetch a new one, do not replay blindly.
    #[error("CSRF token mismatch")]
    CsrfMismatch,
    /// 404: resource or record not found (also used for records outside the user's scope).
    #[error("not found")]
    NotFound,
    /// 405: route exists but not for this method.
    #[error("method not allowed")]
    MethodNotAllowed,
    /// 409: the write could not run now (e.g. a lock wait timed out); nothing
    /// was applied and it may be retried. `code` is the stable error code.
    #[error("conflict: {code}")]
    Conflict {
        /// Stable machine-readable code (`write_conflict`, …).
        code: &'static str,
        /// Human message.
        message: &'static str,
    },
    /// 413: request body exceeds the configured limit.
    #[error("payload too large")]
    PayloadTooLarge,
    /// 429: rate limit exceeded; retry after the given number of seconds.
    #[error("too many requests")]
    TooManyRequests {
        /// Seconds until the limit resets (`Retry-After`).
        retry_after: u64,
    },
    /// 422: input rejected.
    #[error("validation failed")]
    Validation(ValidationErrors),
    /// 500: anything unexpected. The message is logged only.
    #[error("internal error: {0}")]
    Internal(String),
}

impl AppError {
    /// Wraps any displayable error as an internal error.
    pub fn internal(err: impl std::fmt::Display) -> Self {
        Self::Internal(err.to_string())
    }

    /// HTTP status for this error.
    pub fn status(&self) -> StatusCode {
        match self {
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::Unauthenticated => StatusCode::UNAUTHORIZED,
            Self::Forbidden | Self::CsrfMismatch => StatusCode::FORBIDDEN,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::Conflict { .. } => StatusCode::CONFLICT,
            Self::MethodNotAllowed => StatusCode::METHOD_NOT_ALLOWED,
            Self::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Self::Validation(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Self::TooManyRequests { .. } => StatusCode::TOO_MANY_REQUESTS,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// Stable machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            Self::BadRequest(_) => "bad_request",
            Self::Unauthenticated => "unauthenticated",
            Self::Forbidden => "forbidden",
            Self::CsrfMismatch => "csrf_mismatch",
            Self::NotFound => "not_found",
            Self::Conflict { code, .. } => code,
            Self::MethodNotAllowed => "method_not_allowed",
            Self::PayloadTooLarge => "payload_too_large",
            Self::Validation(_) => "validation_failed",
            Self::TooManyRequests { .. } => "too_many_requests",
            Self::Internal(_) => "internal_error",
        }
    }
}

impl From<ValidationErrors> for AppError {
    fn from(errors: ValidationErrors) -> Self {
        Self::Validation(errors)
    }
}

impl From<sea_orm::DbErr> for AppError {
    /// Lock waits that timed out, a busy SQLite database, deadlocks and
    /// serialization failures roll the write back: 409 `write_conflict` (nothing
    /// applied, retryable). Anything else is an internal error.
    fn from(err: sea_orm::DbErr) -> Self {
        if retryable(&err) {
            return Self::Conflict {
                code: "write_conflict",
                message: "Another change to the same data is in progress. Nothing was saved; try again.",
            };
        }
        Self::internal(err)
    }
}

/// Whether the database rolled the write back because of other writers: lock
/// wait timeouts, deadlocks, serialization failures, a busy SQLite database.
/// Decided by SQLSTATE (PostgreSQL) and error number (MySQL) where the driver
/// gives one, by the message otherwise.
fn retryable(err: &sea_orm::DbErr) -> bool {
    #[cfg(any(feature = "sqlite", feature = "postgres", feature = "mysql"))]
    if let Some(retryable) = retryable_code(err) {
        return retryable;
    }
    const MESSAGES: [&str; 6] = [
        "lock timeout",
        "deadlock detected",
        "could not serialize access",
        "database is locked",
        "Lock wait timeout exceeded",
        "Deadlock found when trying to get lock",
    ];
    let text = err.to_string();
    MESSAGES.iter().any(|m| text.contains(m))
}

/// The driver's verdict, when it has one (`None`: decide by the message).
#[cfg(any(feature = "sqlite", feature = "postgres", feature = "mysql"))]
fn retryable_code(err: &sea_orm::DbErr) -> Option<bool> {
    use sea_orm::{DbErr, RuntimeErr};
    if let DbErr::Exec(RuntimeErr::SqlxError(e))
    | DbErr::Query(RuntimeErr::SqlxError(e))
    | DbErr::Conn(RuntimeErr::SqlxError(e)) = err
        && let sea_orm::sqlx::Error::Database(db) = e.as_ref()
    {
        #[cfg(feature = "mysql")]
        if let Some(mysql) = db.try_downcast_ref::<sea_orm::sqlx::mysql::MySqlDatabaseError>() {
            // 1205 lock wait timeout, 1213 deadlock, 3572 NOWAIT.
            return Some(matches!(mysql.number(), 1205 | 1213 | 3572));
        }
        // PostgreSQL: lock timeout, deadlock, serialization failure.
        if matches!(db.code().as_deref(), Some("55P03" | "40P01" | "40001")) {
            return Some(true);
        }
    }
    None
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ErrorBody<'a> {
    message: &'a str,
    code: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    errors: Option<&'a BTreeMap<String, Vec<String>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    form_errors: Option<&'a Vec<String>>,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let message = match &self {
            Self::BadRequest(detail) => detail.as_str(),
            Self::MethodNotAllowed => "Method not allowed.",
            Self::Internal(detail) => {
                tracing::error!(%detail, "internal error");
                "Server error."
            }
            Self::Unauthenticated => "Unauthenticated.",
            Self::Forbidden => "This action is unauthorized.",
            Self::CsrfMismatch => "CSRF token mismatch.",
            Self::NotFound => "Not found.",
            Self::Conflict { message, .. } => message,
            Self::PayloadTooLarge => "Payload too large.",
            Self::Validation(_) => "The given data was invalid.",
            Self::TooManyRequests { .. } => "Too many attempts. Please try again later.",
        };
        let (errors, form_errors) = match &self {
            Self::Validation(v) => (Some(&v.errors), Some(&v.form_errors)),
            _ => (None, None),
        };
        let body = ErrorBody { message, code: self.code(), errors, form_errors };
        let mut response = (self.status(), axum::Json(body)).into_response();
        if let Self::TooManyRequests { retry_after } = self {
            response.headers_mut().insert(axum::http::header::RETRY_AFTER, retry_after.into());
        }
        response
    }
}
