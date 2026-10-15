//! What the API returns for a post, like a Laravel API resource (wire format:
//! ids as strings).

use serde::Serialize;

use crate::models::post;

#[derive(Serialize)]
pub struct PostJson {
    id: String,
    title: String,
    body: Option<String>,
    published: bool,
    published_at: Option<String>,
}

impl From<post::Model> for PostJson {
    fn from(p: post::Model) -> Self {
        Self {
            id: p.id.to_string(),
            title: p.title,
            body: p.body,
            published: p.published,
            published_at: p.published_at.map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
        }
    }
}
