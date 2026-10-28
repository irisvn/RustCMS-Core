# Validation & pagination

Three extractors handle what an API route reads from a request: `Validated<T>`
parses and checks a JSON body, `Pagination` reads `?page` and `?per_page`, and
`Throttle<N>` limits how often a user calls the route. Each one answers in the
standard [error format](errors.md) when the request is wrong.

## Quick start

```rust
use rustcms::prelude::*;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewTag {
    pub name: String,
}

impl Validate for NewTag {
    fn validate(&mut self) -> Result<(), ValidationErrors> {
        self.name = self.name.trim().to_owned();
        let mut errors = ValidationErrors::new();
        if self.name.is_empty() {
            errors.add("name", "is required");
        }
        if errors.is_empty() { Ok(()) } else { Err(errors) }
    }
}

pub async fn store(Validated(input): Validated<NewTag>) -> AppResult<StatusCode> {
    // `input` is parsed and valid here.
    Ok(StatusCode::CREATED)
}
```

## Validating input

`Validated<T>` reads the JSON body like [`Json<T>`](routing.md#handlers), then
calls `T::validate`. `validate` takes `&mut self`, so it can normalize the
input (trim, drop empty values) before checking it. The demo's input for a new
post:

<<< @/../../../examples/demo/src/http/requests/post_request.rs#new-post [src/http/requests/post_request.rs]

<<< @/../../../examples/demo/src/http/requests/post_request.rs#validate [src/http/requests/post_request.rs]

The handler that uses it, with a rate limit counted before the body is read:

<<< @/../../../examples/demo/src/http/controllers/post_controller.rs#store [src/http/controllers/post_controller.rs]

Put `Validated<T>` (or `Json<T>`) last: it consumes the body, and the
extractors before it (`Auth`, `Throttle`, `Path`) run first.

What the client gets:

| Body | Answer |
|---|---|
| larger than the body limit | 413 `payload_too_large` |
| missing or unsupported JSON `Content-Type` (`application/json`, `application/*+json`), or malformed JSON | 400 `bad_request` |
| JSON, but not a `T` (wrong type, missing field, unknown field with `deny_unknown_fields`) | 422 with a form error (code `invalid_body`) |
| `validate` returns errors | 422 with field errors |

Use `#[serde(deny_unknown_fields)]` on every input: a client that sends a
field you do not read (`author_id`) gets a 422 instead of a silent success.

The body limit is `http.body_limit` (default 1 MiB, `1048576` bytes) for every
request; give one route a larger limit with
[`app.body_limit`](routing.md#plain-axum-routes).

### Messages

Write messages in plain English, without the field name (`"is required"`; the
client shows the field). A message that follows one of the framework's
templates gets a stable code clients can translate
([error codes](../reference/error-codes.md)): `"is required"` is `required`,
`"may not be longer than 200 characters"` is `max_length` with
`{"max": "200"}`. Any other text gets the code `custom` and is shown as
written.

`ValidationErrors` methods:

| Method | Does |
|---|---|
| `ValidationErrors::new()` | an empty bag |
| `ValidationErrors::field(field, message)` | a bag with one field message |
| `errors.add(field, message)` | adds a message for a field |
| `errors.add_form(message)` | adds a message for the whole form |
| `errors.is_empty()` | no message recorded |
| `errors.into_result()` | `Ok(())` when empty, else `Err(AppError::Validation(..))` |

See [Errors](errors.md#validation-errors) for the response format.

## Pagination

`Pagination` reads `?page` and `?per_page`; `ApiPage::fetch` runs the query
for that page and counts every row. The demo's list of published posts:

<<< @/../../../examples/demo/src/http/controllers/post_controller.rs#index [src/http/controllers/post_controller.rs]

The response:

```json
{
  "data": [{ "id": "42", "title": "Hello", "body": null, "published": true, "published_at": "2026-10-01T08:00:00Z" }],
  "meta": { "page": 1, "perPage": 20, "total": 15, "lastPage": 1 }
}
```

| Parameter | Range | Default | Constant |
|---|---|---|---|
| `page` | 1 to 10,000 | 1 | `rustcms::http::api::MAX_PAGE` |
| `per_page` | 1 to 100 | 20 | `MAX_PER_PAGE`, `DEFAULT_PER_PAGE` |

- Out of range, not a number, empty or given twice → 422 on that parameter
  (`"must be a number from 1 to 10000"`, `"may only be given once"`). Other
  query parameters are left to your handler.
- A page past the last one answers an empty `data` with the same `meta`.
- `lastPage` is at least 1, even for an empty list.
- Always order the query, and end with a unique column (the key): without a
  total order, rows can move between pages.

`Pagination` can also be built by hand: `Pagination::new(page, per_page)`
checks the bounds, `Pagination::from_query(Some("page=2"))` reads a raw query
string; both return `Result<Pagination, ValidationErrors>`. `page.page()` and
`page.per_page()` read the values. `ApiPage<T>` has two public fields, `data`
and `meta` (`rustcms::http::ApiPageMeta`: `page`, `per_page`, `total`,
`last_page`).

## Rate limiting

`Throttle<N>` allows N requests a minute per user and route, then answers 429
with `Retry-After` (seconds). Put it before the body extractor: attempts are
counted before the body is read and validated, so invalid requests count too.

- The counter key is the user and the route pattern (`/api/v1/posts`), not the
  method: if the `GET` and the `POST` handlers of one path both take
  `Throttle`, they share one counter. Handlers without `Throttle` do not count.
- It needs a token: without one, the request is 401, even on a public route.
  Use it on routes with `Access::authenticated()` or `Access::can(…)`.
- Limits are kept in memory, per instance. With Redis configured, every
  instance counts in Redis ([Several instances](../digging-deeper/instances.md));
  if Redis does not answer, the request fails (500) rather than going through
  unlimited.

## Pitfalls

- `validate` runs only after the body parsed as a `T`: a 422 for a wrong shape
  carries a form error, not field errors.
- Messages are sorted by field name in the response (`errors` is a map), and
  kept in the order you added them within a field.
- Field rules here apply to your API routes only. Admin forms validate with
  their own [field rules](../admin/forms.md), including the static options
  of selects and checkbox lists ([options](../admin/forms.md#options-are-not-validated)).

## Related pages

- [Routing](routing.md)
- [Errors](errors.md)
- [Error codes](../reference/error-codes.md)
- [Testing](testing.md): `assert_validation_error(field)` checks a 422 on a
  field.
