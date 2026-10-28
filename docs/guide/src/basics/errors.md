# Errors

Handlers return `AppResult<T>` (`Result<T, AppError>`). An `AppError` becomes a
JSON response with a status, a human `message` and a stable `code`, so clients
can branch on the code and show the message. Use it for every expected failure:
not found, not allowed, invalid input, a write that must be retried.

![A form with an error under a field and an error above the form](/img/basics/form-errors.webp)
*Local demo: the admin panel shows `errors` under each field and `formErrors` in an alert at the top of the form.*

## Quick start

```rust
pub async fn show(State(app): State<AppState>, Path(id): Path<i64>) -> AppResult<Json<PostJson>> {
    let post = post::Entity::find_by_id(id)
        .one(app.db())
        .await?                       // a database error: 500, or 409 when it can be retried
        .ok_or(AppError::NotFound)?;  // no row: 404
    Ok(Json(post.into()))
}
```

```json
{ "message": "Not found.", "code": "not_found" }
```

## The response

Every `AppError` answers JSON with `message` and `code`. The other keys appear
only for one status:

| Key | Present on | Holds |
|---|---|---|
| `message` | every error | a human message (see the table below) |
| `code` | every error | the stable code |
| `reason` | 409 | which conflict (`busy`, …) |
| `errors`, `formErrors` | 422 | messages per field, messages for the whole form |
| `issues`, `formIssues` | 422 | the stable code and parameters of each message, in the same order |

A 422:

```json
{
  "message": "The given data was invalid.",
  "code": "validation_failed",
  "errors": { "title": ["is required"] },
  "formErrors": [],
  "issues": { "title": [{ "code": "required", "params": {} }] },
  "formIssues": []
}
```

A 409:

```json
{
  "message": "Another change to the same data is in progress. Nothing was saved; try again.",
  "code": "write_conflict",
  "reason": "busy"
}
```

## Variants

| `AppError` | Status | `code` | `message` |
|---|---|---|---|
| `BadRequest(detail)` | 400 | `bad_request` | `detail`, **shown to the client** |
| `Unauthenticated` | 401 | `unauthenticated` | `Unauthenticated.` |
| `Forbidden` | 403 | `forbidden` | `This action is unauthorized.` |
| `CsrfMismatch` | 403 | `csrf_mismatch` | `CSRF token mismatch.` |
| `NotFound` | 404 | `not_found` (also for records outside the user's scope) | `Not found.` |
| `MethodNotAllowed` | 405 | `method_not_allowed` | `Method not allowed.` |
| `Conflict { code, reason, message }` | 409 | `code` (e.g. `write_conflict`) | `message` |
| `PayloadTooLarge` | 413 | `payload_too_large` | `Payload too large.` |
| `Validation(errors)` | 422 | `validation_failed` | `The given data was invalid.` |
| `TooManyRequests { retry_after }` | 429 | `too_many_requests` (+ `Retry-After` header, seconds) | `Too many attempts. Please try again later.` |
| `Internal(detail)` | 500 | `internal_error` | `Server error.`; the detail is logged, never returned |

Helpers:

| Method | Does |
|---|---|
| `AppError::internal(err)` | wraps anything that implements `Display` as `Internal` (logged, 500) |
| `error.status()` | the HTTP status |
| `error.code()` | the stable code |
| `ValidationErrors` → `AppError` | `From`, so `Err(errors.into())` or `errors.into_result()?` |

All top-level codes, including those the framework uses for its own 409s, are
listed in [Error codes](../reference/error-codes.md#top-level-codes).

### Your own conflicts

`Conflict` takes three `&'static str`: a `code` and `reason` clients can
branch on, and a `message` shown as written (it is not translated). Use it
when a request is valid but cannot apply now:

```rust
return Err(AppError::Conflict {
    code: "order_shipped",
    reason: "shipped",
    message: "This order has shipped and can no longer be changed.",
});
```

## Database errors

`?` converts a `sea_orm::DbErr`:

- A write rolled back because of other writers becomes a 409 `write_conflict`
  with `reason: "busy"`: the statement that failed was not applied. This
  covers lock wait timeouts, deadlocks, serialization failures and a busy
  SQLite database (`database is locked`). On PostgreSQL it is decided by
  SQLSTATE `55P03`, `40P01` and `40001`; on MySQL by error numbers `1205`,
  `1213` and `3572`; otherwise by the message.

  The conversion only classifies the error: it does not roll back what the
  handler wrote before the failing statement. In custom handlers, use a
  transaction or idempotency to make retrying the whole request safe.
- Anything else becomes a 500 `internal_error`; the database message is
  logged, never returned.

A missing row is not an error for SeaORM (`one()` returns `None`): turn it into
`AppError::NotFound` yourself, as in the quick start.

## Validation errors

```rust
let mut errors = ValidationErrors::new();
errors.add("email", "must be a valid email address");
errors.add_form("This was changed by someone else meanwhile. Reload and try again.");
errors.into_result()?;                                  // Ok(()) when empty

return Err(ValidationErrors::field("slug", "has already been taken").into());
```

`errors` holds messages per field, `formErrors` messages for the whole form.
`issues` and `formIssues` carry, in the same order, the stable code and
parameters of each message, for clients that translate them; see
[Error codes](../reference/error-codes.md). A message that follows no template
gets the code `custom` with `{"message": …}`. In Rust,
`errors.issues()` and `errors.form_issues()` return them as
`rustcms::i18n::Issue` values (`code`, `params`); `Issue::of(message)` gives
the issue of one message.

## Framework-generated responses {#responses-that-are-not-apperror}

| Answer | Body |
|---|---|
| unknown path | 404 `not_found`, JSON as above |
| known path, other method | 405 `method_not_allowed`, JSON |
| request longer than `http.timeout_secs` (default 30) | 408 with an **empty** body |
| `GET /health` | its own body: 200 `{"status":"ok","database":"ok"}` or 503 `{"status":"degraded","database":"unavailable"}` |

Handlers can call `rustcms::http::request_deadline()` to know when the 408
will cut the request off, and finish optional work before it.

A panic in a handler (or in a guard or middleware) answers the standard 500
(`{"message": "Server error.", "code": "internal_error"}`) and is logged once,
with its message, in the request's span; the connection stays usable. A
stream that has already answered (server-sent events) cannot become a 500: the
panic ends that stream. Prefer returning `AppError::internal`: a panic says
nothing to the caller and can leave shared state half-updated.

## Security

- `BadRequest(detail)` and `Conflict.message` are shown to the client: never
  put secrets, SQL or internal paths in them. Use `AppError::internal` for
  details only the logs should see.
- `NotFound` is also the answer for records outside the caller's scope, so a
  client cannot tell a hidden record from a missing one. Keep that rule in your
  own handlers.

See [Authorization](../security/authorization.md).

## Pitfalls

- Handle a 409 `write_conflict` by its `reason` and the endpoint's retry
  contract. `busy` is meant to be retried. `receipt_expired` (the admin API's
  idempotent writes) means the earlier request with this key is no longer
  known and may have been committed: reload and check the earlier result
  before trying again. Do not retry a 500 blindly.
- `CsrfMismatch` concerns the admin session only: fetch a new token, do not
  replay the request as is.
- Clients should read `code` (and `reason`), not `message`: messages may
  change wording.

## Related pages

- [Validation & pagination](validation.md)
- [Error codes](../reference/error-codes.md)
- [Request lifecycle](../architecture/lifecycle.md)
- [Routing](routing.md)
