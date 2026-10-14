# Errors

Handlers return `AppResult<T>` (`Result<T, AppError>`). Every error answers
JSON in one format:

```json
{ "message": "The given data was invalid.", "code": "validation_failed",
  "errors": { "title": ["is required"] }, "formErrors": [],
  "issues": { "title": [{ "code": "required", "params": {} }] }, "formIssues": [] }
```

| `AppError` | Status | `code` |
|---|---|---|
| `BadRequest(detail)` | 400 | `bad_request` |
| `Unauthenticated` | 401 | `unauthenticated` |
| `Forbidden` | 403 | `forbidden` |
| `CsrfMismatch` | 403 | `csrf_mismatch` |
| `NotFound` | 404 | `not_found` (also for records outside the user's scope) |
| `MethodNotAllowed` | 405 | `method_not_allowed` |
| `Conflict { code, reason, message }` | 409 | e.g. `write_conflict` (nothing was applied; retry) |
| `PayloadTooLarge` | 413 | `payload_too_large` |
| `Validation(errors)` | 422 | `validation_failed` |
| `TooManyRequests { retry_after }` | 429 | `too_many_requests` (+ `Retry-After`) |
| `Internal(detail)` | 500 | `internal_error` — the detail is logged, never returned |

`?` converts database errors: a lock timeout or deadlock becomes a 409
`write_conflict` (nothing was saved), anything else a 500.

## Validation errors

```rust,ignore
let mut errors = ValidationErrors::new();
errors.add("email", "must be a valid email address");
errors.add_form("This was changed by someone else meanwhile. Reload and try again.");
errors.into_result()?;                                  // Ok(()) when empty

return Err(ValidationErrors::field("slug", "has already been taken").into());
```

`errors` holds messages per field, `formErrors` messages for the whole form.
`issues` and `formIssues` carry, in the same order, the stable code and
parameters of each message, for clients that translate them; see
[Error codes](../reference/error-codes.md). A 409 carries a `reason`.
