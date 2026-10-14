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

mod deletions;
mod detect;
mod download;
pub mod entity;
mod prune;
mod resource;
mod upload;
mod uploads;

pub use detect::*;
pub(crate) use download::*;
pub use prune::*;
pub use resource::*;
pub(crate) use upload::*;

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
