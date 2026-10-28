# Theming and branding

The panel's look is set in two layers. The **theme** (`AdminProvider::theme`)
is fixed in code: colors, corner radius, font, logo, favicon and the default
color scheme. **Runtime branding** (`AdminProvider::branding`) lets the
application change the brand name, logos and colors while it runs, for
example from a Settings → Branding page, without a deploy. Use the theme for
your product's look; add runtime branding when operators choose the brand.

![A themed panel in light and dark mode, with its logo](/img/admin/theme-light-dark.webp)
*A RustCMS LMS example: the panel with a blue theme and its own logo.*

## Quick start

```rust
// src/providers/admin_panel_provider.rs
use rustcms::admin::Theme;

AdminProvider::new()
    .brand("Acme")
    .theme(Theme::new().primary("#2563eb").primary_foreground("#ffffff"))
```

`brand(name)` names the panel: the sidebar, the sign-in page and the browser
tab.

## Theme

The LMS example's theme: a primary color for each scheme, a radius, a logo
and a favicon.

<<< @/../../../examples/lms/src/providers/admin_panel_provider.rs#theme [src/providers/admin_panel_provider.rs]

| `Theme` method | Effect |
|---|---|
| `Theme::new()` | the panel's defaults |
| `color(token, "#hex")` | sets a token of the light scheme (see below) |
| `primary(…)`, `primary_foreground(…)` | shortcuts for those two tokens |
| `dark(\|dark\| dark.primary(…))` | tokens of the dark scheme (`Palette::color`, `primary`, `primary_foreground`); tokens it leaves out keep the panel's dark defaults |
| `radius("0.625rem")` | corner radius, up to `4rem` or `64px` |
| `font_family("Inter, system-ui, sans-serif")` | installed fonts, best first, at most 200 characters |
| `logo(bytes, mime)`, `favicon(bytes, mime)` | PNG, WebP or ICO up to 512 KiB, checked against their bytes |
| `color_scheme(ColorScheme::…)` | the default scheme: `System` (default), `Light` or `Dark` |

| Token | Used for |
|---|---|
| `background`, `foreground` | page background and text |
| `card`, `muted`, `muted-foreground`, `border`, `ring` | panels, secondary text, borders, focus rings |
| `primary`, `primary-foreground` | buttons, active links, the default logo mark |
| `secondary`, `destructive`, `destructive-foreground`, `success`, `warning` | other buttons and states |

The list is `rustcms::admin::custom::TOKENS`.

- Colors are hex (`#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`).
- Font names are letters, digits, spaces and dashes. Each name is quoted in
  the stylesheet; the generic families `serif`, `sans-serif`, `monospace`,
  `system-ui`, `ui-sans-serif`, `ui-serif` and `ui-monospace` stay bare.
  Web fonts are not supported.
- Anything else stops the boot: no text of yours is copied into CSS as
  written. SVG logos are not supported.
- The static theme has a single logo. A separate logo for dark mode needs
  [runtime branding](#runtime-branding) (`Branding::logo_dark`).

### Light and dark

`ColorScheme::System` follows the operating system and its changes. Each user
can choose Light, Dark or System at the bottom of the sidebar. The choice is
kept in that browser (other tabs follow), and the application's
`ColorScheme` applies until they choose.

![The Light, Dark and System switch at the bottom of the sidebar](/img/admin/theme-scheme-switch.webp)
*A RustCMS LMS example: choosing the color scheme.*

### Where the theme is served

The theme becomes a stylesheet and images served from
`{admin}/assets/brand/theme.css`, `…/logo`, `…/logo-dark` (runtime branding
only) and `…/favicon`, each with `?v={hash}` and cached for good while `v` is
current. The stylesheet loads before your extension styles. The brand name
and logo also appear above the sign-in, password reset and invitation forms.

![The sign-in page with the brand's name and logo](/img/admin/theme-sign-in.webp)
*A RustCMS LMS example: the sign-in page.*

## CSS variables

Extension stylesheets ([custom fields](custom-fields.md),
[script pages](customizing.md#script-pages), [React pages](react-pages.md#styles))
use the theme through CSS variables, so they follow light and dark mode:

| Variable | |
|---|---|
| `--background`, `--foreground`, `--card`, `--muted`, `--muted-foreground`, `--border`, `--ring` | the tokens above |
| `--primary`, `--primary-foreground`, `--secondary`, `--destructive`, `--destructive-foreground`, `--success`, `--warning` | the tokens above |
| `--radius` | the theme's radius; `--radius-sm`, `--radius-md`, `--radius-lg` (= `--radius`) and `--radius-xl` derive from it |
| `--font-sans` | the theme's font stack |

```css
.acme-card { border: 1px solid var(--border); border-radius: var(--radius-lg); color: var(--muted-foreground); }
```

The panel sets `data-scheme="dark"` on the root element in dark mode; React
pages read it with `useColorScheme()`.

## Runtime branding

A `BrandingSource` returns the brand while the application runs. What it
returns goes over `brand(…)` and the `Theme`, field by field.

The demo shows the site's name from its settings once it has been changed:

<<< @/../../../examples/demo/src/admin/branding.rs#site-branding [src/admin/branding.rs]

<<< @/../../../examples/demo/src/providers/admin_panel_provider.rs#branding [src/providers/admin_panel_provider.rs]

An application with its own branding page calls `invalidate` after saving,
so this instance shows the change at once:

```rust
app.service::<rustcms::admin::AdminBranding>().expect("AdminProvider").invalidate();
```

![A Settings → Branding page and the panel using the chosen brand](/img/admin/runtime-branding.webp)
*RDockPanel, built on RustCMS: an operator's branding page.*

### What a `Branding` sets

| `Branding` method | Rules |
|---|---|
| `name(…)` | 1–64 characters, not blank, no control characters; plain text |
| `logo(bytes, mime)`, `logo_dark(bytes, mime)`, `favicon(bytes, mime)` | PNG, WebP or ICO up to 512 KiB, checked against their bytes; never SVG. `logo_dark` is shown when the page is dark (else `logo`) |
| `color(token, value)`, `primary(value)` | the theme's tokens and color rules |
| `dark(\|p\| p.try_color(token, value))` | tokens of the dark scheme |
| `radius(value)` | as `Theme::radius` |

- Fonts and custom CSS are not available at runtime.
- Each setter returns `Result<_, BrandingError>`. Validate an operator's
  input with the same rules first: `Branding::check_name`,
  `Branding::check_color` and `Branding::check_image`.
- Contrast is not checked.

### Cache and failures

- **Cache**: each instance keeps what `load` returned for
  `[admin] branding_ttl_secs` (default 30, 1–3600), one load at a time.
  `AdminBranding::invalidate()` makes the next request on this instance load
  again; other instances follow within that period (there is no push between
  instances). A load that overlaps an invalidation is not kept.
- **Failures**: `None`, an error, or a load slower than
  `[admin] branding_timeout_ms` (default 2000, 1–30000) shows the static brand
  and theme until the next period; the error is logged without its data.
- **One branding per page**: the panel reads the brand, logos and favicon
  from the page it loaded with, until a reload. Images and the stylesheet are
  served by kind: a page from another instance or from before a change gets
  the current file (with `no-cache`; cached for good only when `v` matches)
  while that kind exists, else a 404.

### Recovery

A broken source must not lock operators out of the panel's look:

- `[admin] branding = "static"` (or `RUSTCMS_ADMIN__BRANDING=static`) ignores
  the source; a restart applies it. The default is `"runtime"`.
- `rustcms admin:branding` shows the mode and, in runtime mode, loads the
  branding once and reports whether it is valid. It changes nothing.
- `AdminBranding::is_runtime()` (the source is used), `has_source()` (one is
  registered, even if `static` ignores it) and `probe(app)` (what
  `admin:branding` runs) let your own pages show the same state.

::: code-group

```toml [config/admin.toml]
branding = "runtime"      # or "static"
branding_ttl_secs = 30
branding_timeout_ms = 2000
```

```sh [.env]
RUSTCMS_ADMIN__BRANDING=static
```

:::

## Security notes

- The brand name, logos and colors are public: the sign-in page shows them
  to anyone.
- Every value reaching CSS is checked (hex colors, lengths, font names); no
  text of yours is copied into the stylesheet as written, and the panel's
  Content Security Policy stays as strict as without a theme.
- Images are checked against their bytes; SVG is never served.

## Related pages

- [Customizing the panel](customizing.md): pages, slots and cells
- [Configuration](../getting-started/configuration.md): the `[admin]` section
- [Several instances](../digging-deeper/instances.md): the branding cache per instance
- [Production checklist](../security/production.md)
