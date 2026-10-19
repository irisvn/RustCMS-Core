# W9 plan: panel home, navigation order and hidden resources (0.5.1); resource API and UI kit 2 for React pages (0.6.0)

Round 3, 2026-10-18 (Codex rounds 1–2 applied). Requested by RDockPanel
(Phase 3: an EasyPanel-like interface on React pages). Approved; part A
shipped in 0.5.1, part B in 0.6.0. The text below is the historical design.

## Status (implemented)
- Final APIs: `useResource(props, slug, { query, ...options })`; controlled
  `Tabs` with a labelled `TabsList`, content rendered only when selected;
  `DropdownMenu` takes its trigger content directly (it renders its own
  button).
- Overlays portal into the open `<dialog>`, else directly into `body` (no
  separate root), and set `left`, `top` and `max-height` with CSSOM.
- Beyond the plan:
  - `PanelPageMeta::permission_any` (server, any-of page permissions);
  - more navigation icons;
  - the built-in lists use the kit's `Table`.
- Noted while implementing: JSON `.required()` refuses a missing field or
  `null` but accepts `""`, while a CSV import refuses an empty required cell.
  `props.resources` sends values unchanged.

## Why
RDockPanel replaces the resource lists in its menu with its own React pages
(Dashboard, Templates, Monitor, Settings, a Docker group). It needs:
- a chosen home page and a chosen sidebar order;
- resources that keep their routes but have no sidebar entry;
- the resource API from React pages, so its policies, validation, audit and
  encrypted fields are reused;
- more UI components.

## Releases
- **0.5.1 — part A** (server and SPA only, no SDK change): home, sort, and
  hidden navigation.
- **0.6.0 — part B** (SDK, `ui` level 2): `props.resources`, resource
  hooks, `props.toast`, the new components, and the additive `ui` rule.
- **Public response structs gain fields** (`PanelResponse`, `ResourceInfo`,
  `PageLink`). They are output types built by the panel; an app that
  constructs them with struct literals breaks. This is stated in the
  upgrade guide.

## Part A (0.5.1)

### A1. Home page
- `AdminProvider::home(slug)`: `/admin` opens that custom page.
  - An unknown slug panics at build, as a duplicate slug does.
- `PanelResponse.home: Option<String>` (the slug only) is set only when the
  user may open that page, by the same `allowed` check as the page links.
  - Otherwise it is absent and the default dashboard shows. There is no 403
    on `/admin`.
  - A user who may not open the page learns neither its slug, label nor
    icon.
- **SPA:**
  - The index route shows `Loading` until the panel data arrives (no flash
    of the dashboard, no dashboard request). It then redirects with
    `replace` to `/p/{home}`.
  - The "Overview" entry becomes the home page's entry (its label and
    icon), and the page is not listed again in its group.
  - The default dashboard is not reachable while a home is set. Its widgets
    route stays, authorized as now, for apps without a home.
- **Breadcrumbs:** a custom page is recognized on `/p/{slug}` **and**
  `/p/{slug}/…` (sub-pages). Today only the exact path is.

### A2. Sort
- `PanelPageMeta::sort(i32)` and `ResourceMeta::sort(i32)`, default 0.
  - `PageLink.sort` and `ResourceInfo.sort` are sent only when not 0.
- **Order** (unchanged when nothing is set):
  - Today's base order is resources by slug (the registry sorts them), then
    pages in registration order.
  - Entries are ordered by `(sort, base order)`: a stable sort on today's
    list.
  - Groups are ranked by the smallest `sort` among their **listed**
    entries, then by first appearance in the base order. Hidden resources
    and the extracted home entry do not count. The ungrouped entries form
    one group like the others.
  - System and Settings stay last (built in).
- **RDockPanel's wish:**
  - Dashboard is `home`;
  - Templates, Monitor and Settings are pages with sort 1, 2, 3;
  - the Docker group's entries get sort 10 and up.

### A3. Hidden resources
- `ResourceMeta::navigation(false)`: no sidebar entry and no dashboard card.
  - Its routes, record pages, API, relations, audit links and breadcrumbs
    are unchanged.
- `ResourceInfo.nav: false` (sent only when false). `panel.navigation` still
  lists the resource, but **only for users with `view_any`, as now**. The
  breadcrumb, audit and record pages look it up there; the shell and the
  dashboard cards skip it.
- UX only: nothing about authorization changes, and nothing unauthorized is
  disclosed.

### Test A
- **Rust:**
  - full-response assertions (`tests/custom.rs`, `http_api.rs`):
    - `home` is present for a user who may open the page, and absent
      (with no label or icon anywhere) for one who may not;
    - an unknown home slug panics;
    - `sort` and `nav` appear only when set;
    - a hidden resource is listed only with `view_any`;
    - its records API still answers, and still 403s without `view_any`;
  - the contract fixture is **populated** with home, sort and nav (kitchen
    sink), then regenerated together with `types.gen.ts`;
  - the oracle cases are unchanged (no new routes).
- **Vitest:**
  - the index redirects to the home with no dashboard request; with no home,
    or one not allowed, the dashboard shows;
  - the sidebar order:
    - resources registered in a non-alphabetical order;
    - ties keep today's order;
    - group rank ignores hidden entries and the home;
    - the order is unchanged with no sort;
  - a hidden resource has no link, but its record page and breadcrumb work;
  - the breadcrumb on `/p/{home}/a/b`.
- **E2E:** unchanged (the demo sets nothing). Vitest covers the shell.

### Docs A
- `admin/customizing.md`: Home and order.
- `admin/resources.md`: `navigation(false)` and `sort`.
- CHANGELOG, release notes, upgrade guide (0.5.1: nothing to change; the
  struct-literal note).

## Part B (0.6.0)

### B1. `props.resources` (pages and slots; not cells)
- **Methods:**
  - `list(slug, query?)` and `get(slug, key)`;
  - `create(slug, data)` and `update(slug, key, data)`;
  - `delete(slug, key)`, `restore(slug, key)` and `forceDelete(slug, key)`;
  - `action(slug, name, keys, data?)`.
- **Paths** go through a shared validator in `lib/paths.ts`
  (`resourcePath`), before anything is queued:
  - slug and action name: `[a-z0-9_-]{1,64}`;
  - key: non-empty, at most 256 bytes, no control characters, and not `.`
    or `..`;
  - each part is sent through `encodeURIComponent`;
  - the built URL is parsed with `new URL`. Its pathname must equal the
    expected segments exactly (no normalization happened) and stay under
    `{admin}/api/resources/`.
  - Regression tests cover `forceDelete(s, ".")` (it would become
    `/records/force`), `".."`, `"a/b"`, `"%2e"`, and a custom admin prefix.
  - The server does not add key rules: the client validator is the defense
    against path confusion.
- **Writes and the mount.** `guardedWrite` and `keyedWrite` take a `send`
  callback that runs **inside** the queue and rechecks the mount (as
  `facade.ts` does now). The ordering is fixed:
  1. a stale call or a stale queued write rejects with `session_changed`
     and sends nothing;
  2. for a sent write, a maybe-applied error is recorded (paused) and a
     success invalidates the queries;
  3. **then** delivery to a stale mount is suppressed (`session_changed`).
  - `create` and `action` use `keyedWrite` (`Idempotency-Key`, a pending
    write). `update`, `delete`, `restore` and `forceDelete` use
    `guardedWrite`, as the panel's own pages do.
- **Paused writes and recovery:**
  - The scope is the **resource slug**, the same as the resource's built-in
    pages.
  - The view's mount records each resource scope it **uses**: every
    `props.resources` read and hook subscription, and every write attempt,
    recorded before the pause check. A view opened while its resource is
    already paused therefore shows the banner before its first write. `ReactView`
    renders a `PausedBanner` for each paused scope among them, next to its
    `x:<namespace>` banner. The record lives in `ReactView` state, so it
    survives a Retry remount within the epoch and is cleared on an epoch
    change.
  - "Retry safely" re-sends the pending write (identity, body and key
    unchanged) through `retryPending`.
  - A recovered result only invalidates queries and shows "applied". It
    never calls an extension callback: the extension's promise has already
    settled.
  - Tests:
    - pause from the extension, recover from the built-in list;
    - pause from the built-in form, banner in the extension;
    - entering, and entering again, an extension whose resource is already
      paused shows the banner before any write;
    - two scopes give two banners;
    - unmount during the retry;
    - a session change drops the pending write for another user;
    - a failed retry keeps the pause.
- **Queries and invalidation:** resource reads use a key family
  `["rustcms.r", slug, namespace, epoch, locale, …]`.
  - `invalidateAfterWrite(slug)` invalidates `["rustcms.r", slug]` too, so
    built-in writes, facade writes and recovered writes refresh every
    extension view of that resource. Built-in keys are invalidated as now.
- **Updates (today's semantics, documented):**
  - `PATCH` with the changed fields; last write wins.
  - There is no record version: an undeclared `version` field is a 422.
  - Encrypted maps carry their `revision`; a stale one is `409
    secret_stale`. The guide tells extensions to keep the user's draft on a
    409 or 422.
  - Generic optimistic concurrency is out of scope.
- **Errors:** `ApiError` (`status`, `code`, `issues`, `formIssues`). The SDK's
  `fieldErrors(error)` returns `Record<field, string[]>` of translated
  messages.
- **SDK:**
  - `useResource(props, slug, query)` and `useRecord(props, slug, key)`;
  - `useResourceMutation(props, slug, op)` with `op` one of `create`,
    `update`, `delete`, `restore`, `forceDelete` or `action`;
  - never retried; callbacks dropped when the view is stale.
- **Authorization:** unchanged. Every call uses the existing routes, so
  policy, `scope()`, field rules and audit apply.

### B2. `props.toast` (pages and slots)
- `props.toast.success(text)` and `props.toast.error(text)`, bound to the
  view's epoch:
  - after an epoch change they do nothing;
  - after unmount within the same epoch they still show (feedback for the
    user).
- **The panel renders the region:**
  - `role="status"` (polite) for successes, `role="alert"` for errors;
  - at most 5 toasts; successes dismissed after 5 s, errors after 8 s; a
    close button.
- The store is **cleared on every API epoch change** (sign-out, sign-in,
  another user, credential rotation, a CSRF reset), so no message crosses
  sessions.
- Text only: no HTML, and the `t()` of the extension's catalogs applies.
- Tests:
  - a stale emission is ignored;
  - toasts are cleared on sign-out and sign-in;
  - the limit, dismissal and roles.

### B3. UI kit `ui` 2
New host components in `admin-ui/src/components/ui/`, used by the panel
where it fits:
- **`Tabs`, `TabsList`, `TabsTrigger`, `TabsContent`:**
  - controlled `value` and `onValueChange`;
  - `role` `tablist`, `tab` and `tabpanel`, with
    `aria-controls`/`aria-labelledby` and `aria-selected`;
  - roving focus with the arrows and Home/End;
  - the guide's sub-path recipe: `value={props.path || "overview"}` and
    `onValueChange={(v) => props.navigate("/p/x/" + v)}`.
- **`DropdownMenu`:**
  - a trigger and items (`onSelect`, `destructive`, `disabled`), a
    separator and a label;
  - the keyboard: arrows, Home/End and Enter/Space. Escape closes and
    returns focus to the trigger. Tab and Shift+Tab close the menu and move
    focus forward or back from the trigger, as if the menu were not there.
    Both are tested inside a native dialog;
  - closes on an outside click.
- **`Tooltip`:**
  - opens on hover and on focus, after a delay;
  - closes on Escape;
  - stays while the pointer is over the tooltip;
  - `aria-describedby`.
- **Overlays and dialogs:**
  - A `PortalContainer` context is set by `Dialog` to its `<dialog>`
    element (which is in the top layer after `showModal()`); otherwise it
    is a host overlay root at the end of `body`.
  - Menus and tooltips portal into that container, so they render above
    and inside a modal's interactive subtree.
  - Tests:
    - a menu inside a dialog;
    - Escape closes the menu first, then the dialog;
    - Tab;
    - focus return;
    - edges of the viewport, scroll and resize.
- **Positioning, CSP-safe:**
  - a layout effect computes the position and assigns it per property with
    CSSOM: `el.style.setProperty("left", …)` and `top`;
  - never a `style` attribute in markup, `setAttribute("style")` or
    `cssText`;
  - only per-property `style.setProperty("left" | "top", "<n>px")`
    assignments, which `style-src 'self'` does not govern (it governs
    `style` attributes, `<style>` elements and `cssText`-like whole-string
    writes);
  - coordinates are `position: fixed` relative to the viewport for both
    containers (the top-layer `<dialog>` and the body root), recomputed on
    scroll (capture) and resize;
  - the E2E (Chromium, as the suite) checks the actual placement and
    hit-testing (`elementFromPoint`) after `showModal()`, scroll and resize,
    and the absence of any `securitypolicyviolation` under the unchanged
    production CSP (`panel/spa.rs`).
  - No third-party popover library.
- **`Table`, `TableHeader`, `TableBody`, `TableRow`, `TableHead`,
  `TableCell`:** the panel list's classes. The panel's list renderer is moved
  onto them.
- **`Skeleton`, `Separator`.**
- **`CodeBlock`:** a `<pre>` with a copy button (`navigator.clipboard`, and
  a message when it is unavailable), no highlighting.
- `ui.d.ts`, the `ui` list in ADR 0032, and the host-type drift test are
  updated.

### B4. Compatibility: `ui` becomes an additive level
- **The host accepts `required.ui <= host.ui`** (today: equal).
  - `ui` 2 covers the new components, `props.resources` and `props.toast`.
  - Removing anything from `ui` or from the props now needs `api: 3`
    (ADR 0032 amended).
- **Requirements belong to scripts; registrations inherit them.**
  - The loader keeps `document.currentScript` during a v2 script's
    execution, and each `register*` call made then is recorded with that
    script's `Requires`. The record is kept per `(kind, name)` and used for
    the registration checks and for which props are given.
  - A registration outside a v2 script's execution (v1 scripts, which have
    no `Requires`) is v1, as now.
- **Declared vs used, checked by both sides:**
  - The new SDK carries its level (`SDK_UI = 2`). **Before registering**, it
    checks `host().versions.ui >= SDK_UI` itself and throws ("this page needs
    a newer panel"). So even a 0.5.x host, which ignores extra arguments,
    never runs it: the script fails to register and its views show
    "unavailable". This works whatever the provider declared.
  - It also passes `{ ui: SDK_UI }` with the registration. A 0.6.0+ host
    refuses a registration whose SDK level is above its script's declared
    `Requires.ui`. The console says: "built with SDK ui 2 but `Requires`
    declares ui 1: update the provider".
  - An old-SDK bundle passes no level and is treated as 1. The SDK-level
    check applies to v2 registrations only.
- **`props.resources` and `props.toast`** are given only to registrations
  whose script declares `ui >= 2`.
- **Matrix, tested:**

  | Bundle | 0.5.x host | 0.6.0 host |
  |---|---|---|
  | `ui` 1, old SDK | runs | runs (no `resources` or `toast`) |
  | `ui` 2, new SDK | refused before running (`Requires`) | runs |
  | new SDK, declares `ui` 1 (a mistake) | refused by the SDK's own check at registration | refused at registration, with the message |
  | v1 scripts | unchanged | unchanged |

- Tests:
  - two names in one script;
  - scripts of mixed `ui` levels;
  - an old-SDK bundle;
  - a v1 script with no `Requires`;
  - the SDK's own check against a host reporting `ui` 1.
- **CLI:**
  - `make:page --react` writes `ui: 2`;
  - `make:sdk` and `rustcms upgrade` list every `extension_script_v2`
    registration in the provider that declares an older `ui`, and print the
    edit;
  - edited SDK files are kept, as now, and the kept files are listed.

### Test B
- **Vitest:**
  - the resources facade:
    - every method's method, path, body and `Idempotency-Key`;
    - path confusion (above);
    - stale and queued-after-unmount writes are never sent;
    - the order: record, invalidate, then suppress;
    - the recovery cases (above);
    - a 422 through `fieldErrors`;
    - the cross-invalidation of `rustcms.r` keys;
  - toast (above);
  - the `ui` 2 components (above);
  - compatibility:
    - the matrix rows;
    - a `ui` 3 bundle is refused;
    - the declared-vs-SDK-level refusal;
    - `resources` and `toast` only given at `ui >= 2`.
- **Rust:**
  - the CLI tests: `ui: 2` in `make:page --react`, and the listing of old
    registrations by `make:sdk`;
  - no server change.
- **E2E (the demo React page):**
  - create, update and delete of a demo record through `props.resources`;
  - a 422 shown with `fieldErrors`;
  - a toast;
  - a DropdownMenu and Tooltip, also inside a Dialog, at their computed
    positions;
  - no CSP violation.
- SDK copies kept equal and the demo rebuilt (`just generated`).

### Docs B
- ADR 0032 amended:
  - the additive `ui` and the SDK level;
  - the `ui` 2 list;
  - `props.resources` and `props.toast`;
  - path validation for the resources facade;
  - the overlay container.
- Guide `admin/react-pages.md`:
  - resources: update semantics, field errors, paused writes;
  - toast;
  - the new components with examples;
  - the Tabs-with-sub-path recipe;
  - upgrading a page to `ui` 2.
- CHANGELOG, release notes, upgrade guide (0.6.0: `ui` 1 bundles keep
  working; to use `ui` 2, run `make:sdk`, set `ui: 2`, and rebuild).

## Out of scope
- Panel-rendered record forms inside React pages (`<ResourceForm>`).
- Generic optimistic concurrency for records.
- Custom sidebar sections or external links.
- A home page that also keeps the built-in dashboard widgets.
