# React pages

A page, a slot or a cell can be a React component that renders **inside the
panel**: on its React, its React Query client, its UI kit, theme, language and
router. This is the extension host API v2. Plain scripts
([Customizing the panel](customizing.md#script-pages), v1) keep working.

## Creating one

```sh
rustcms make:page StatsPage --react
cd resources/admin && npm install && npm run build   # Node 20.19+ or 22.12+
```

`make:page --react` writes:

- `src/admin/pages/stats_page.rs`, a `PanelPage` with
  `PageView::Script("<app>/stats")`;
- `resources/admin/src/<app>/stats.tsx` and `stats.css` (your sources);
- placeholders for the built `resources/admin/js/<app>/stats.js` and
  `css/<app>/stats.css`;
- in the admin provider:

  ```rust
  .extension_script_v2("stats", include_str!("../../resources/admin/js/blog/stats.js"),
      Requires { react: "19.3", react_query: "5.104", ui: 2 })
  .extension_style("stats", include_str!("../../resources/admin/css/blog/stats.css"))
  ```

- the first time only: `resources/admin/package.json`, `tsconfig.json`,
  `.gitignore` and the SDK in `resources/admin/sdk`. Existing files are never
  overwritten.

**Commit the built `js/` and `css/` files and `package-lock.json`.** The Rust
build reads them with `include_str!`, so `cargo build` and deployments need no
Node. Afterwards, `npm ci && npm run build` rebuilds every
`src/<vendor>/<name>.tsx`; a second page is a second file.

Until it is built, the page shows "unavailable"; the app still compiles.

## The component

```tsx
// resources/admin/src/blog/stats.tsx
import { type PageProps, registerPage, useAdminMutation, useAdminQuery, useEvents } from "@rustcms/admin-sdk";
import { Button, Card, CardContent, Dialog } from "@rustcms/admin-sdk/ui";
import "./stats.css";

type Data = { total: number };

function Stats(props: PageProps<Data>) {
  const posts = useAdminQuery<Post[]>(props, "/blog/posts", { query: { status: "draft" } });
  const publish = useAdminMutation(props, "/blog/publish", "post");
  return (
    <Card>
      <CardContent className="stats-body">
        <p>{props.t("Total")}: {props.data.total}</p>
        {props.can("blog.publish") && <Button onClick={() => publish.mutate({ all: true })}>{props.t("Publish")}</Button>}
      </CardContent>
    </Card>
  );
}

registerPage("blog/stats", Stats);
```

Register with `registerPage`, `registerSlot` or `registerCell` and the name of
the Rust side (`PageView::Script("…")` of a page or slot, `.cell("…")` of a
column). Within each kind (page, slot or cell) a name is taken once, by v1
or v2.

These imports use the panel's own modules, except `@rustcms/admin-sdk`,
whose hooks and registration code are bundled into your script. Other installed
dependencies and your local modules are bundled into the script; other
React, React DOM and React Query subpaths (`react-dom/client`,
`react/jsx-dev-runtime`) and every dynamic `import()` fail the build:

| Import | What |
|---|---|
| `react`, `react/jsx-runtime` | the panel's React |
| `react-dom` | `createPortal` |
| `@tanstack/react-query` | `useQuery`, `useMutation`, `useQueryClient`, `useInfiniteQuery` |
| `@rustcms/admin-sdk` | `registerPage/Slot/Cell`, the hooks below, the prop types |
| `@rustcms/admin-sdk/ui` | `Alert`, `Badge`, `Button`, `buttonVariants`, `Card`, `CardContent`, `CardHeader`, `CardTitle`, `Checkbox`, `Switch`, `Dialog`, `Input`, `Label`, `NativeSelect`, `Textarea`, `Page`, `PageHeader`, `Loading`, `ErrorState`, `cn`; and from `ui` 2: `Tabs*`, `DropdownMenu*`, `Tooltip`, `Table*`, `Skeleton`, `Separator`, `CodeBlock` ([below](#components)) |
| `@rustcms/admin-sdk/host` | `ApiError`, `versions` |

Never call `createRoot`: the panel renders your component.

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
  ([Languages](i18n.md)).
- `can(permission)` is true when the user holds that admin permission. It
  only hides buttons: the server checks every request.
- `navigate("/p/stats/items/2")` opens a panel path.

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
  { reconnect })` open a [stream](realtime.md) while mounted.
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
apply:

```tsx
import { fieldErrors, useResource, useResourceMutation } from "@rustcms/admin-sdk";

const services = useResource<Service>(props, "services", { query: { search: "web", per_page: "25" } });
const restart = useResourceMutation(props, "services", "action", {
  onSuccess: () => props.toast.success(props.t("Restarted")),
});
const save = useResourceMutation(props, "services", "update");
// services.data?.data: envelopes { key, title, abilities, data, labels }
restart.mutate({ name: "restart", keys: [service.key] });
save.mutate({ key: service.key, data: { replicas: "3" } });
const errors = fieldErrors(save.error); // { replicas: ["must be at most 10"] }
```

- **Methods:** `list`, `get`, `create`, `update`, `delete`, `restore`,
  `forceDelete`, `action`. The hooks `useResource`, `useRecord` and
  `useResourceMutation` wrap them.
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

## Toasts

`props.toast.success(text)` and `props.toast.error(text)` (`ui` 2) show a
message in the panel's corner:

- a success for 5 s, an error for 8 s; at most 5 at a time;
- announced to screen readers;
- kept when the user leaves the page, dropped when the session changes.

## Page titles

`props.title(text)` (`ui` 2, pages) names the current sub-path in the
panel's heading and breadcrumb; `props.title(null)` hides the heading (it
stays for screen readers) when the page draws its own:

```tsx
useEffect(() => {
  if (props.path.startsWith("services/")) props.title(props.path.slice("services/".length));
}, [props.path, props.title]);
```

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
page's CSS file (imported by the TSX), with the theme's variables so they
follow light and dark mode: `--background`, `--foreground`, `--card`,
`--muted`, `--muted-foreground`, `--primary`, `--primary-foreground`,
`--secondary`, `--border`, `--ring`, `--success`, `--warning`,
`--destructive`, `--radius`.

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
- A bundle built with the `ui` 2 SDK whose `Requires` still says `ui: 1` is
  refused at registration, with a message: update the provider.
- `AdminProvider::extension_timeout(Duration)` (1–60 s, default 10) bounds how
  long the panel waits for scripts to register.

## Testing

`npm run typecheck` checks the sources against the SDK's types. Test the
Rust side like any page (`app.get("/admin/api/pages/stats/items/2")`);
end-to-end tests drive the built page in a browser.

## Upgrading the SDK

`rustcms upgrade` warns when `resources/admin/sdk` is older than the command.
Run `rustcms make:sdk`: it replaces the files you did not edit and lists the
ones it kept, and the `extension_script_v2` registrations whose `Requires`
declare an older `ui`: set `ui: 2` there. Then update `package.json` to the
versions in `Requires`, run `npm install && npm run build`, and commit.
