# Request lifecycle

## Starting

`main.rs` hands your `build` function to the console. For `serve`:

1. configuration is loaded (`config/*.toml`, `.env`, environment);
2. `build(config)` assembles the `AppBuilder`: every provider's `register`
   runs, binding services, routes and commands;
3. the database is connected (and, for MySQL, its settings checked);
4. every provider's `boot` runs (background jobs start here);
5. the HTTP server listens on `app.listen`.

Your own commands, and most framework commands, follow the same steps and run
instead of serving. A few do less: `key:generate` runs before the
configuration is complete, the `migrate` commands only connect to the
database, and `route:list` assembles the application without connecting or
booting it.

## The HTTP stack

Every request goes through, outermost first:

1. **Request id** — `x-request-id` is generated (UUID) or kept from the
   request, and returned.
2. **Tracing** — one span per request with method, path and request id.
3. **Client IP** — the `ClientIp` extractor; see [proxies](#client-ip-and-proxies).
4. **Timeout** — `http.timeout_secs`, then 408.
5. **Body limit** — `http.body_limit`, then 413, on every request body (a
   larger `Content-Length` is refused before the handler runs; chunked bodies
   stop at the limit). Routes may raise it (`app.body_limit(method, path, bytes)`).
6. **Guard** — chosen by path: the admin session guard under
   `auth.admin_path`, the API guard elsewhere ([Authentication](../security/authentication.md)).
7. **Route access** — for API routes, the `Access` you declared, before any
   extractor reads the request.
8. **Your handler** — extractors, then your code.

Unknown routes answer 404 and wrong methods 405, both as JSON.
`GET /health` returns `{"status":"ok","database":"ok"}`, or 503 when the
database is down.

## Responses and errors

Handlers return `AppResult<T>`. Errors become JSON in one format
([Errors](../basics/errors.md)); internal details are logged, never returned.

## Client IP and proxies

`X-Forwarded-For` is ignored unless the TCP peer is in `http.trusted_proxies`.
Then the client is the right-most address that is not a trusted proxy, so a
client cannot choose its address by sending the header itself.

```toml
# config/http.toml
trusted_proxies = ["10.0.0.0/8"]
```
