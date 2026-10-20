# P9 plan: runtime white-label branding

Round 2, 2026-10-20 (Codex round 1 applied). Requested by RDockPanel. Approved and
shipped in 0.7.0; the text below is the design (ADR 0033).
Target: 0.8.0, or 0.7.0 together with P7. It adds a public trait, a builder,
config and routes.

## Status (implemented)
- The static theme is served the same way (`assets/brand/`), with or without
  a source, so there is one path; the generated theme stylesheet and images
  are no longer extension assets. Without a source, the index is still
  rendered once at boot.
- The service is reached with `state.service::<AdminBranding>()`.
- `Branding::check_name` joins `check_image` and `check_color`.
- No previous snapshot is kept: serving by kind made it unnecessary.
- The demo brands the panel with the site's name (Settings → Site) and a
  primary color, with `branding_ttl_secs = 2`.

## Why
RDockPanel lets an operator change the panel's brand in Settings → Branding.
The app side stays in RDockPanel: uploads, validation UI, storage and `system.manage`.

Core today knows only the static `Theme` (`features/custom/theme.rs`):
- colors, radius and font, compiled into a stylesheet at boot;
- logo and favicon as `&'static [u8]`, served as content-hashed extension
  assets;
- the brand string `AdminProvider::brand`.

`index.html` is rendered once at boot (`panel/spa.rs` `render_index`). The
sign-in page shows neither the brand nor the logo.

## Design

### 1. API
```rust
#[async_trait]
pub trait BrandingSource: Send + Sync + 'static {
    /// The current branding, or `None` for the static theme. Called at most
    /// once per cache period per instance.
    async fn load(&self, app: &AppState) -> AppResult<Option<Branding>>;
}

AdminProvider::new().brand("RDockPanel").theme(static_theme).branding(SettingsBranding)
```

**`Branding`** is an owned value. Each method returns
`Result<Self, BrandingError>`, so the app validates uploads with the same
rules core applies:
- `name(String)`: 1–64 characters, no control characters, plain text.
- `logo(Bytes, mime)`, `logo_dark(Bytes, mime)`, `favicon(Bytes, mime)`:
  - PNG, WebP or ICO only, with the magic bytes checked against the MIME;
  - at most 512 KiB each.
  - This is the existing `theme::image` check, made fallible and taking owned
    bytes. SVG is refused.
- `color(token, hex)`, `primary(hex)`, `dark(|palette| …)`:
  - the existing `TOKENS` allowlist and `hex_color` check;
  - `Palette` gains fallible `try_color` beside the panicking `color`.
- `radius`: the existing `length` check.
- Fonts are left out: a font family has no visible effect without a font file,
  and font files would mean serving uploads as fonts.

Public helpers for the app's own form validation:
`Branding::check_image(&[u8], &str)` and `Branding::check_color(&str, &str)`.

**Merge rule:** a runtime field overrides the static `Theme` or `brand` field
of the same name. A missing field falls back to the static one.
`logo_dark` falls back to the runtime `logo`, then the static logo.

### 2. Cache and invalidation
- **`BrandingCache`**, bound in the container and reachable as
  `app.get::<AdminBranding>()`, holds one entry: a `generation`, an
  `Arc<Resolved>` and an expiry time.
  - `Resolved` holds the merged values, the rendered stylesheet, and the images
    with their hashes.
- **When a load runs:** on the first request after expiry, set by
  `[admin] branding_ttl_secs` (default 30, 1–3600).
  - Single flight: one `tokio::sync::Mutex` guards the loader, so concurrent
    requests wait for the same load.
- **Publication:** one `std::sync::Mutex` guards the entry and the generation.
  - The loader records the generation when it starts.
  - The check-and-store runs under that lock: the result is stored only when
    the generation has not changed.
  - A discarded load is retried once. The retry's result is used for the
    current request either way, without being stored when the generation moved
    again.
  - This follows `PermissionResolver`, whose entries are keyed by generation.
  - Because assets are addressed by kind (section 3), an unstored snapshot
    never yields broken URLs.
- **`invalidate()`:** under the entry lock, bumps the generation and clears the
  entry, so this instance uses the new branding on its next request.
  RDockPanel calls it after saving Branding.
  - Other instances pick it up within the TTL. This is documented: there is no
    pub/sub in this version.
- **Failure:**
  - When `load` returns `Err` or a timeout passes (`[admin] branding_timeout_ms`,
    default 2000), the static theme is used and cached for the TTL, so a broken
    source is not called on every request.
  - The error is logged at `warn` with no data.
- **Memory:** at most about 1.5 MiB (three images). No previous snapshots are kept.

### 3. Serving
- **Images and stylesheet by kind:** `{admin}/assets/brand/{kind}?v={hash8}`,
  where `kind` is `logo`, `logo-dark`, `favicon` or `theme.css`.
  - The handler serves the **current** snapshot's file for that kind, so no
    instance has to know another's hashes. This covers load balancing,
    restarts and rapid updates.
  - When `v` equals the current hash, the response is
    `Cache-Control: public, max-age=31536000, immutable`. Otherwise it is
    served with `no-cache`, so a stale `v` never pins new bytes for long.
  - A kind with no runtime or static file returns a 404.
  - Every response carries:
    - the exact MIME of the stored kind, never sniffed, with `nosniff`;
    - `Cross-Origin-Resource-Policy: same-origin`;
    - `Content-Disposition: inline`;
    - the panel's CSP.
  - GET and HEAD only; other methods return a 405.
  - The route is public, as the static theme images are today, since the
    sign-in page needs it.
  - `assets/brand/` is dispatched in `spa.rs` `page()` before the generic
    `assets/` branch. `assets/ext/` keeps working unchanged.
- **Stylesheet:** `theme.css` is the merged token stylesheet, built by the
  existing `Theme::stylesheet` code path. It is served as `text/css` from
  `'self'`, so the CSP is unchanged and no inline style is added.
- **`index.html`:**
  - With a branding source, index is rendered per request from a template
    split at boot: prefix, config, suffix. Rendering only fills the config
    JSON, so it stays cheap.
  - In runtime mode, the generated static `theme` extension stylesheet is not
    registered (`provider.rs`). Instead `assets/brand/theme.css?v=…` is the
    **first** entry of `extensions.styles`, so application extension styles
    still come after it and may refine it.
  - The config gains:
    - `brand`;
    - `theme.logo`, `theme.logoDark`, `theme.favicon` as `assets/brand/...`
      paths.
  - The index keeps `Cache-Control: no-store`, so a reload shows the new brand.
  - Without a branding source, behaviour is identical to today: rendered once
    at boot.
- **SPA config parser** (`admin-ui/src/lib/config.ts` `readConfig`):
  - It gains `brand` and `logoDark`, with types.
  - Its path allowlist accepts `assets/brand/(logo|logo-dark|favicon|theme.css)?v=[0-9a-f]{8}`
    beside the existing `assets/ext/...` forms, and nothing else.
- **One branding per document:** the SPA takes the brand name, logos and
  favicon from the index config only, for the whole document until reload.
  `PanelResponse.brand` keeps being sent (resolved from the cache) for
  compatibility, but the shell no longer reads it when `config.brand` is set.
  So the HTML request and the panel request can never mix two brandings.

### 4. Sign-in page (and the static theme)
- `login.tsx`, `recovery.tsx` and the invitation and reset pages show the
  logo, or the default mark, and the brand name above the form, from the index
  config.
- The static `Theme` gets this too: the index config always carries `brand`.
  The brand name becomes visible before sign-in: it is the panel's public
  name, and this is noted in `security/production.md`.
- `document.title` = brand name, on every page.
- **Dark logo:** the SPA picks `logoDark` when `data-scheme` resolves to dark,
  through the existing theme lib (a MutationObserver hook like
  `useColorScheme`). `<picture>` media queries would not follow the in-app
  switch.

### 5. Custom CSS (not in this plan)
Only theme tokens. A later CSS hook would need:
- a parsed allowlist of properties and selectors, scoped to brand classes;
- no `@import`, `url()`, `expression`, `position: fixed`, or `display`,
  `visibility` or `opacity` on auth and security UI;
- serving as a stylesheet from `'self'`, under the unchanged CSP.

It is recorded in the ADR as future constraints only.

### 6. Recovery
- `[admin] branding = "static"`, or the env var `RUSTCMS_ADMIN__BRANDING=static`
  (the config loader's `RUSTCMS_` + `__` mapping), makes the panel ignore the
  source: `load` is never called. It is read at boot, and a `warn` is logged
  once.
- Use case: a bad brand makes the panel unusable, for example unreadable
  colors.
- The token check refuses only malformed values, not ugly ones. Contrast is not
  checked; this is documented.
- The console command `admin:branding` prints:
  - whether a source is set;
  - the effective mode (`runtime` or `static`);
  - in runtime mode only, the result of one `load`, validated, with no bytes
    printed. In static mode it never calls `load`.
  - It changes nothing. Clearing the stored data is the app's job: RDockPanel
    may add its own reset command.

## Test
- **Rust unit:**
  - `Branding` validation:
    - MIME mismatch;
    - SVG, oversize and empty images;
    - a bad token or hex value;
    - a name with control characters or longer than 64 characters.
  - The merge rule.
  - The stylesheet only ever contains checked values.
- **Rust integration** (`crates/rustcms-admin/tests/branding.rs`, with a fake source counting calls and gated by channels):
  - TTL caching: one load per period.
  - Single flight under concurrent requests.
  - `invalidate` takes effect on the next request.
  - Two deterministic interleavings:
    - `invalidate` before the check-and-store: nothing is stored, one retry
      runs;
    - `invalidate` right after the store: the next request reloads.
  - An `Err` or timeout falls back to the static theme and is cached.
  - Brand assets, anonymous:
    - correct MIME, nosniff, CORP, strict CSP;
    - the current `v` gets immutable caching;
    - a stale or foreign `v` gets the current bytes with `no-cache`, never a
      404, as for HTML from another instance or before two quick updates;
    - a missing kind returns 404;
    - HEAD is allowed; POST returns 405;
    - `assets/ext/` still served.
  - Validation refuses SVG.
  - `index.html`:
    - the config carries the brand, escaped (attribute escaping, `<`, quotes);
    - in runtime mode the static theme stylesheet is absent and the brand
      stylesheet is the first style;
    - it is `no-store`;
    - without a source it is identical to today (snapshot), apart from `brand`.
  - `RUSTCMS_ADMIN__BRANDING=static`, through the real config loader, never
    calls `load`; neither does `admin:branding` in static mode.
  - The `admin:branding` command output.
  - The route snapshot in `tests/paths.rs` updated.
  - The asset route is an SPA route, not in `http::routes`, so the oracle does
    not list it. The anonymous tests above cover it instead.
- **Vitest:**
  - `readConfig` keeps `brand`, `logoDark` and `assets/brand/...` paths, and
    drops paths outside the allowlist.
  - The sign-in page shows the logo and brand.
  - The dark logo follows `data-scheme`.
  - `document.title`.
  - The shell's brand comes from the config, not `PanelResponse.brand`.
  - Cascade: when static, runtime and extension styles set the same token,
    the extension value wins over the runtime one, and the static one is
    absent.
- **E2E (Chrome):** the demo gets a runtime source, backed by a demo setting.
  - Change the brand, then reload: the new name, logo and primary color appear
    on the shell and the sign-in page.
  - The static fallback works.
  - No CSP violation.

## Docs
- **Guide:**
  - `admin/customizing.md`: runtime branding, merge rule, cache and TTL,
    `invalidate`, multi-instance delay, recovery.
  - `getting-started/configuration` (or the config reference): `[admin]
    branding`, `branding_ttl_secs`, `branding_timeout_ms`, and
    `RUSTCMS_ADMIN__BRANDING`.
  - `security/production.md`:
    - the brand is public before sign-in;
    - images are PNG/WebP/ICO only;
    - the recovery path.
  - `reference/cli.md`: regenerated for `admin:branding`.
  - `release-notes.md`, `upgrade.md`.
- **Core:**
  - New ADR 0033 (runtime branding):
    - why tokens only;
    - why no SVG;
    - cache and invalidation;
    - the future CSS hook constraints.
  - `CHANGELOG.md`, `admin-protocol.md` (index config fields),
    `architecture.md`, `review-checklist.md`.
- After implementing: recheck all docs against the code, then a Codex docs review.

## Out of scope
- Arbitrary CSS, fonts and SVG logos.
- Per-user or per-tenant branding.
- Cross-instance push invalidation (pub/sub).
- Branded emails.
