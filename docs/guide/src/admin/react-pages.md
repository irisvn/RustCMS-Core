# React pages

A page, a slot or a cell can be a React component that renders **inside the
panel**: on its React, its React Query client, its UI kit, theme, language and
router. This is the extension host API v2. Use it for interactive tools
(dashboards of a service, editors, multi-step flows) that a server template
cannot draw. Plain scripts ([Customizing the panel](customizing.md#script-pages),
v1) keep working.

![A React page with tabs, a tag list and toasts](/img/admin/react-page.webp)
*A RustCMS demo application: the React demo page.*

## Quick start {#creating-one}

```sh
rustcms make:page StatsPage --react
cd resources/admin && npm install && npm run build   # Node ^20.19.0 or >=22.12.0
rustcms permission:sync
```

`make:page StatsPage --react` (`--react` and `--script` exclude each other)
writes, for an application whose package is `blog`:

- `src/admin/pages/stats_page.rs`, a `PanelPage` with
  `PageView::Script("blog/stats")` and the permission `stats.view`;
- `resources/admin/src/blog/stats.tsx` and `stats.css` (your sources);
- placeholders for the built `resources/admin/js/blog/stats.js` and
  `css/blog/stats.css`, so the application compiles before the first build;
- in the admin provider:

  ```rust
  .extension_script_v2("stats", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/resources/admin/js/blog/stats.js")), rustcms::admin::Requires { react: "19.3", react_query: "5.104", ui: 2 })
  .extension_style("stats", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/resources/admin/css/blog/stats.css")))
  ```

- the first time only: `resources/admin/package.json`, `tsconfig.json`,
  `.gitignore` and the SDK in `resources/admin/sdk`. Existing files are never
  overwritten.

The vendor (`blog`) is your package name with `_` replaced by `-`. The
generated component:

```tsx
// resources/admin/src/blog/stats.tsx
import { type PageProps, registerPage, useAdminQuery } from "@rustcms/admin-sdk";
import { Card, CardContent } from "@rustcms/admin-sdk/ui";
import "./stats.css";

type Data = { user: string };

function StatsPage(props: PageProps<Data>) {
  // Your `AdminRoutes` under `{admin}/api/x` (e.g. `.get("/stats", …)`):
  // const items = useAdminQuery<Item[]>(props, "/stats");
  void useAdminQuery;
  return (
    <Card>
      <CardContent className="stats-body">
        <p>
          {props.t("Signed in as user")} {props.data.user}
        </p>
        {props.path && <p>{props.path}</p>}
      </CardContent>
    </Card>
  );
}

registerPage("blog/stats", StatsPage);
```

Until it is built, the page shows "unavailable"; the application still
compiles.

::: warning Commit the build
Commit the built `js/` and `css/` files, `package-lock.json` and
`.rustcms-build.json`. The Rust build reads the built files with
`include_str!`, so `cargo build` and deployments need no Node.
:::

`npm run build` (the SDK's `rustcms-build`) builds every
`src/<vendor>/<name>.tsx` into `js/<vendor>/<name>.js` and
`css/<vendor>/<name>.css`. An entry without styles gets an empty CSS file,
so `include_str!` always finds one. `.rustcms-build.json` lists what the
build wrote, so the outputs of a removed entry are deleted at the next build
(never hand-written scripts beside them). A second page is a second file.
The build uses the SDK's Vite preset, `rustcmsExtension({ entry, root })`
from `@rustcms/admin-sdk/vite`.

## Full example

The demo's React page reads a sub-path in `data`:

<<< @/../../../examples/demo/src/admin/pages/react_page.rs#react-page [src/admin/pages/react_page.rs]

and registers the built script:

<<< @/../../../examples/demo/src/providers/admin_panel_provider.rs#react-extension [src/providers/admin_panel_provider.rs]

Its tag list reads and writes the `tags` resource with the panel's rules
(policy, validation, audit), and shows toasts:

<<< @/../../../examples/demo/resources/admin/src/demo/react.tsx#tags [resources/admin/src/demo/react.tsx]

A live counter from an [event stream](realtime.md):

<<< @/../../../examples/demo/resources/admin/src/demo/react.tsx#use-events [resources/admin/src/demo/react.tsx]

## The component

Register with `registerPage`, `registerSlot` or `registerCell` and the name of
the Rust side (`PageView::Script("…")` of a page or slot, `.cell("…")` of a
column). Within each kind (page, slot or cell) a name is taken once, by v1
or v2.

**Never call `createRoot`**: the panel renders your component.

The panel's own modules are reached only through the imports the SDK's preset
maps. `@rustcms/admin-sdk` itself (hooks and registration code) is bundled
into your script, and so are other installed dependencies and your local
modules. Other React, React DOM and React Query subpaths (`react-dom/client`,
`react/jsx-dev-runtime`) and every dynamic `import()` fail the build.

| Import | What |
|---|---|
| `react`, `react/jsx-runtime` | the panel's React |
| `react-dom` | `createPortal` |
| `@tanstack/react-query` | `useQuery`, `useMutation`, `useQueryClient`, `useInfiniteQuery` |
| `@rustcms/admin-sdk` | `registerPage/Slot/Cell`, the hooks below, the prop types |
| `@rustcms/admin-sdk/ui` | `Alert`, `Badge`, `Button`, `buttonVariants`, `Card`, `CardContent`, `CardHeader`, `CardTitle`, `Checkbox`, `Switch`, `Dialog`, `Input`, `Label`, `NativeSelect`, `Textarea`, `Page`, `PageHeader`, `Loading`, `ErrorState`, `cn`; and from `ui` 2: `Tabs*`, `DropdownMenu*`, `Tooltip`, `Table*`, `Skeleton`, `Separator`, `CodeBlock` ([below](#components)) |
| `@rustcms/admin-sdk/host` | `ApiError` (`status`, `code`, `fieldErrors`, `formErrors`, `maybeApplied`), `versions` (`{api, react, reactQuery, ui}`) |

## Props

| | Page | Slot | Cell |
|---|---|---|---|
| `data` (frozen copy of `data()`) | ✓ | ✓ | `value` |
| `record` | | ✓ | ✓ |
| `slug`, `path`, `search` | ✓ | | |
| `slot`, `resource`, `recordKey` | | ✓ | |
| `props` (the cell's) | | | ✓ |
| `locale`, `t`, `scope`, `can` | ✓ | ✓ | ✓ |
| `navigate`, `reload`, `api`, `realtime` | ✓ | ✓ | |
| `resources`, `toast` (`ui` 2) | ✓ | ✓ | |
| `title` (`ui` 2) | ✓ | | |

- `t(text)` translates with the catalogs of `registerTranslations`
  ([Languages](i18n.md#extensions)).
- `can(permission)` is true when the user holds that admin permission. It
  only hides buttons: the server checks every request.
- `navigate("/p/stats/items/2")` opens a panel path.
- Cells are drawn once per row of a list: they get no API client, streams or
  navigation, only the value, the record and their props. Fetch what a whole
  list needs in a page.

## Calling your routes

`props.api.get/delete(path, { query }?)` and
`props.api.post/put/patch(path, body?, { query }?)` call your
[admin routes](admin-routes.md) under `{admin}/api/x`, with the session and
CSRF token. Paths start with one `/` (`/blog/posts`); `..`, `//`, `?`, `#`,
encoded slashes and double encoding are refused before anything is sent.
Errors are `ApiError` (`status`, `code`).

Hooks wrap them with React Query:

- `useAdminQuery(props, path, { query, ...options })`: cached per view, user
  and language.
- `useAdminMutation(props, path, method)`: on success, refetches the view's
  queries. Writes are never retried automatically: if the outcome of a write
  is unknown (the connection dropped), the panel's "paused" banner asks the
  user, and the view's next writes wait.
- `useEvents(props, path, handlers)` and `useSocket(props, path, handlers,
  { reconnect })` open a [stream or socket](realtime.md) while mounted.
  `useSocket` returns `{ send }`.
- `useColorScheme()` returns `"light"` or `"dark"` and re-renders when the
  user or the system changes it, e.g. to redraw a canvas chart. CSS needs
  nothing: the theme's variables follow it. It reads the panel's
  `data-scheme`, so it works on any 0.6 panel.

When the session changes (sign-in, sign-out, another tab), the view is
mounted again for the new session; calls and callbacks of the old one do
nothing (a call rejects with `ApiError` code `session_changed`). A refused
CSRF token also ends the session state, so that call rejects the same way.
A **read** (`api.get`, `resources.list`/`get`) answered after its view was
mounted again in the same session (a sub-path navigation) is delivered: its
query cache entry, shared with the new view, gets the data instead of an
error. Writes answered late still reject.

## Records of your resources

With `ui: 2` in `Requires`, `props.resources` reads and writes the records of
your [resources](resources.md) through the panel's own routes, so their
policy, `scope()`, field rules, validation, audit log and encrypted fields
apply. The demo's `Tags` component above uses it.

| Hook / method | |
|---|---|
| `useResource(props, slug, { query, ...options })` | a page of records: `{ data: [Envelope], meta: { page, perPage, total, lastPage } }` |
| `useRecord(props, slug, key, options?)` | one record |
| `useResourceMutation(props, slug, op, options?)` | `op`: `create` `{data}`, `update` `{key, data}`, `delete`, `restore`, `forceDelete` `{key}`, `action` `{name, keys, data?}` |
| `fieldErrors(error)` | the field errors of a refused write (422): `{ field: [messages] }` |
| `props.resources.list/get/create/update/delete/restore/forceDelete/action` | the same without React Query |
| `props.resources.track(slug)` | marks a resource as used by the view (its paused-write banner shows there; the hooks do it) |

An `Envelope` is `{ key, title?, abilities, data, trashed?, labels? }`.

- **Values** use the panel's wire format: 64-bit integers and decimals as
  strings. `props.resources` sends values unchanged: send `null` to clear a
  field (or to leave a `required` one empty, which it refuses); an empty
  string stays an empty string.
- **Updates** send the changed fields, and the last write wins. Encrypted
  maps answer `409 secret_stale` when they changed meanwhile. On a 409 or
  422, keep the user's draft.
- **Lost answers:** a write whose answer was lost pauses that resource's
  writes, in your view and in the resource's own pages. Both show the
  banner; creates and actions can be retried safely there. Writes are never
  retried automatically.
- **Refreshing:** after any write to a resource, every view reading it
  refetches, the built-in lists included.
- **Validation:** slugs, action names and keys a URL could change (`.`,
  `..`) are refused before anything is sent.

Lower-level helpers, for your own React Query calls: `adminKey(props, path,
query)` and `resourceKey(props, slug, …)` (the query keys the hooks use),
`queryPairs(query)` and `SDK_UI` (the `ui` level the SDK is built for, 2).

## Toasts

`props.toast.success(text)` and `props.toast.error(text)` (`ui` 2) show a
message in the panel's corner:

- a success for 5 s, an error for 8 s; at most 5 at a time (the oldest goes
  first); text is cut at 500 characters;
- announced to screen readers;
- kept when the user leaves the page, dropped when the session changes.

## Page titles

`props.title(text)` (`ui` 2, pages) names the current sub-path in the
panel's heading and breadcrumb; `props.title(null)` hides the heading (it
stays for screen readers) when the page draws its own:

<<< @/../../../examples/demo/resources/admin/src/demo/react.tsx#title [resources/admin/src/demo/react.tsx]

A title holds for the sub-path it was set on: on another one the page's own
title shows again until it is set there too. It ends with the view and with
the session.

## Components

`ui` 2 adds:

- `Tabs` (controlled): with sub-paths,
  `<Tabs value={props.path || "overview"} onValueChange={(v) => props.navigate("/p/x/" + v)}>`,
  then `TabsList aria-label=…`, `TabsTrigger value=…` and
  `TabsContent value=…`.
- `DropdownMenu trigger="Actions"` with `DropdownMenuItem onSelect destructive
  disabled`, `DropdownMenuSeparator` and `DropdownMenuLabel`. The keyboard
  works, and Escape returns to the button.
- `Tooltip content="…"` around one element.
- `Table`, `TableHeader`, `TableBody`, `TableRow`, `TableHead` and
  `TableCell`, styled like the panel's lists.
- `Skeleton`, `Separator`, and `CodeBlock code="…"` (with a copy button).

Menus and tooltips work inside a `Dialog`. Every `Dialog` has a close button
(×, "Close") in its top right corner, after your content, so opening still
focuses your first field; `closeButton={false}` leaves it out, and
`closeDisabled` disables it and Escape (for example while saving).

## Sub-pages

`/admin/p/<slug>/<sub/path>` opens the same page: `props.path` is
`"sub/path"` (`""` without one), and `PanelPage::data` reads
`ctx.page_path()`. The page's permissions cover every sub-path; check more in
`data` when some need it. A sub-path is up to 16 non-empty segments of
`A-Z a-z 0-9 . _ ~ -` (`.` and `..` are refused), at most 512 bytes as sent
(one trailing slash is dropped). Anything else is "not found" in the panel
and a 422 validation error on `path` from the API, after the permission
checks.

## Styles

The panel's stylesheet holds only the classes the panel uses. Put yours in the
page's CSS file (imported by the TSX), with the theme's
[CSS variables](theming.md#css-variables) so they follow light and dark mode:
`--background`, `--foreground`, `--card`, `--muted`, `--muted-foreground`,
`--primary`, `--primary-foreground`, `--secondary`, `--border`, `--ring`,
`--success`, `--warning`, `--destructive`, `--radius`.

```css
.stats-body { padding-top: 1.25rem; color: var(--muted-foreground); }
```

The `ui` components carry their own styles. Styles are never injected at run
time, so the panel's Content Security Policy stays strict.

## Errors and compatibility

- A component that throws while rendering shows "unavailable" with Retry;
  other views are unaffected (a cell shows its plain value).
- `Requires` states the React, React Query and `ui` versions the bundle was
  built against. A panel with another major, an older minor or a lower `ui`
  does not load the script at all, and the console says why: rebuild with the
  SDK of that RustCMS version. A `ui: 1` bundle keeps working on newer
  panels.
- A bundle built with the `ui` 2 SDK refuses to register on a panel below
  `ui` 2, and a bundle whose `Requires` still says `ui: 1` is refused at
  registration, with a message: update the provider.
- `AdminProvider::extension_timeout(Duration)` (1–60 s, default 10) bounds how
  long the panel waits for scripts to register.

## Testing

`npm run typecheck` checks the sources against the SDK's types. Test the
Rust side like any page (`app.get("/admin/api/pages/stats/items/2")`);
end-to-end tests drive the built page in a browser.

## Upgrading the SDK

`rustcms upgrade` warns when the version of `resources/admin/sdk` differs from
the one the command carries (older or newer). Run `rustcms make:sdk`: it
replaces the files you did not edit and lists the ones it kept, and the
`extension_script_v2` registrations whose `Requires` declare an older `ui`:
set `ui: 2` there. Then update `package.json` to the versions in `Requires`,
run `npm install && npm run build`, and commit.

## Security notes

- Every request goes through the server's checks: `can()` only hides
  buttons, and `props.resources` uses the panel's routes, so `scope()`, the
  policy and field rules apply ([Authorization](authorization.md)).
- Extensions run with the user's session; ship only code you review. The
  panel's CSP is not loosened for them.

## Related pages

- [Customizing the panel](customizing.md): pages, slots, cells, v1 scripts
- [Admin routes](admin-routes.md): the JSON endpoints your page calls
- [Realtime](realtime.md): `useEvents`, `useSocket`
- [Theming and branding](theming.md#css-variables)
