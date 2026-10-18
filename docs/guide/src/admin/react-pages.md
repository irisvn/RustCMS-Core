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

  ```rust,ignore
  .extension_script_v2("stats", include_str!("../../resources/admin/js/blog/stats.js"),
      Requires { react: "19.3", react_query: "5.104", ui: 1 })
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
column). A
name is taken once, by v1 or v2.

Only these imports are available; anything else (`react-dom/client`,
`react/jsx-dev-runtime`, `import()`) fails the build:

| Import | What |
|---|---|
| `react`, `react/jsx-runtime` | the panel's React |
| `react-dom` | `createPortal` |
| `@tanstack/react-query` | `useQuery`, `useMutation`, `useQueryClient`, `useInfiniteQuery` |
| `@rustcms/admin-sdk` | `registerPage/Slot/Cell`, the hooks below, the prop types |
| `@rustcms/admin-sdk/ui` | `Alert`, `Badge`, `Button`, `buttonVariants`, `Card`, `CardContent`, `CardHeader`, `CardTitle`, `Checkbox`, `Switch`, `Dialog`, `Input`, `Label`, `NativeSelect`, `Textarea`, `Page`, `PageHeader`, `Loading`, `ErrorState`, `cn` |
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

- `t(text)` translates with the catalogs of `registerTranslations`
  ([Languages](i18n.md)).
- `can(permission)` is true when the user holds that admin permission. It
  only hides buttons: the server checks every request.
- `navigate("/p/stats/items/2")` opens a panel path.

## Calling your routes

`props.api.get/post/put/patch/delete(path, body?, { query })` call your
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

When the session changes (sign-in, sign-out, another tab), the view is
mounted again for the new session; calls and callbacks of the old one do
nothing (a call rejects with `ApiError` code `session_changed`). A refused
CSRF token also ends the session state, so that call rejects the same way.

## Sub-pages

`/admin/p/<slug>/<sub/path>` opens the same page: `props.path` is
`"sub/path"` (`""` without one), and `PanelPage::data` reads
`ctx.page_path()`. The page's permissions cover every sub-path; check more in
`data` when some need it. Segments are `A-Z a-z 0-9 . _ ~ -`, at most 16;
anything else is "not found" in the panel and a 422 validation error on
`path` from the API.

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
  built against. A panel with another major, an older minor or another `ui`
  does not load the script at all, and the console says why: rebuild with the
  SDK of that RustCMS version.
- `AdminProvider::extension_timeout(Duration)` (1–60 s, default 10) bounds how
  long the panel waits for scripts to register.

## Testing

`npm run typecheck` checks the sources against the SDK's types. Test the
Rust side like any page (`app.get("/admin/api/pages/stats/items/2")`);
end-to-end tests drive the built page in a browser.

## Upgrading the SDK

`rustcms upgrade` warns when `resources/admin/sdk` is older than the command.
Run `rustcms make:sdk`: it replaces the files you did not edit and lists the
ones it kept. Then update `package.json` to the versions in `Requires`,
`npm install && npm run build`, and commit.
