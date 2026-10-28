# Admin panel

The admin panel is a single-page application that lists, shows and edits the
records of your [resources](resources.md), like a Filament panel. It is
embedded in your binary: running it never requires Node. Use it for your
team's back office; build your public site or API next to it.

![The demo's panel: sidebar groups, the dashboard and its resource cards](/img/admin/overview-shell.webp)
*The demo application: the panel's shell with its Content and Settings groups.*

## Quick start

A new application (`rustcms new`) already has a panel. Its provider lives in
`src/providers/admin_panel_provider.rs`:

```rust
use rustcms::prelude::*;

pub fn provider() -> AdminProvider {
    AdminProvider::new()
        .brand("Blog")
        // Labels in Vietnamese (`rustcms lang:missing vi` lists what is missing).
        .translations("vi", include_str!("../../lang/vi.toml"))
        // rustcms:resources
        // rustcms:pages
        // rustcms:extensions
}
```

`rustcms make:resource Post --generate` adds `.resource::<PostResource>()` at
the `// rustcms:resources` marker. Providers are registered in
`src/bootstrap/providers.rs`, the authentication provider first (the panel
uses its session guard) and the storage provider before the panel when you use
the media library:

<<< @/../../../examples/demo/src/bootstrap/providers.rs#register [src/bootstrap/providers.rs]

Run `rustcms permission:sync` after adding a resource, then sign in at
`/admin` with a user whose role has its permissions.

## Full example

The demo's panel: resources, settings groups, a dashboard, custom pages, a
slot, extensions and translations.

<<< @/../../../examples/demo/src/providers/admin_panel_provider.rs#provider [src/providers/admin_panel_provider.rs]

## `AdminProvider`

| Method | Does |
|---|---|
| `new()` | an empty panel |
| `brand(name)` | the name the panel shows (default `"rustcms"`) |
| `resource::<R>()` | adds a [resource](resources.md) |
| `media()` | the [media library](media.md) (register `StorageProvider` first) |
| `settings::<T>()`, `mail_settings()` | [settings](settings.md) groups, and the [mail settings](settings.md#mail) |
| `dashboard(Dashboard)` | [dashboard widgets](dashboard.md) |
| `page::<P>()`, `home(slug)` | [custom pages](customizing.md#custom-pages); the [home page](#home-page) |
| `slot::<S>()` | a [slot beside records](customizing.md#slots-beside-records) |
| `theme(Theme)`, `branding(source)` | the [theme](customizing.md#theme), [runtime branding](customizing.md#runtime-branding) |
| `extension_script(name, js)`, `extension_style(name, css)` | [extension](custom-fields.md) scripts and styles |
| `extension_script_v2(name, js, requires)` | a script built with the SDK ([React pages](react-pages.md)) |
| `extension_timeout(duration)` | how long the panel waits for each extension script: 1 to 60 seconds, default 10 |
| `translations(locale, toml)` | your labels in another language ([i18n](i18n.md)) |

The panel itself is written in English (`en`) and Vietnamese (`vi`); a locale
in `app.locales` outside these stops the boot.

## Paths

The panel is served under `auth.admin_path` (default `/admin`):

| Path | Serves |
|---|---|
| `/admin` | the dashboard, or the [home page](#home-page) |
| `/admin/login`, `/admin/forgot-password`, `/admin/reset-password`, `/admin/invitation` | signing in, password reset, accepting an invitation |
| `/admin/r/<slug>` | a resource's list; `…/create`, `…/records/<key>`, `…/records/<key>/edit`; `…/import` for [importable](import-export.md) resources |
| `/admin/p/<slug>`, `/admin/p/<slug>/…` | a [custom page](customizing.md#custom-pages) and its sub-paths |
| `/admin/settings/<group>` | a [settings](settings.md) group |
| `/admin/system/users`, `…/users/create`, `…/users/<id>` | [users](users-roles.md) |
| `/admin/system/roles`, `…/roles/create`, `…/roles/<id>` | [roles](users-roles.md) |
| `/admin/system/audit`, `/admin/system/queue`, `/admin/system/schedules` | the [audit log](../digging-deeper/audit-log.md#viewing-the-log), the [queue](../digging-deeper/queues.md#the-queue-page), the [schedules](../digging-deeper/scheduling.md#the-schedules-page) |
| `/admin/account` | the user's own account |
| `/admin/assets/…` | the panel's JS and CSS |
| `/admin/assets/ext/…` | your [extension](custom-fields.md) scripts and styles |
| `/admin/assets/brand/{theme.css,logo,favicon}?v=…` | the [theme](customizing.md#theme)'s stylesheet and images; also `logo-dark` with [runtime branding](customizing.md#runtime-branding) |
| `/admin/api/…` | the panel's JSON API ([Admin API](../reference/admin-api.md)); `/admin/api/panel` describes the panel for the signed-in user |

System pages and the Settings group appear as the user's permissions allow.
Every user has their own Account page.

## Navigation

Resources the user may list (`view_any`) appear in the sidebar, grouped by
`ResourceMeta::group`, with `ResourceMeta::icon`:

```rust
ResourceMeta::new("Post").plural("Posts").group("Content").icon("file-text")
```

Icons (also for pages): `activity`, `bell`, `box`, `calendar`, `chart`,
`cloud`, `container`, `cpu`, `dashboard`, `database`, `file-code`,
`file-text`, `folder`, `gauge`, `git-branch`, `globe`, `hard-drive`,
`history`, `home`, `image`, `key`, `layers`, `link`, `lock`, `logs`, `mail`,
`network`, `package`, `rocket`, `server`, `settings`, `shield`,
`shopping-cart`, `tag`, `terminal`, `users`, `workflow`. Any other name shows
a generic icon (the LMS example's `book-open` and `clipboard-check` do).

### Order

By default, resources come in registration order (`.media()` where it is
called), then [custom pages](customizing.md#custom-pages) in registration
order, resources first in a group. The dashboard's resource cards follow
the same order. `.sort(n)` on
`ResourceMeta` or `PanelPageMeta` (default 0) changes it:

- entries are ordered by `sort`, then by that default order;
- groups are ordered by the smallest `sort` among their entries, then by
  first appearance;
- System and Settings always come last.

```rust
PanelPageMeta::new("Templates").permission("templates.view").sort(1)
ResourceMeta::new("Project").group("Docker").sort(10)
```

### Hiding a resource

`ResourceMeta::navigation(false)` removes the sidebar entry and the
dashboard card. The resource keeps its routes, record pages, API, relations
and breadcrumbs, for example when your own [React pages](react-pages.md)
replace its list. This is only presentation: listing still needs `view_any`,
and record pages and writes keep their own ability checks (`view`, `update`, …).

### Home page

`AdminProvider::home("dashboard")` makes `/admin` open the custom page
`dashboard` instead of the default dashboard, with that page as the first
sidebar entry. Users who may not open it see the default dashboard. An
unknown slug stops the boot.

## What users get

- **Lists** ([tables](tables.md)): search, sortable columns, filters and
  pagination, kept in the URL; row and bulk [actions](actions.md); a trash
  for resources with [soft deletes](resources.md#soft-deletes).
- **Forms** ([forms](forms.md)): built for the operation and the user. Only
  fields the user may write are editable, edits send changed fields only, and
  server validation messages appear under each input. A write whose outcome
  is unknown (a timeout, a cut connection) pauses further writes on that
  resource, and creates and actions can be retried safely.
- **Record pages** ([resources](resources.md#the-record-page)) with their
  [actions](actions.md#placement-in-lists), [relations](relationships.md),
  [slots](customizing.md#slots-beside-records) and history.
- **Languages**: English and Vietnamese, chosen per user ([i18n](i18n.md)).
- **Roles** (System → Roles): permissions by area with a search box, the
  labels and presets the application declares
  ([The Roles page](../security/authorization.md#the-roles-page-areas-labels-and-presets)).
- Someone who may update a record but not view it gets an empty edit form;
  records outside a user's scope show "Not found", forbidden pages "Access
  denied".

The color scheme is the app's `ColorScheme` (default `System`: follows the
operating system) until a user chooses Light, Dark or System for themselves
([theme](customizing.md#theme)). The layout works from phones to wide screens
and from the keyboard.

![The demo's posts list on a phone, with the sidebar folded](/img/admin/overview-phone.webp)
*The demo application at phone width.*

## Commands

The panel adds these commands to your application:

| Command | Does |
|---|---|
| `idempotency:prune` | deletes expired [idempotency receipts](../reference/admin-api.md#retrying-writes-safely) (`serve` does it hourly) |
| `media:prune` | deletes queued media files and abandoned uploads (only with `.media()`) |
| `lang:missing <locale>` | lists the labels missing in a [translation](i18n.md) |
| `admin:branding` | shows where the panel's branding comes from and loads it once (changes nothing) |

## Security

- Every request is authorized on the server: see
  [Authorization](authorization.md) for the rules.
- Content Security Policy: `default-src 'self'; script-src 'self';
  style-src 'self'; img-src 'self' data:; font-src 'self'; connect-src 'self';
  object-src 'none'; base-uri 'self'; form-action 'self'; frame-ancestors
  'none'`. `connect-src` also names the `ws://` or `wss://` form of the
  panel's origins, for [sockets](realtime.md). Nothing else is ever added:
  extensions must follow it (no inline scripts or styles).
- `X-Frame-Options: DENY`, `X-Content-Type-Options: nosniff` and
  `Referrer-Policy: same-origin`; the reset-password and invitation pages
  send `Referrer-Policy: no-referrer`, since their URL carries a token.
- The session cookie is `HttpOnly`, `SameSite=Lax`, scoped to the admin path;
  writes need the CSRF token and an allowed origin
  ([Authentication](../security/authentication.md)).
- Signing out (or a 401, or signing out in another tab) clears every cached
  record, schema and form. Panel API responses are `private, no-store`.
- After sign-in the user returns to the page they asked for, never outside the
  panel.

## Pitfalls

- Register `AuthProvider` before `AdminProvider`, and `StorageProvider`
  before a panel with `.media()`: otherwise the boot stops with
  *register AuthProvider before AdminProvider* or *register StorageProvider
  before AdminProvider::media()*.
- Resources are checked when the application starts. A misconfiguration (an
  unknown column, an input for a field that does not exist, a filter on a
  field that is not filterable) stops the boot with a message naming the
  resource and the fix.

## Related pages

- [Resources](resources.md), [Dashboard](dashboard.md), [Authorization](authorization.md)
- [Customizing the panel](customizing.md)
- [Admin API](../reference/admin-api.md)
