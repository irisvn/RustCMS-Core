# ADR 0032: Extension host API v2 (React pages that share the SPA)

- Status: accepted (0.5.0, W5); amended in 0.6.0 (W9: `ui` 2, an additive level)
- Date: 2026-10-18

## Context

Script pages (v1, ADR 0019) get an element and build DOM by hand. Apps such as
RDockPanel want React, the panel's UI kit, React Query, the API client with
CSRF, sub-pages and permission-aware buttons, without shipping a second React.

## Decision

- **Trusted code.** A v2 extension is the app's own code, compiled into the
  binary like a v1 script; it runs with the panel's privileges. The host's
  guarantees protect the user from stale sessions and malformed paths, not
  from a hostile extension. CSP is unchanged (no `unsafe-inline`, no `blob:`).
- **Compatibility before execution.** `AdminProvider::extension_script_v2(name,
  js, Requires { react, react_query, ui })` puts the requirements in the index
  config next to the script URL. The SPA compares them with the host before
  inserting the `<script>`: `api` exactly, `ui` at most the host's level
  (0.5.x required it exactly), `react` and `react_query` the same major and a
  host minor at least the required one. An incompatible
  script never runs; its views show "unavailable" and the console says why.
  Registration checks again. `extension_script` (v1) is unchanged, and
  `window.rustcms.apiVersion` stays 1; v2 is `window.rustcms.v2`.
- **Import ids.** The SDK's Vite preset maps exactly `react`,
  `react/jsx-runtime`, `react-dom` (`createPortal` only),
  `@tanstack/react-query`, `@rustcms/admin-sdk/ui` and
  `@rustcms/admin-sdk/host` to frozen objects in `window.rustcms.v2.modules`.
  Unlisted `react`, `react-dom` and `@tanstack/react-query` subpaths
  (`react-dom/client`, `react/jsx-dev-runtime`) and all dynamic imports are
  build errors; other installed dependencies and local modules are bundled. Extensions are built in production mode.
- **`ui` is an additive level** (0.6.0). Each level only adds components
  and props; removing anything needs host API 3.
  - Level 1: `Alert`, `Badge`, `Button`, `buttonVariants`, `Card`,
    `CardContent`, `CardHeader`, `CardTitle`, `Checkbox`, `Switch`, `Dialog`,
    `Input`, `Label`, `NativeSelect`, `Textarea`, `Page`, `PageHeader`,
    `Loading`, `ErrorState`, `cn`.
  - Level 2 adds:
    - components: `Tabs`, `TabsList`, `TabsTrigger`, `TabsContent`,
      `DropdownMenu`, `DropdownMenuItem`, `DropdownMenuSeparator`,
      `DropdownMenuLabel`, `Tooltip`, `Table`, `TableHeader`, `TableBody`,
      `TableRow`, `TableHead`, `TableCell`, `Skeleton`, `Separator`,
      `CodeBlock`;
    - props: `props.resources` and `props.toast` on pages and slots;
      `props.title` on pages (the shell's heading and breadcrumb for the
      current sub-path, `null` hides the heading; bound to the view, its
      sub-path and the epoch).
- **Declared vs used.** A registration inherits the `Requires` of the
  script running it (`document.currentScript` during loading). The SDK
  (`SDK_UI = 2`) refuses to register on a panel below its level, so even a
  0.5.x panel never runs it, and it passes `{ ui: SDK_UI }`. The panel
  refuses a registration whose SDK level is above its script's declared
  `ui`. Level-2 props go only to scripts that declare `ui >= 2`.
  `make:sdk` lists older declarations in
  `src/providers/admin_panel_provider.rs`; `rustcms upgrade` does so when the
  vendored SDK's VERSION differs from its own.
- **`props.resources`.** It calls the existing resource routes, so policy,
  `scope()`, field rules, validation and audit apply; it adds no server
  routes.
  - Slugs and action names must be `[a-z0-9_-]{1,64}`. Keys are refused when
    they are `.`, `..`, empty, contain control characters or exceed
    256 bytes. Each part is encoded, and the URL must parse back to exactly
    those segments: this is the defense against path confusion
    (`forceDelete(s, ".")` would otherwise become a delete of `force`).
  - Writes share the resource's paused-write scope (its slug) with the
    built-in pages. Creates and actions carry an `Idempotency-Key`.
  - A queued write is rechecked against the mount inside the queue. A pause
    is recorded and a success invalidates queries before a stale mount's
    delivery is refused.
  - The view shows the banner of every resource it used (reads, hooks or
    writes) in the epoch.
  - Reads use the key family `["rustcms.r", slug, …]`, which every write to
    the slug invalidates.
- **`props.toast`.** Bound to the view's epoch. The store is cleared on every
  epoch change, so no message crosses sessions. Successes are polite
  (`status`) and errors assertive (`alert`); at most 5 at a time.
- **Overlays.** Menus and tooltips portal into the open `<dialog>` (the top
  layer) when inside one, else into `body`. They are `position: fixed`, placed
  by per-property CSSOM `style.setProperty` writes of `left`, `top` and
  `max-height`, which `style-src 'self'` does not govern; never style markup,
  `setAttribute("style")`, `cssText` or injected `<style>` elements.
- **Rendering.** `registerPage/Slot/Cell(name, Component)`: the component is
  rendered as an element inside the host tree (one React, one QueryClient,
  theme, i18n, router); there is no `createRoot`. Within each kind (page,
  slot or cell) a name is taken once, by v1 or v2. An error boundary per instance shows "unavailable" with Retry; a
  cell falls back to its plain text.
- **Epoch binding.** The view subscribes to the session epoch with
  `useSyncExternalStore` and is keyed by it: sign-in, sign-out or rotation
  unmounts the instance with its handles. Every host callback (`navigate`,
  `reload`, `api.*`, realtime handlers) is bound to its mount; a stale call
  rejects with `ApiError` `session_changed` and sends nothing, and a response
  that arrives after the mount went stale is never delivered. Writes go
  through `guardedWrite` under the scope `x:<namespace>`, so an unknown
  outcome pauses later writes of that view like built-in writes; nothing is
  retried automatically.
- **Path validation, client and server.** One SPA validator serves `api`,
  realtime and sub-pages: one leading slash; no scheme, `//`, `\`, controls,
  `?`, `#`, empty segments or malformed escapes; each segment decoded once and
  refused if it becomes `.`, `..`, `/`, `\` or still holds `%`; the final URL
  must keep the origin and the `{admin}/api/x/` prefix. Sub-pages
  `GET {admin}/api/pages/{slug}/{*path}` read the raw `OriginalUri`, split
  before decoding, and refuse with a 422 validation error on `path` after the usual 401/403/404 checks
  (segments `[A-Za-z0-9._~-]+`, ≤16 segments, ≤512 bytes).
  `Ctx::page_path()` gives the canonical sub-path to `PanelPage::data`.
- **`permissions`.** `GET {admin}/api/panel` lists the declared admin-guard
  permissions the user holds (all declared ones for a super admin; that
  disclosure is accepted). `can()` is UX only and false while the panel data
  is loading or of another epoch.
- **CSP build gate.** `rustcms-build` runs one Vite IIFE build per
  `src/<vendor>/<name>.tsx`, emits its own CSS file (empty when unstyled), and
  never injects CSS at runtime. The E2E test loads a CSS-bearing bundle with a
  dialog portal under the production CSP and fails on any violation.
- **SDK vendoring and drift.** `admin-ui/sdk/` is canonical; the CLI template
  and the demo hold copies checked equal by `just generated`. Apps vendor it
  in `resources/admin/sdk` (`file:./sdk`). `sdk/VERSION` and `sdk/MANIFEST`
  let `rustcms upgrade` warn about an older SDK and `make:sdk`
  replace only unmodified files. Built `js/` and `css/` files are committed,
  so `cargo build` needs no Node.

## Consequences

- v2 extensions are coupled to the host's React 19 and React Query 5 majors;
  a host major upgrade refuses them at load time until they are rebuilt.
- v1 scripts keep working unchanged.
- Out of scope: lazy chunks, third-party (untrusted) extensions, HMR against
  the running panel.
