# W5 plan: extension host API v2, React pages that share the SPA (round 4, 2026-10-18; Codex rounds 1–3 applied; implemented in 0.5.0; historical design)

Builds on the 0.4.0 draft (`v0.4.0-panel-platform.md`, "W5") and keeps its
review notes:
- v1 stays separate;
- `requires` is checked **before a script runs**;
- the sub-path is in requests and cache keys, with oracle cases;
- the SDK is vendored, not published.

Extension code is **trusted** (same origin, `script-src 'self'`; it can already
call `fetch`). The rules below are the supported contract, so that bugs in
extensions fail closed; they are not a sandbox. The guide says so.

## Why

Script pages (v1) get an element and build DOM by hand. RDockPanel's pages
(container lists, logs, the terminal) want:
- React, the panel's UI kit and React Query;
- the API client with CSRF;
- sub-pages, and permission-aware buttons;
- all without a second React.

## Decisions to confirm (the maintainer's)
1. **Version: 0.5.0** (a new public API).
2. **Coupling:** v2 extensions run on the host's React 19 and React Query 5.
   A host major upgrade breaks them, and they must be rebuilt; `requires`
   makes that a load-time refusal.

## Design

### Compatibility, before execution
- **`AdminProvider::extension_script_v2(name, js, Requires { api: 2, react:
  "19.3", react_query: "5.104", ui: 1 })`** carries the requirements in the
  index config, next to the script's URL. The SPA compares them with the
  host **before inserting the `<script>`**: an incompatible script never
  runs. Its views show "unavailable", and the console says why.
  - **Matching:** `api` and `ui` exactly; `react` and `react_query` the same
    major and a host minor at least the required one.
- Registration checks the same again (defensive).
- Plain `extension_script` keeps v1 behaviour. A v1 script that checks
  `window.rustcms.apiVersion === 1` still sees 1 (`apiVersion` stays 1;
  v2 is `window.rustcms.v2`).

### `window.rustcms.v2`
- **`modules`:** frozen objects holding the host's instances. The **exact
  import ids** the preset maps:

  | Import id | Exposes |
  |---|---|
  | `react` | the namespace (default and named exports, both mapped) |
  | `react/jsx-runtime` | `jsx`, `jsxs`, `Fragment` |
  | `react-dom` | `createPortal` only |
  | `@tanstack/react-query` | `useQuery`, `useMutation`, `useQueryClient`, `useInfiniteQuery` |
  | `@rustcms/admin-sdk/ui` | the UI list below |
  | `@rustcms/admin-sdk/host` | `ApiError`, `versions` (host runtime values) |

  - The vendored SDK is a local package (`resources/admin/sdk`, referenced
    as `"@rustcms/admin-sdk": "file:./sdk"`).
    - Its **hooks are real code**, bundled into the extension, importing
      only `react` and `@tanstack/react-query`. Those stay external and map
      to the host's instances.
    - Its `/ui` and `/host` entries are **type-only stubs** that the preset
      maps to `window.rustcms.v2.modules.ui` and `.host`. `ApiError` is
      therefore the host's own class, and `instanceof` works.

  - `react/jsx-dev-runtime`, `react-dom/client` and any other subpath are
    build errors in the preset (never shipped).
  - Extensions are always built in production mode.
- **`ui` v1 list** (fixed; adding is minor, removing bumps `ui`): the
  components that exist in `components/ui/` at implementation time, named
  in the ADR, plus `Page`, `PageHeader`, `Loading`, `ErrorState`, `cn`.
  `Table` is included only if a `components/ui/table.tsx` exists by then.
- **`versions`:** `{ api: 2, react, reactQuery, ui: 1 }`, read from the
  host's own package versions at build time.
- **`registerPage(name, Component)`, `registerSlot(name, Component)`,
  `registerCell(name, Component)`.** Registered components are rendered as
  elements (`<Component {...props} />`), never called as functions. One name
  registry is shared with v1: a name is taken once, by either version. After
  the deadline, registration is refused.

### Mounting, lifecycle, errors
- A v2 view renders **inside** the host tree. There is no `createRoot`, so
  the QueryClient, theme, i18n and router are shared.
- **Epoch:** the host component subscribes with
  `useSyncExternalStore(useCallback((l) => api.onEpochChange(l), [api]),
  () => api.epoch)` (a closure bound to `api`), so an epoch
  change (sign-in, sign-out, rotation) re-renders. The view is keyed by
  `(registration, epoch, slug or record)`: a new epoch unmounts the old
  instance, with all its handles, and mounts a fresh one.
- Every host-made callback (`navigate`, `reload`, `api.*`, the realtime
  handlers) is **bound to its mount**. After unmount or an epoch change it
  does nothing: requests are not sent, and responses and errors are dropped.
- **Errors:**
  - An error boundary per view catches errors in render and in lifecycle
    (effects). The view shows "unavailable" with a Retry that remounts it.
    Only that instance fails; others of the same registration are
    unaffected. A cell falls back to its plain text.
  - Event-handler, timer and promise errors are not caught by React. Host
    callbacks the extension's realtime handlers run are wrapped, so a throw
    is logged and the stream goes on. Unhandled rejections stay the
    extension's own (stated).
  - Cleanup continues past a throwing handle.

### Props
- **`PageProps`:**
  - `data`, `locale`, `t`, `navigate`, `reload`;
  - `slug`;
  - `path`: the canonical sub-path, `""` for none;
  - `search`: a frozen `URLSearchParams` copy, reactive: a change re-renders
    with new props;
  - `api`, `realtime`, `can`.
- **`SlotProps`:** `slot` (its name), `resource`, `recordKey`, `data`,
  `record`, `locale`, `t`, `navigate`, `reload`, `api`, `realtime`, `can`.
- **`CellProps`:** `value`, `record`, `props`, `locale`, `t`, `can`. There is
  no `api` or `realtime`.
- `data` and `record` are frozen deep copies.

### `api` (the facade)
- `get`, `post`, `put`, `patch`, `delete` (`put` added to `ApiClient` through
  `request`).
- They **delegate to the panel's `ApiClient`**, so CSRF, identity headers,
  stale-response handling and unknown-write outcomes stay as they are.
- **Epoch, before and after.**
  - **Every promise settles.** A call from a stale mount (another epoch, or
    unmounted) rejects at once with `ApiError` code `session_changed`
    (status 0), and nothing is sent.
  - **A response that arrives after the mount went stale** also rejects
    with `session_changed`; its data is never delivered. The hooks
    suppress their callbacks separately.
  - **Unknown write outcomes:** facade writes go through the host's
    `guardedWrite` under the scope `x:<namespace>`, as built-in writes do.
    - Every `ApiError` with `maybeApplied` (`outcome_unknown`,
      `incomplete_response`, a network failure with `outcomeUnknown`) is
      recorded **before** any stale-delivery conversion.
    - Later writes in that scope are refused until the user resolves it.
      The panel's "paused write" banner shows it.
    - It is kept across a same-user credential rotation and never carried
      to another user.
    - Nothing is retried automatically.
- **Paths:** written as `/docker/containers` (exactly one leading slash, as
  v1 realtime paths already are), relative to `{admin}/api/x`. One shared
  function validates them for `api` and `realtime`. The v1
  `realtime.events(path, handlers)` and `socket(path, handlers, options)`
  signatures are unchanged; they go through an adapter that accepts that
  one leading slash. Queries are a separate `options.query`.
  - **Refused before any decoding:**
    - a scheme or `//`;
    - `\`, control characters, `?` and `#` (queries go in `options.query`);
    - an empty segment (inside the path; the one leading slash is fine);
    - malformed `%` escapes.
  - **Then each segment is decoded once.** It is refused when it decodes to
    `.`, `..`, `/` or `\`, or still contains `%` (double encoding).
  - **Last check:** the final URL is built with `new URL(…, location.origin)`
    and must keep the same origin and start with `{admin}/api/x/` at a
    segment boundary.
  - Tests: `%2e%2e`, `%2E.`, `%252e`, `a%2Fb`, `a\b`, `//evil`,
    `http://evil`, a custom admin prefix.
- `ApiError` is a runtime export of the SDK (a re-export of the host's
  class, through `modules`).

### Hooks (SDK, on the props)
- **`props.scope`:** an immutable value the host issues per mount:
  `{ namespace, epoch }`, where `namespace` is `page:<slug>`,
  `slot:<slot>:<resource>:<key>` or `cell:<resource>:<field>`.
- **`useAdminQuery(props, path, { query, … })`:**
  - `query` is a record of string or string-list values;
  - it is serialized into a sorted list of `[name, value]` pairs, duplicates
    kept and in order;
  - that list is both the key part and exactly what is sent;
  - key: `["rustcms.x", scope.namespace, scope.epoch, locale, canonicalPath,
    pairs]`;
  - duplicate keys share one request (React Query).
- **`useAdminMutation(props, path, method)`:**
  - invalidates `["rustcms.x", scope.namespace, scope.epoch]` on success;
  - an obsolete completion (another epoch, or unmounted) runs no callback;
  - writes are never retried or replayed automatically.
- **`useEvents(props, path, handlers)`, `useSocket(props, path, handlers,
  options)`:**
  - they open their handle in an effect and close it in its cleanup, and
    the close is idempotent;
  - the handlers are kept in a ref, so changing them does not reopen the
    handle; the path and options do;
  - StrictMode's setup, cleanup, setup sequence leaves exactly one open
    handle.
- The page query keeps its existing key dimensions, with the sub-path
  added.

### Sub-paths
- **SPA:** the route `/p/$slug/$` next to `/p/$slug`, rendered by
  `CustomPage`.
  - The sub-path is validated from the **raw** `location.pathname`
    (still encoded), with the same rules as the server, before the router's
    decoding is trusted.
  - The page request rebuilds it segment by segment with
    `encodeURIComponent`, so it is never sent as one raw string.
  - Its canonical form goes into the page query key; back and refresh
    restore it.
  - An invalid sub-path shows "not found" without a request.
- **Server:** `GET {admin}/api/pages/{slug}/{*path}`.
  - The handler reads the **raw** path from `OriginalUri`, not axum's
    decoded `Path`, and splits it on `/` **before** decoding.
  - **Refused (400 `invalid`):**
    - malformed escapes, invalid UTF-8;
    - a segment that decodes to `/`, `\`, `.` or `..`, or still contains `%`;
    - an empty segment (a single trailing slash is removed first);
    - more than 512 raw bytes or 16 segments;
    - a decoded segment outside `[A-Za-z0-9._~-]+`.
  - **Precedence as today:**
    - 401 without a session;
    - 403 when the user may open no page;
    - 404 for an unknown slug;
    - 403 without the page's permissions;
    - then 400 for the sub-path;
    - only then `data()`.
  - `Ctx::page_path()` (`""` without one) is available to `PanelPage::data`.
    The page's permissions apply to every sub-path; a page whose sub-paths
    need more checks them in `data`.
- **Oracle:** both routes go in `rustcms_admin::http::routes`;
  `fixtures/authz/admin.toml` gets cases for each actor, a sub-path, an
  invalid one, and an unknown slug.

### `can(permission)`
- `GET {admin}/api/panel` gains `permissions`: from the auth
  `PermissionRegistry`, the admin-guard permissions (including those declared
  by `AdminRoutes` and pages) **that the user holds**, sorted.
  - Other guards, undeclared or orphaned names, and unheld ones are left out.
  - A super admin gets every declared admin-guard name. This disclosure is
    accepted: they can see the role editor anyway.
  - The contract fixture and `types.gen.ts` are regenerated.
- The panel query gets the epoch in its key. `can` is false while the panel
  data is loading, errored or of another epoch. It is UX only; the server
  checks every request.

### Styles and the build (the CSP gate)
- **The preset (`rustcmsExtension()`), with `npm run build` running
  `rustcms-build`, the preset's small Node script:**
  - it discovers `src/*/*.tsx` and runs **one Vite build per entry** (Rollup
    allows a single input for IIFE);
  - Vite library mode, IIFE;
  - `build.cssCodeSplit: false`, so each build emits its own
    `css/<vendor>/<name>.css`;
  - an **empty CSS file** is written when an entry has no styles, so the
    unconditional `include_str!` always finds one;
  - `emptyOutDir: false`, so one entry's build never deletes another's
    files; outputs of removed entries are deleted by name;
  - **no runtime CSS injection** (no `vite:css` injection into JS), no
    HMR or refresh bootstrap, no dynamic imports or lazy chunks (a build
    error);
  - deterministic file names (no hashes: the panel hashes them).
- **Classes:** the host stylesheet has only the classes the host uses.
  Extensions bring their own CSS, built by their own Tailwind pass over
  their sources, using the host's tokens (`var(--…)` theme variables,
  documented). The `ui` components carry their own classes and need
  nothing.
- The CSS is registered with `extension_style`. Portals render inside the
  host, so they are styled by the host and by the extension's CSS.
- **Gate:** the E2E test loads a CSS-bearing bundle with a dialog (portal)
  under the unchanged production CSP and fails on any violation.

### Loading
- `AdminProvider::extension_timeout(Duration)`: 1 to 60 s, default 10, sent
  in the index config.

### App source convention and CLI
- **Sources:** `resources/admin/` holds:
  - `package.json` (pinned versions, `"engines": { "node": "^20.19.0 ||
    >=22.12.0" }`, which is what Vite 7 needs), a standalone
    `tsconfig.json`, and `package-lock.json`;
  - `vite.config.ts` and `sdk/` (vendored);
  - `src/<vendor>/<name>.tsx`.
- **Outputs:** `resources/admin/js/<vendor>/<name>.js` and
  `resources/admin/css/<vendor>/<name>.css` are **committed**, so
  `cargo build` needs no Node (tested with `node_modules` removed).
- **Entries:** `vite.config.ts` discovers every `src/*/*.tsx` as an entry, so
  a second page adds a file and overwrites nothing.
- **`rustcms make:page <Name> --react`** (exclusive with `--script` and
  `--template`):
  - checks for collisions first (the Rust file, the TSX, the slug);
  - writes the `PanelPage` (`PageView::Script("<vendor>/<name>")`), the TSX,
    the provider registration (`extension_script_v2` and `extension_style`
    with `include_str!`) and `lang/*.toml` examples;
  - creates `package.json`, `tsconfig.json`, `vite.config.ts` and `sdk/` only
    if absent, and never overwrites them;
  - prints:
    - for a new setup: `cd resources/admin && npm install && npm run
      build`, then commit `package-lock.json` and the outputs;
    - afterwards: `npm ci && npm run build`.
- **CI:** the CLI job provisions Node 22. The scaffold test:
  1. generates a React page;
  2. runs `npm install` once (it writes the lockfile);
  3. removes `node_modules`, then runs `npm ci && npm run build`;
  4. removes `node_modules` again and checks that `cargo build` passes.

### SDK (vendored) and drift
- **One canonical source:** `admin-ui/sdk/`. The CLI's template copy
  (`crates/rustcms-cli/templates/sdk/`) is generated from it and checked
  equal by `just generated` (CI).
- **Portable:** there are no `@/` paths and no host-private dependencies.
  The declarations are standalone. A test builds a fixture extension outside
  `admin-ui` with `tsc --strict --noEmit` against the copied SDK.
- **`sdk/VERSION`** holds the RustCMS version. `rustcms upgrade` compares it
  and warns: "older SDK in resources/admin/sdk: run `rustcms make:sdk
  --update`". It never overwrites edited files; `--update` replaces only
  unmodified files, by checksums in `sdk/MANIFEST`, and reports the others.
  The warning appears even when Cargo is already current.
- **Types:** `sdk/index.d.ts` declares the props, the host API,
  `Requires`, the hooks, `ApiError` and the `ui` components' props. A
  host-side type test fails when the host's exports drift from those
  declarations.

## Test
- **Vitest (admin-ui):**
  - **v1 unchanged:**
    - `apiVersion === 1` scripts;
    - the v1 realtime paths (`/docker/…`) through the adapter;
    - a name already taken by a v1 registration is refused for v2, and the
      reverse;
    - registrations after the freeze are refused;
    - so are registrations of a script that missed its deadline, even
      before the freeze.
  - **Compatibility:** an incompatible v2 script's top-level side effect
    never runs; the defensive check at registration.
  - **Rendering:**
    - pages, slots and cells inside the host tree;
    - a shared QueryClient (the host cache holds the extension's query);
    - `createPortal` works;
    - production and development host builds both render a compiled,
      hook-using fixture extension.
  - **Epoch:**
    - a rotation re-renders and remounts;
    - a saved `api.post` called after an account change is refused;
    - a delayed response after unmount is dropped;
    - realtime handles are closed;
    - StrictMode leaves one handle.
  - **Paths:**
    - every case in "Paths", with a custom admin prefix;
    - the SPA's raw-pathname check;
    - segment-wise encoding of the page request;
    - reactive `search` and `path`.
  - **The `api` facade:**
    - a 422 maps to `ApiError` with field errors;
    - a CSRF failure (`csrf_mismatch`) refreshes the token and the call can
      be retried by the extension;
    - stale calls and stale responses reject with `session_changed`;
    - a network failure, a truncated success and an `outcome_unknown` write
      are each recorded, and a later write in the scope is refused until
      resolved;
    - two concurrent writes;
    - an account change drops the record, while a same-user rotation keeps
      it.
  - **Query keys:** pairs serialized deterministically, duplicates kept,
    two different queries never share a key.
  - **Errors:**
    - render and effect throws mark one instance, and Retry remounts it;
    - a throwing realtime handler keeps the stream;
    - cleanup goes on past a throwing handle;
    - the cell fallback.
  - **`can`:** false while loading or for another epoch; follows
    `permissions`.
  - **The deadline**, and translated host fallbacks (`t()` keys exist in en
    and vi).
- **Rust (`db_test!`):**
  - the sub-path route: precedence; `page_path`; every invalid form (`%2F`,
    `%2e%2e`, `%252e`, invalid UTF-8, too long, too many segments);
  - oracle cases for both routes;
  - `panel.permissions`:
    - a super admin, a role, a direct grant;
    - a name of another guard, an undeclared name;
    - after revocation;
  - contract fixtures and `types.gen.ts` regenerated;
  - new validation messages have `i18n::TEMPLATES` entries
    (`crates/rustcms-admin/tests/issues.rs`).
- **CLI:**
  - `make:page --react` files, collisions, and that it is exclusive;
  - a second page;
  - existing config left alone;
  - the scaffold build: `npm install`, then `npm ci` from the lockfile and
    `npm run build`, then `cargo build` without `node_modules`;
  - the SDK copy equals `admin-ui/sdk/`; `make:sdk --update` keeps edited
    files;
  - `rustcms-build` with two CSS-bearing entries: a rebuild keeps both,
    and an entry without styles gets an empty CSS file.
- **E2E (Chrome), in the demo** (a React page built with the SDK):
  - a sub-route (`/admin/p/react-demo/items/2`): it loads, survives a
    refresh, and back works;
  - a list with `useAdminQuery`, a save with `useAdminMutation` (CSRF), a
    stream with `useEvents`;
  - a dialog (portal) with the extension's CSS;
  - no CSP violation, and exactly one React instance;
  - a slot and a cell of the same extension.

## Docs
- ADR 0032:
  - the v2 host API and its trusted-code contract;
  - compatibility before execution;
  - the import ids;
  - epoch binding;
  - path validation (client and server);
  - `permissions` exposure;
  - the CSP build gate;
  - SDK vendoring and drift.
- Guide `admin/react-pages.md` (new, linked in `SUMMARY.md`):
  - `make:page --react`, the build and Node, committed outputs;
  - props, `api`, hooks, sub-pages;
  - styles with the host's tokens;
  - realtime from React;
  - errors;
  - testing;
  - upgrading the SDK.
- Guide `admin/customizing.md`: v1 vs v2.
- `reference/cli.md` regenerated.
- CHANGELOG, the upgrade guide, and a CLAUDE.md rule: v2 extensions never
  call `createRoot`, import only the mapped ids, and commit their built
  files; extension paths go through the shared validator.

## Out of scope
- Publishing `@rustcms/admin-sdk` on npm.
- React form widgets (a v2 `registerWidget`).
- Running an app's Vite build from `cargo build`.
