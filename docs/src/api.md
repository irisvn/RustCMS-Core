# API (tokens)

Everything outside the admin path uses the **API guard**: requests carry a
personal access token, `Authorization: Bearer rcms_<id>_<secret>`. Tokens are
stored hashed, can expire, and carry **abilities**: a request may do what both
the token's abilities and the user's API-guard permissions allow (`*` = all).

The user needs API-guard permissions first (roles of the `api` guard):

```sh
rustcms role:assign --email you@example.com --role super_admin --guard api   # or any api role
rustcms token:create --email you@example.com --name deploy --abilities posts.view_any --expires-days 30
```

Or over HTTP: `POST /api/v1/auth/token` with `{email, password, token_name,
abilities, expires_in_days}` (rate limited like sign-in); `DELETE
/api/v1/auth/token` revokes the token used; `GET /api/v1/auth/me`.

## Writing endpoints

```rust,ignore
async fn list(State(app): State<AppState>, auth: Auth) -> AppResult<Json<Value>> {
    auth.authorize_guard(API_GUARD, "posts.view_any")?;   // first, before reading input
                                                           // 403 unless permitted (role ∩ token abilities)
    // …
}

app.routes(|router| router.route("/api/v1/posts", get(list)));
app.describe_routes("app", [(Method::GET, "/api/v1/posts".into())]); // for `route:list`
```

Declare API permissions so `permission:sync` creates them:

```rust,ignore
AuthProvider::new().permission(API_GUARD, "posts.view_any", "posts")
```

A complete example is `examples/demo/src/api.rs` (`GET/POST /api/v1/posts`):
listing published posts with `posts.view_any`, creating drafts with
`posts.create`, strict input (`deny_unknown_fields`), and tests in
`examples/demo/tests/api.rs`.

Revocation (password change, sign-out everywhere, disabling) applies to the next
request; a write already running with the old credentials still commits (like
admin resource writes). See [Credential revocation](security/revocation.md).
