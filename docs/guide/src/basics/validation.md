# Validation & pagination

## Validating input

`Validated<T>` reads the JSON body, then calls `T::validate`:

```rust,ignore
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewPost {
    pub title: String,
    pub body: Option<String>,
}

impl Validate for NewPost {
    fn validate(&mut self) -> Result<(), ValidationErrors> {
        let mut errors = ValidationErrors::new();
        self.title = self.title.trim().to_owned();          // normalize
        if self.title.is_empty() {
            errors.add("title", "is required");
        } else if self.title.chars().count() > 200 {
            errors.add("title", "may not be longer than 200 characters");
        }
        if errors.is_empty() { Ok(()) } else { Err(errors) }
    }
}

pub async fn create(
    State(app): State<AppState>,
    auth: Auth,
    _: Throttle<30>,                       // 30 a minute per user, counted before the body is read
    Validated(input): Validated<NewPost>,  // parsed, then NewPost::validate
) -> AppResult<(StatusCode, Json<PostJson>)> {
    // …
}
```

| Body | Answer |
|---|---|
| larger than the limit | 413 |
| not JSON | 400 |
| JSON, but not a `T` (use `deny_unknown_fields`) | 422 with a form error |
| `validate` returns errors | 422 with field errors |

Messages are plain English (`"is required"`). Using the framework's wording
gives each message a stable code clients can translate
([error codes](../reference/error-codes.md)); other text gets the code `custom`.

## Pagination

```rust,ignore
pub async fn list(State(app): State<AppState>, page: Pagination) -> AppResult<Json<ApiPage<PostJson>>> {
    let select = post::Entity::find().order_by_desc(post::Column::Id); // always order a paged list
    Ok(Json(ApiPage::fetch(select, app.db(), &page, PostJson::from).await?))
}
```

- `Pagination` reads `?page` (1–10,000) and `?per_page` (1–100, default 20);
  out of range, not a number or given twice → 422 on that parameter.
- `ApiPage<T>` answers `{data: […], meta: {page, perPage, total, lastPage}}`.

## Rate limiting

`Throttle<N>` allows N requests a minute per user and route, then 429 with
`Retry-After`. It runs before the body is read. With Redis configured, limits
are shared by every instance ([Several instances](../digging-deeper/instances.md)).
