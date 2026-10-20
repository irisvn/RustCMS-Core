# Customizing the panel

Beyond resources and settings, an application can change how the panel
looks (**theme**), add its own **pages**, put content **beside records**
(slots) and draw **table cells** its own way. Pages and slots are written as
server templates (HTML, like Blade views in Filament) or drawn by extension
scripts. The panel's Content Security Policy stays as strict as before: no
inline script, no inline style, nothing from other sites.

Files live in `resources/` ([Directory structure](../getting-started/directory-structure.md)):

```text
resources/
├── views/admin/*.html       templates of pages and slots
└── admin/
    ├── js/<vendor>/*.js     extension scripts (widgets, script pages, cells)
    ├── css/<vendor>/*.css   extension stylesheets
    └── logo.png, favicon.png
```

## Theme

```rust,ignore
// src/providers/admin_panel_provider.rs
AdminProvider::new().theme(
    Theme::new()
        .primary("#2563eb")
        .primary_foreground("#ffffff")
        .dark(|dark| dark.primary("#60a5fa"))
        .radius("0.625rem")
        .font_family("Inter, system-ui, sans-serif")
        .logo(include_bytes!("../../resources/admin/logo.png"), "image/png")
        .favicon(include_bytes!("../../resources/admin/favicon.png"), "image/png")
        .color_scheme(ColorScheme::System), // or Light, Dark
)
```

| Token (`Theme::color`, `Palette::color`) | Used for |
|---|---|
| `background`, `foreground` | page background and text |
| `card`, `muted`, `muted-foreground`, `border`, `ring` | panels, secondary text, borders, focus rings |
| `primary`, `primary-foreground` | buttons, active links, the default logo mark |
| `secondary`, `destructive`, `destructive-foreground`, `success`, `warning` | other buttons and states |

- Colors are hex (`#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`); `radius` is up to
  `4rem` or `64px`; `font_family` lists installed fonts (letters, digits,
  spaces, dashes; each is quoted for you). Anything else stops the boot: no
  text of yours is copied into CSS as written.
- `dark(…)` sets tokens of the dark scheme; tokens it leaves out keep the
  panel's dark defaults. `ColorScheme` is the default scheme: `Light`,
  `Dark`, or `System` (default), which follows the operating system and its
  changes. Each user can choose Light, Dark or System at the bottom of the
  sidebar. The choice is kept in that browser (other tabs follow), and the
  app's `ColorScheme` applies until they choose.
- Logo and favicon are PNG, WebP or ICO up to 512 KiB, checked against their
  bytes. SVG and web fonts are not supported yet.
- The theme becomes a stylesheet and images served from
  `{admin}/assets/brand/{theme.css,logo,favicon}?v={hash}` (cached for good
  when `v` is current), loaded before your extension styles. Extension styles
  may use the same tokens: `var(--primary)`, `var(--radius-lg)`,
  `var(--font-sans)`.
- The brand name and logo also appear above the sign-in, password reset and
  invitation forms, and the brand names the browser tab.

## Runtime branding

When the brand is chosen while the app runs (a Settings → Branding page of
your own), give the panel a `BrandingSource`. What it returns goes over
`brand(…)` and the `Theme`, field by field:

```rust,ignore
use rustcms::admin::{AdminBranding, Branding, BrandingSource};

struct SettingsBranding;

#[async_trait]
impl BrandingSource for SettingsBranding {
    async fn load(&self, app: &AppState) -> AppResult<Option<Branding>> {
        let Some(row) = branding::Entity::find().one(app.db()).await? else { return Ok(None) };
        let mut b = Branding::new().name(&row.name).map_err(AppError::internal)?;
        if let Some(logo) = row.logo {
            b = b.logo(logo, &row.logo_mime).map_err(AppError::internal)?;
        }
        Ok(Some(b.primary(&row.primary).map_err(AppError::internal)?))
    }
}

AdminProvider::new().brand("Acme").theme(theme).branding(SettingsBranding)

// After saving the branding (in your settings handler):
app.service::<AdminBranding>().expect("AdminProvider").invalidate();
```

- **What it sets**: `name` (1–64 characters, plain text), `logo`, `logo_dark`
  (shown when the page is dark; else `logo`), `favicon` (PNG, WebP or ICO up
  to 512 KiB, checked against their bytes; never SVG), `color`, `primary`,
  `dark(…)` (with `Palette::try_color`) and `radius`: the theme's tokens and
  rules. Fonts and custom CSS are not available at runtime. Each setter
  returns `Result<_, BrandingError>`; validate uploads with the same rules
  through `Branding::check_image`, `Branding::check_color` and
  `Branding::check_name`. Contrast is not checked.
- **Cache**: each instance keeps what `load` returned for
  `[admin] branding_ttl_secs` (default 30, 1–3600), one load at a time.
  `AdminBranding::invalidate()` makes the next request on this instance load
  again; other instances follow within the period (there is no push between
  instances). A load that overlaps an invalidation is not kept.
- **Failures**: `None`, an error, or a load slower than
  `[admin] branding_timeout_ms` (default 2000, 1–30000) shows the static brand
  and theme until the next period; the error is logged without its data.
- **One branding per page**: the panel reads the brand, logos and favicon
  from the page it loaded with, until a reload. Images and the stylesheet are
  served by kind, so a page from another instance or from before a change
  still gets images (the current ones, not cached).
- **Recovery**: `[admin] branding = "static"` (or
  `RUSTCMS_ADMIN__BRANDING=static`) ignores the source until it is removed; a
  restart applies it. `rustcms admin:branding` shows the mode and, in runtime
  mode, loads the branding once and reports whether it is valid; it changes
  nothing.

## Custom pages

A page's place in the sidebar follows `PanelPageMeta::group` and
[`sort`](overview.md#order); `AdminProvider::home(slug)` makes one the
[home page](overview.md#home-page).

```rust,ignore
// src/admin/pages/course_overview_page.rs (`rustcms make:page CourseOverviewPage`)
pub struct CourseOverviewPage;

#[async_trait]
impl PanelPage for CourseOverviewPage {
    const SLUG: &'static str = "course-overview";            // /admin/p/course-overview

    fn meta() -> PanelPageMeta {
        PanelPageMeta::new("Course overview").icon("layers").group("Learning").permission("courses.view_any")
    }

    fn view() -> PageView {
        rustcms::admin::view!("course_overview.html")    // resources/views/admin/course_overview.html
    }

    async fn data(ctx: &Ctx) -> AppResult<Value> {
        let courses = ctx.records("courses", &ListQuery::default()).await?;   // scope, policy, readable fields
        Ok(json!({ "courses": courses.data }))
    }
}
// providers/admin_panel_provider.rs: .page::<CourseOverviewPage>()
```

```html
<!-- resources/views/admin/course_overview.html -->
<section class="rc-grid">
  {% for course in data.courses %}
  <article class="rc-card">
    <h2 class="rc-title"><a class="rc-link" href="{{ admin_url('r/courses/records/' ~ course.key) }}">{{ course.title }}</a></h2>
    <meter class="rc-meter" min="0" max="{{ course.data.capacity }}" value="3"></meter>
  </article>
  {% else %}
  <p class="rc-muted">{{ t("No courses yet.") }}</p>
  {% endfor %}
</section>
```

- **Who sees it:** every `.permission(…)` of `PanelPageMeta`, and at least
  one of each `.permission_any([…])` group. A page needs at least one of
  either, and an empty group stops the boot.
  `declare_permission("reports.view")` also declares it for
  `permission:sync`.
  - `permission_any` suits a page that gathers several sections, for example
    a settings page for whoever manages any of them:
    `.permission_any(["storage_providers.view_any", "notification_channels.view_any"])`.
    The page's `data` and its [admin routes](admin-routes.md) still check
    each section. The navigation lists the pages a user
  may open. A user who may open no page gets 403 for any slug; otherwise an
  unknown slug is 404 and a page they may not open 403. `data` runs only for
  allowed users.
- **`data` is application code**, like a controller: read records with
  `ctx.records(slug, &query)` and `ctx.record(slug, key)`, which apply the
  resource's scope, policy and field read rules. Anything else you return
  (counts, joins), make sure the user may see it. Values keep the
  [wire format](../reference/wire-format.md) (64-bit integers as strings). At
  most 1 MiB.
- **Templates** use [minijinja](https://docs.rs/minijinja) (Jinja syntax).
  HTML escaping is always on and cannot be turned off (`safe`, `tojson` and
  the `autoescape` statement are not available, however it is spaced); one
  template per page (no `include`, `import`, `from`, `extends`); compiled at
  boot (an error stops it); bounded work, and output stops at 512 KiB (the
  request fails with 500, nothing larger is held in memory). Templates are
  application code, like `data`: the bounds catch mistakes over large data,
  they do not make a hostile template safe. The context: `data`, `locale`, `user.id`,
  `t("text")` (your catalogs, like `ctx.t`), `can("permission")`,
  `admin_url("path")` (a link in the panel). **Quote every attribute value.**
- **Then the HTML is cleaned** by an allowlist: structure, text, lists,
  tables, links, images, `details`, `progress`, `meter`, `time`; attributes
  `class`, `title`, `role`, `aria-*`, `data-*` and a few per element. Removed:
  scripts, styles and `style=`, event handlers, forms and inputs, frames,
  `svg`, `id` and `name`. Links must stay in the panel (`/admin/…`, not its API
  or assets) or be `https:` / `mailto:`; images must come from the panel
  (media files, assets). Links to the panel navigate without reloading.
- **Classes for templates** (the panel's utility classes are not available
  to your HTML): `rc-stack`, `rc-grid`, `rc-card`, `rc-title`, `rc-muted`,
  `rc-table`, `rc-badge`, `rc-link`, `rc-meter`, styled with the theme. Add
  your own with `extension_style`.
- The page is rendered for each request, in the user's language: nothing is
  cached but the compiled template.
- **Editing templates:** `view!("file.html")` embeds
  `resources/views/admin/file.html` in the binary, so a release build never
  reads the file. In a debug build with `APP_ENV=local` (`cargo run -- serve`
  on your machine), the file is read again at each request: save it and
  reload the page, no rebuild. A file read this way is checked as at boot; an
  error answers 500 and is logged (fix the file and reload). If the file is
  missing, the embedded copy is used. `lang:missing` reads the copy embedded
  at build time. `PageView::Template("…")` takes the text directly and is
  never read again.

### Script pages

Two kinds of scripts exist:

- **v1** (`extension_script`, below): `mount(element, ctx)` gets an element
  and builds DOM by hand. No build step.
- **v2** ([React pages](react-pages.md), `extension_script_v2`): a React
  component rendered inside the panel, with its UI kit, React Query, the API
  client, sub-pages and `can()`. Built with npm, committed.

```rust,ignore
fn view() -> PageView { PageView::Script("lms/calendar") }    // `rustcms make:page CalendarPage --script`
```

```js
// resources/admin/js/lms/calendar.js, loaded with .extension_script("calendar", include_str!(…))
window.rustcms.registerPage("lms/calendar", {
  apiVersion: 1,
  mount(element, ctx) {
    // ctx: { data, locale, t(text), navigate(path), reload() }
    const list = document.createElement("ul");
    element.append(list);
    const render = (current) => { list.replaceChildren(/* … from current.data … */); };
    render(ctx);
    return { update: render, unmount: () => element.replaceChildren() };
  },
});
```

The script gets the page's `data` (a copy), is told about changes (language)
with `update`, and is unmounted when the user leaves. `navigate` opens a
page of the panel; `reload` reads the page again. Both do nothing once the
view is unmounted or the session changed (sign-in, sign-out, credentials
rotated in any tab): the view is mounted again for the new session. If the
page can no longer be read (403, 404), the view is unmounted and the error
shown. Scripts register while the panel loads (names `vendor/name`, each part
at most 32 lowercase letters, digits or `-`). A script that is missing, throws
or does not return `{ update, unmount }` shows a notice instead of the page
(what it drew is removed). **Scripts run with the panel's full privileges**
(the same origin, the user's session): build elements rather than writing
`innerHTML` with data, and write through resources and actions.

## Slots beside records

```rust,ignore
pub struct CourseSeatsSlot;

#[async_trait]
impl PanelSlot for CourseSeatsSlot {
    const NAME: &'static str = "course-seats";
    fn placement() -> Placement { Placement::RecordAside("courses") }
    fn meta() -> SlotMeta { SlotMeta::new().label("Seats").permission("courses.view") }
    fn view() -> PageView { rustcms::admin::view!("course_seats.html") }
    async fn data(ctx: &Ctx, record: &Envelope) -> AppResult<Value> { … }
}
// .slot::<CourseSeatsSlot>()
```

A slot appears on the record's view page, in a card titled with its label.
The record is loaded as that page loads it (missing or out of scope: 404;
denied: 403) and `data` gets it with readable fields only. A resource whose
policy admits by permission (the default policy) answers 403 to a user
without its `view` before any parameter is checked. A slot that fails to load
again (no longer allowed) disappears. Slots whose
permissions the user lacks are left out before their data is read. Templates
and scripts work as for pages (`registerSlot` for scripts; `ctx.record` is
the record). Dashboard and header placements are planned.

## Custom cells

```rust,ignore
TextColumn::make("progress").cell("lms/progress").props(json!({"max": 100}))
```

```js
window.rustcms.registerCell("lms/progress", {
  apiVersion: 1,
  mount(element, ctx) {           // ctx: { data (the value), record, props, locale, t }
    const meter = document.createElement("meter");
    meter.max = 100;
    element.append(meter);
    const render = (current) => { meter.value = Number(current.data) || 0; };
    render(ctx);
    return { update: render, unmount: () => element.replaceChildren() };
  },
});
```

The cell draws the value the user may read (the column only exists for
readable fields). Until a script registers the name, or after it throws, the
column is drawn by its type. No inline styles: use attributes (`meter`,
`progress`) or classes from an extension stylesheet.

## Translations

`rustcms lang:missing vi` lists page labels and groups, slot labels and every
call of `t` with a string literal in your templates' code (`{{ t("Seats") }}`,
`t( 'It\'s' )`, inside `{% %}` too; not text outside `{{ }}` / `{% %}` nor
comments). Text computed at render (`t(name)`) is not found: add it to the
catalog yourself. Script text uses
`window.rustcms.registerTranslations` ([Custom fields](custom-fields.md)).
