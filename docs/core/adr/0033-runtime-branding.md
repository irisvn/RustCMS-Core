# ADR 0033: Runtime branding

- Status: accepted (0.7.0, P9)
- Date: 2026-10-20

## Context

The static `Theme` (ADR 0019) compiles colors, radius, fonts, a logo and a
favicon into the binary, and `index.html` was rendered once at boot. Apps
such as RDockPanel let an operator change the brand while the panel runs
(Settings → Branding), which the static theme cannot express. The app owns the
upload page, storage and permission; the panel must show the result safely.

## Decision

- **Source.** `AdminProvider::branding(impl BrandingSource)`; `load(&AppState)
  -> AppResult<Option<Branding>>`. `Branding` is a checked value: name
  (1–64 characters, not blank, no control characters), logo, dark logo and favicon
  (PNG, WebP or ICO, magic bytes checked, ≤ 512 KiB), the theme's color
  tokens (hex only) and radius. Each setter returns `Result`, and
  `check_image`/`check_color`/`check_name` let the app validate uploads with
  the same rules. A runtime field overrides the static one of the same name.
- **Tokens only.** No fonts (a family needs font files, which would mean
  serving uploads as fonts), no SVG (scriptable), no CSS of the app's. A later
  CSS hook would need a parsed allowlist of properties and selectors scoped to
  brand classes; no `@import`, `url()`, `expression`, `position: fixed`, nor
  `display`/`visibility`/`opacity` on authentication and security UI; served
  as a stylesheet from `'self'` under the unchanged CSP.
- **Cache.** `AdminBranding` (a service) keeps one entry per instance for
  `[admin] branding_ttl_secs` (default 30). Loads are single-flight; an
  entry is stored only if the generation did not move while loading (check
  and store under one lock), and a discarded load is retried once.
  `invalidate()` bumps the generation and clears the entry. Instances
  converge within the TTL: no pub/sub (simplicity; a stale brand for 30 s is
  acceptable). A failing or slow load (`branding_timeout_ms`) shows the static
  theme for one period; the error is logged without its data.
- **Serving by kind.** The theme's files (static or runtime) are served at
  `{admin}/assets/brand/{theme.css|logo|logo-dark|favicon}?v={hash}`. The
  handler always returns the current file of that kind: HTML from another
  instance, from before a restart or from before two quick changes gets an
  image while that kind exists (a missing kind is a 404). Cached for good only when `v` matches, else `no-cache`. Exact MIME,
  `nosniff`, `Cross-Origin-Resource-Policy: same-origin`, `inline`, the
  panel's CSP. The static theme stylesheet is no longer an extension asset;
  the brand stylesheet is the first of `extensions.styles`, so app styles
  still refine it.
- **One branding per document.** With a source in runtime mode, `index.html`
  is rendered per request (still `no-store`; in static mode once, even with a
  source registered); the config carries `brand` and the asset paths.
  The SPA takes the brand, logos and favicon from it until a reload, never
  from `PanelResponse.brand` (kept for compatibility), so two requests cannot
  mix two brandings. The sign-in pages show the brand: it is public.
- **Recovery.** `[admin] branding = "static"` (`RUSTCMS_ADMIN__BRANDING`)
  ignores the source at boot. `admin:branding` reports the mode and, in
  runtime mode only, loads once; it changes nothing (clearing stored data is
  the app's job).

## Consequences

- Without a source, the panel behaves as before, except the theme's paths
  (`assets/brand/`) and the brand on the sign-in pages.
- Contrast and taste are not checked: a malformed value is refused, an ugly
  one is not; the recovery switch covers an unusable result.
- A brand change takes up to one TTL to reach other instances.
