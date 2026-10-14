//! Building blocks of JSON APIs: pagination, validated bodies, page responses.
//!
//! ```rust,ignore
//! async fn list(State(app): State<AppState>, page: Pagination) -> AppResult<Json<ApiPage<PostJson>>> {
//!     let select = post::Entity::find().order_by_desc(post::Column::Id);
//!     Ok(Json(ApiPage::fetch(select, app.db(), &page, PostJson::from).await?))
//! }
//!
//! async fn create(Validated(input): Validated<NewPost>) -> AppResult<StatusCode> {
//!     todo!()
//! }
//! ```

use axum::extract::{FromRequest, FromRequestParts, Request};
use axum::http::request::Parts;
use sea_orm::{ConnectionTrait, EntityTrait, FromQueryResult, PaginatorTrait, Select};
use serde::Serialize;
use serde::de::DeserializeOwned;

use super::Json;
use crate::error::{AppError, AppResult, ValidationErrors};

/// Highest `?page` accepted.
pub const MAX_PAGE: u64 = 10_000;
/// Highest `?per_page` accepted.
pub const MAX_PER_PAGE: u64 = 100;
/// `?per_page` when the request gives none.
pub const DEFAULT_PER_PAGE: u64 = 20;

/// `?page` (1–10,000, default 1) and `?per_page` (1–100, default 20) of a list.
/// A value out of range, not a number or given twice is a 422 on that field;
/// other query parameters are left to the handler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pagination {
    page: u64,
    per_page: u64,
}

impl Default for Pagination {
    fn default() -> Self {
        Self { page: 1, per_page: DEFAULT_PER_PAGE }
    }
}

impl Pagination {
    /// A page within the bounds above, or the field errors.
    pub fn new(page: u64, per_page: u64) -> Result<Self, ValidationErrors> {
        let mut errors = ValidationErrors::new();
        for (field, value, max) in [("page", page, MAX_PAGE), ("per_page", per_page, MAX_PER_PAGE)] {
            if !(1..=max).contains(&value) {
                errors.add(field, format!("must be a number from 1 to {max}"));
            }
        }
        if errors.is_empty() { Ok(Self { page, per_page }) } else { Err(errors) }
    }

    /// 1-based page.
    pub fn page(&self) -> u64 {
        self.page
    }

    /// Items per page.
    pub fn per_page(&self) -> u64 {
        self.per_page
    }

    /// Reads `page` and `per_page` from a raw (form-encoded) query string.
    pub fn from_query(query: Option<&str>) -> Result<Self, ValidationErrors> {
        let mut values: [Option<u64>; 2] = [None, None];
        let mut errors = ValidationErrors::new();
        for (key, value) in form_urlencoded::parse(query.unwrap_or_default().as_bytes()) {
            let (slot, max) = match key.as_ref() {
                "page" => (&mut values[0], MAX_PAGE),
                "per_page" => (&mut values[1], MAX_PER_PAGE),
                _ => continue,
            };
            if slot.is_some() {
                errors.add(key.as_ref(), "may only be given once");
                continue;
            }
            match value.parse::<u64>() {
                Ok(n) if (1..=max).contains(&n) => *slot = Some(n),
                _ => {
                    *slot = Some(0);
                    errors.add(key.as_ref(), format!("must be a number from 1 to {max}"));
                }
            }
        }
        if !errors.is_empty() {
            return Err(errors);
        }
        Self::new(values[0].unwrap_or(1), values[1].unwrap_or(DEFAULT_PER_PAGE))
    }
}

impl<S: Send + Sync> FromRequestParts<S> for Pagination {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, AppError> {
        Ok(Self::from_query(parts.uri.query())?)
    }
}

/// Paging data of an [`ApiPage`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiPageMeta {
    /// This page (1-based).
    pub page: u64,
    /// Items per page.
    pub per_page: u64,
    /// Items in all pages.
    pub total: u64,
    /// Last page (at least 1).
    pub last_page: u64,
}

/// A page of a list: `{data: [...], meta: {page, perPage, total, lastPage}}`.
#[derive(Debug, Clone, Serialize)]
pub struct ApiPage<T> {
    /// The items.
    pub data: Vec<T>,
    /// Paging data.
    pub meta: ApiPageMeta,
}

impl<T> ApiPage<T> {
    /// Runs `select` for `pagination` (and counts every item), turning each row
    /// into `T` with `map`. Order `select` (e.g. newest first, then by key):
    /// without an order, rows may move between pages. A page past the last one
    /// has no items and the same `meta`.
    pub async fn fetch<E, M, C>(
        select: Select<E>,
        db: &C,
        pagination: &Pagination,
        map: impl FnMut(M) -> T,
    ) -> AppResult<Self>
    where
        E: EntityTrait<Model = M>,
        M: FromQueryResult + Send + Sync,
        C: ConnectionTrait,
    {
        let paginator = select.paginate(db, pagination.per_page);
        let totals = paginator.num_items_and_pages().await?;
        let rows = paginator.fetch_page(pagination.page - 1).await?;
        Ok(Self {
            data: rows.into_iter().map(map).collect(),
            meta: ApiPageMeta {
                page: pagination.page,
                per_page: pagination.per_page,
                total: totals.number_of_items,
                last_page: totals.number_of_pages.max(1),
            },
        })
    }
}

/// An input that checks (and may normalize) itself after it is parsed.
pub trait Validate {
    /// The field errors of this input, if any (a 422).
    fn validate(&mut self) -> Result<(), ValidationErrors>;
}

/// A JSON body parsed like [`Json`] (413 oversized, 400 malformed, 422 when
/// it does not match `T`), then [validated](Validate) (422 with field errors).
#[derive(Debug, Clone)]
pub struct Validated<T>(pub T);

impl<T: DeserializeOwned + Validate, S: Send + Sync> FromRequest<S> for Validated<T> {
    type Rejection = AppError;

    async fn from_request(req: Request, state: &S) -> Result<Self, AppError> {
        let Json(mut value) = Json::<T>::from_request(req, state).await?;
        value.validate()?;
        Ok(Self(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pagination_reads_its_parameters_only() {
        assert_eq!(Pagination::from_query(None).unwrap(), Pagination::default());
        let p = Pagination::from_query(Some("q=x&page=3&per_page=50&sort=-id")).unwrap();
        assert_eq!((p.page(), p.per_page()), (3, 50));
        // Form-encoded, like every query string.
        assert_eq!(Pagination::from_query(Some("page=%32")).unwrap().page(), 2);
        for (query, field) in [
            ("%70age=0", "page"),
            ("page=1&page=2", "page"),
            ("page=0", "page"),
            ("page=abc", "page"),
            ("page=10001", "page"),
            ("per_page=101", "per_page"),
            ("per_page=", "per_page"),
        ] {
            let errors = Pagination::from_query(Some(query)).unwrap_err();
            assert!(errors.errors.contains_key(field), "{query}");
        }
    }
}
