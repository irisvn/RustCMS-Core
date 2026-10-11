# HTTP

Every app gets this stack, outermost first:

1. **Request id** — `x-request-id` is generated (UUID) or kept from the request, and returned.
2. **Tracing** — one span per request with method, path and request id.
3. **Client IP** — `ClientIp` extractor; see below.
4. **Timeout** — `http.timeout_secs`, then 408.
5. **Body limit** — `http.body_limit`, then 413. Enforced on every request body,
   not only in extractors: a larger `Content-Length` is rejected before the handler
   runs, and chunked bodies stop being readable at the limit.

Unknown routes return 404 and wrong methods 405, both as JSON.
`GET /health` returns `{"status":"ok","database":"ok"}` or 503 when the database is down.

## JSON

Use `rustcms::Json` rather than `axum::Json`: rejections become rustcms errors
(413 too large, 400 malformed, 422 wrong shape) in the standard
[error format](../admin-protocol.md#errors).

## Errors

Return `AppResult<T>` from handlers. `AppError::Internal` details are logged and
replaced by `"Server error."` in the response.

## Client IP and proxies

`X-Forwarded-For` is ignored unless the TCP peer is in `http.trusted_proxies`.
Then the client is the right-most address that is not a trusted proxy, so a
client cannot spoof its IP by sending the header itself.

```toml
# config/http.toml
trusted_proxies = ["10.0.0.0/8"]
```
