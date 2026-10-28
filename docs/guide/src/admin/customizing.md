# Customizing the panel

Beyond resources and settings, an application can add its own **pages**, put
content **beside records** (slots), draw **table cells** its own way and
change the panel's look ([theme and branding](theming.md)). Pages and slots
are server templates (HTML, like Blade views in Filament) or are drawn by
extension scripts. Use them for reports, overviews and tools that are not a
list of records. The panel's Content Security Policy stays as strict as
before: no inline script, no inline style, nothing from other sites.

![A custom page drawn from a server template](/img/admin/custom-page.webp)
*A RustCMS demo application: the Post stats page.*

Files live in `resources/` ([Directory structure](../getting-started/directory-structure.md)):

```text
resources/
├── views/admin/*.html          templates of pages and slots
└── admin/
    ├── js/<vendor>/*.js        extension scripts (widgets, script pages, cells; built React pages)
    ├── css/<vendor>/*.css      extension stylesheets
    ├── src/<vendor>/*.tsx      React page sources (make:page --react)
    ├── sdk/, package.json, package-lock.json, .rustcms-build.json
    │                           the React build setup (React pages only)
    └── logo.png, favicon.png
```

## Quick start

```sh
rustcms make:page ReportPage            # a template page
rustcms make:page CalendarPage --script # a page drawn by a v1 script
rustcms make:page StatsPage --react     # a React page (see React pages)
rustcms permission:sync                 # creates the page's permission
```

`make:page ReportPage` writes `src/admin/pages/report_page.rs` and
`resources/views/admin/report.html`, and registers
`.page::<…ReportPage>()` in the admin provider. The page declares the
permission `report.view` (`declare_permission`), so `permission:sync`
creates it; grant it through a role. The page opens at `/admin/p/report`.
With `--script`, the script is named `<vendor>/<slug>`, where the vendor is
your package name with `_` replaced by `-`, and the provider gets its
`extension_script` line.

## Theme

The theme (colors, radius, font, logo, favicon, light and dark) is set with
`AdminProvider::theme(Theme::new()…)`. See
[Theming and branding](theming.md#theme).

## Runtime branding

A `BrandingSource` changes the brand name, logos and colors while the
application runs (`AdminProvider::branding`). See
[Theming and branding](theming.md#runtime-branding).

## Custom pages

A page is a `PanelPage`: a slug, a place in the navigation, who may open it,
a view and the data the view gets.

<<< @/../../../examples/demo/src/admin/pages/post_stats_page.rs#page [src/admin/pages/post_stats_page.rs]

<<< @/../../../examples/demo/resources/views/admin/post_stats.html#template [resources/views/admin/post_stats.html]

Register it with `.page::<PostStatsPage>()` on the admin provider. A page's
place in the sidebar follows `PanelPageMeta::group` and
[`sort`](overview.md#order); `AdminProvider::home(slug)` makes one the
[home page](overview.md#home-page).

| `PanelPageMeta` method | Effect |
|---|---|
| `new(label)` | the title and navigation label (translated) |
| `icon(name)`, `group(label)`, `sort(n)` | the navigation entry |
| `permission(name)` | required; may be called several times (all required) |
| `permission_any([…])` | at least one of the group is required |
| `declare_permission(name)` | `permission(name)`, also declared for `permission:sync` |

### Who sees it

- A page needs every `.permission(…)` and at least one of each
  `.permission_any([…])` group. It must have at least one of either, and an
  empty group stops the boot.
- `permission_any` suits a page that gathers several sections, for example
  a settings page for whoever manages any of them:
  `.permission_any(["storage_providers.view_any", "notification_channels.view_any"])`.
  The page's `data` and its [admin routes](admin-routes.md) still check
  each section.
- The navigation lists the pages a user may open. A user who may open no
  page gets 403 for any slug; otherwise an unknown slug is 404 and a page
  they may not open 403. `data` runs only for allowed users.
- Slugs are 1–64 of `a-z`, `0-9`, `-`. A slug used twice stops the boot.

### Its data

`data` is application code, like a controller:

- Read records with `ctx.records(slug, &query)` and `ctx.record(slug, key)`,
  which apply the resource's `scope()`, policy and field read rules.
- Anything else you return (counts, joins), make sure the user may see it.
- Values keep the [wire format](../reference/wire-format.md) (64-bit
  integers as strings). At most 1 MiB. Without a `data` method the view
  gets `null`.
- `ctx.page_path()` is the [sub-path](react-pages.md#sub-pages) of
  `/admin/p/<slug>/<sub/path>`.

### Templates

Templates use [minijinja](https://docs.rs/minijinja) (Jinja syntax). The
context holds:

| Name | |
|---|---|
| `data` | what `data` returned |
| `locale` | the user's language |
| `user.id` | the signed-in user's id |
| `t("text")` | your catalogs, like `ctx.t` |
| `can("permission")` | whether the user holds an admin permission |
| `admin_url("path")` | a link in the panel |

Rules:

- **HTML escaping is always on and cannot be turned off.** `safe`, `tojson`
  and the `autoescape` statement are not available, however they are spaced.
  There is no way to mark template output as safe HTML.
- One template per page: no `include`, `import`, `from` or `extends`.
  Templates are compiled at boot; an error stops it.
- Work is bounded, and output stops at 512 KiB (the request fails with 500;
  nothing larger is held in memory). Templates are application code, like
  `data`: the bounds catch mistakes over large data, they do not make a
  hostile template safe.
- **Quote every attribute value.**

**Then the HTML is cleaned** on the server by an allowlist:

- kept: structure, text, lists, tables, links, images, `details`,
  `progress`, `meter`, `time`; attributes `class`, `title`, `role`,
  `aria-*`, `data-*` and a few per element;
- removed: scripts, styles and `style=`, event handlers, forms and inputs,
  frames, `svg`, `id`, `name` and comments;
- links must stay in the panel (`/admin/…`, not its API or assets) or be
  `https:` / `mailto:`; images must come from the panel (media files,
  assets). Links to the panel navigate without reloading.

**Classes for templates** (the panel's utility classes are not available to
your HTML): `rc-stack`, `rc-grid`, `rc-card`, `rc-title`, `rc-muted`,
`rc-table`, `rc-badge`, `rc-link`, `rc-meter`, styled with the theme. Add
your own with `extension_style`.

The page is rendered for each request, in the user's language: nothing is
cached but the compiled template.

### Editing templates

`view!("file.html")` embeds `resources/views/admin/file.html` in the binary,
so a release build never reads the file. In a debug build with
`APP_ENV=local` (`cargo run -- serve` on your machine), the file is read
again at each request: save it and reload the page, no rebuild. A file read
this way is checked as at boot; an error answers 500 and is logged (fix the
file and reload). If the file is missing, the embedded copy is used.
`lang:missing` reads the copy embedded at build time.
`PageView::Template("…")` takes the text directly and is never read again.

### Script pages

![A script page reading a live counter](/img/admin/script-page.webp)
*A RustCMS demo application: the Live page, drawn by a v1 script.*

Two kinds of scripts exist:

- **v1** (`extension_script`, below): `mount(element, ctx)` gets an element
  and builds DOM by hand. No build step.
- **v2** ([React pages](react-pages.md), `extension_script_v2`): a React
  component rendered by the panel, with its UI kit, React Query, the API
  client, sub-pages and `can()`. Built with npm, committed.

```rust
fn view() -> PageView { PageView::Script("lms/calendar") }    // `rustcms make:page CalendarPage --script`
```

```js
// resources/admin/js/lms/calendar.js, loaded with .extension_script("calendar", include_str!(…))
window.rustcms.registerPage("lms/calendar", {
  apiVersion: 1,
  mount(element, ctx) {
    // ctx: { data, locale, t(text), navigate(path), reload(), realtime: { events, socket } }
    const list = document.createElement("ul");
    element.append(list);
    const render = (current) => { list.replaceChildren(/* … from current.data … */); };
    render(ctx);
    return { update: render, unmount: () => element.replaceChildren() };
  },
});
```

- The script gets the page's `data` (a copy), is told about changes
  (language) with `update`, and is unmounted when the user leaves.
- `navigate` opens a page of the panel; `reload` reads the page again;
  `realtime.events` and `realtime.socket` open [live streams](realtime.md).
  They do nothing once the view is unmounted or the session changed
  (sign-in, sign-out, credentials rotated in any tab): the view is mounted
  again for the new session.
- If the page can no longer be read (403, 404), the view is unmounted and
  the error shown.
- Scripts register while the panel loads (names `vendor/name`, each part
  1–32 lowercase letters, digits or `-`). A script that is missing, throws or
  does not return `{ update, unmount }` shows a notice instead of the page
  (what it drew is removed).
- **Scripts run with the panel's full privileges** (the same origin, the
  user's session): build elements rather than writing `innerHTML` with data,
  and write through resources, actions and [admin routes](admin-routes.md).

The demo's Live page is a complete v1 script page:
`examples/demo/resources/admin/js/demo/live.js` (see [Realtime](realtime.md)).

## Slots beside records

A slot adds a card to a record's view page. The LMS shows the seats of a
course beside it:

<<< @/../../../examples/lms/src/admin/slots/course_seats_slot.rs#slot [src/admin/slots/course_seats_slot.rs]

<<< @/../../../examples/lms/resources/views/admin/course_seats.html#template [resources/views/admin/course_seats.html]

Register it with `.slot::<CourseSeatsSlot>()`.

![A slot beside a course: active, withdrawn and average progress](/img/admin/slot.webp)
*A RustCMS LMS example: the Seats slot on a course page.*

- `Placement::RecordAside("courses")` names the resource; an unknown
  resource stops the boot. Names are 1–64 of `a-z`, `0-9`, `-`, unique.
- `SlotMeta::new()` gives a card without a heading; `.label("Seats")` titles
  it. A slot needs at least one `.permission(…)` (all are required; there is
  no `permission_any`).
- The record is loaded as the view page loads it (missing or out of scope:
  404; denied: 403), and `data` gets it with readable fields only. A
  resource whose policy admits by permission (the default policy) answers
  403 to a user without its `view` before any parameter is checked.
- Slots whose permissions the user lacks are left out before their data is
  read. A slot that fails to load again (no longer allowed) disappears.
- Templates and scripts work as for pages (`registerSlot` for scripts;
  `ctx.record` is the record). Dashboard and header placements are planned.

## Custom cells

A cell script draws a table column's value. The LMS draws enrollment
progress as a bar:

<<< @/../../../examples/lms/src/admin/resources/enrollment_resource.rs#table [src/admin/resources/enrollment_resource.rs]

<<< @/../../../examples/lms/resources/admin/js/lms/progress.js#cell [resources/admin/js/lms/progress.js]

<<< @/../../../examples/lms/src/providers/admin_panel_provider.rs#cell-extension [src/providers/admin_panel_provider.rs]

![The enrollments table with progress bars](/img/admin/custom-cell.webp)
*A RustCMS LMS example: the `lms/progress` cell.*

- `TableColumn::cell("vendor/name")` names the script; `.props(json!(…))`
  passes settings (at most 4 KiB), read as `ctx.props`.
- The cell's `ctx` holds `data` (the value), `record`, `props`, `locale` and
  `t`.
- The cell draws the value the user may read (the column only exists for
  readable fields). Until a script registers the name, or after it throws,
  the column is drawn by its type.
- No inline styles: use attributes (`meter`, `progress`) or classes from an
  extension stylesheet.

## Translations

`rustcms lang:missing vi` lists page labels and groups, slot labels and every
call of `t` with a string literal in your templates' code
(<code v-pre>{{ t("Seats") }}</code>, `t( 'It\'s' )`, inside `{% %}` too; not
text outside <code v-pre>{{ }}</code> / `{% %}` nor comments). Text computed
at render (`t(name)`) is not found: add it to the catalog yourself. Script
text uses `window.rustcms.registerTranslations`
([Languages](i18n.md#extensions)).

## Security notes

- Template output is always escaped, then sanitized on the server. Nothing
  marks template output as safe, and the CSP is never loosened for a page.
- `data` and slot `data` are your code: `ctx.records` and `ctx.record`
  enforce `scope()`, the policy and field rules; anything else you read,
  authorize yourself ([Authorization](authorization.md)).
- Scripts are trusted application code with the user's session: review them
  like server code.

## Pitfalls

- A page or slot without a permission, a duplicate slug or name, or a
  template that does not compile stops the boot.
- An unquoted attribute value in a template can be split by the data it
  holds: quote every one.
- The page's permission covers all its sub-paths; check more in `data` when
  some need it.

## Related pages

- [Theming and branding](theming.md)
- [React pages](react-pages.md)
- [Admin routes](admin-routes.md): JSON endpoints for your pages
- [Custom fields](custom-fields.md): form widgets
- [Overview](overview.md): navigation and the home page
