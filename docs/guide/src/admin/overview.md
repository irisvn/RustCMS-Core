# Admin panel

The admin panel is a single-page application that renders your
[resources](resources.md). It is embedded in your binary: using it never
requires Node.

```rust,ignore
app.provider(AuthProvider::new());                 // first: the panel uses its session guard
app.provider(rustcms::storage::StorageProvider);   // only for the media library
app.provider(
    AdminProvider::new()
        .brand("Blog")
        .media()                                   // the media library (optional)
        .resource::<CategoryResource>()
        .resource::<PostResource>()
        .translations("vi", include_str!("../lang/vi.toml")),
);
```

It is served under `auth.admin_path` (default `/admin`):

| Path | Serves |
|---|---|
| `/admin`, `/admin/…` | the panel (`/admin/login`, `/admin/r/<slug>`, `…/create`, `…/records/<key>`, `…/edit`) |
| `/admin/assets/…` | the panel's JS and CSS |
| `/admin/assets/ext/…` | your [extension](custom-fields.md) scripts and styles |
| `/admin/api/…` | the panel's JSON API (used by the panel itself) |

Each resource declares its permissions (`posts.view_any`, `posts.create`, …)
for `rustcms permission:sync`. Users see the panel's System pages
([users & roles](users-roles.md)) and their own Account page.

## Navigation

Resources the user may list (`view_any`) appear in the sidebar, grouped by
`ResourceMeta::group`, with `ResourceMeta::icon`:

```rust,ignore
ResourceMeta::new("Post").plural("Posts").group("Content").icon("file-text")
```

Icons: `box`, `file-text`, `folder`, `image`, `settings`, `shield`,
`shopping-cart`, `tag`, `users`.

## What users get

- **Lists**: search, sortable columns, filters and pagination, kept in the URL;
  row and bulk [actions](actions.md); a trash for soft-deleting resources.
- **Forms**: built for the operation and the user: only fields the user may
  write are editable, edits send changed fields only, server validation
  messages appear under each input. A write whose outcome is unknown (a
  timeout, a cut connection) pauses further writes on that resource, and
  creates and actions can be retried safely.
- **Record pages** with their [relations](relationships.md).
- **Languages**: English and Vietnamese, chosen per user ([i18n](i18n.md)).
- Someone who may update a record but not view it gets an empty edit form;
  records outside a user's scope show "Not found", forbidden pages "Access denied".

The theme follows the operating system (light or dark). The layout works from
phones to wide screens and from the keyboard.

## Security

- Content Security Policy `default-src 'self'`, `script-src 'self'`,
  `style-src 'self'`, `frame-ancestors 'none'`, plus `X-Frame-Options: DENY`,
  `nosniff` and `Referrer-Policy: same-origin`. Extensions must follow it (no
  inline scripts or styles).
- The session cookie is `HttpOnly`, `SameSite=Lax`, scoped to the admin path;
  writes need the CSRF token and an allowed origin
  ([Authentication](../security/authentication.md)).
- Signing out (or a 401, or signing out in another tab) clears every cached
  record, schema and form. Panel API responses are `private, no-store`.
- After sign-in the user returns to the page they asked for, never outside the
  panel.
