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
  panel's dark defaults. `ColorScheme::Light` / `Dark` forces one scheme;
  `System` (default) follows the operating system and its changes.
- Logo and favicon are PNG, WebP or ICO up to 512 KiB, checked against their
  bytes. SVG and web fonts are not supported yet.
- The theme becomes a stylesheet and images served from
  `{admin}/assets/ext/` with content hashes (cached for good), loaded before
  your extension styles. Extension styles may use the same tokens:
  `var(--primary)`, `var(--radius-lg)`, `var(--font-sans)`.

## Custom pages

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
        PageView::Template(include_str!(concat!(env!("CARGO_MANIFEST_DIR"),
            "/resources/views/admin/course_overview.html")))
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

- **Who sees it:** every permission of `PanelPageMeta` (at least one: a page
  without one stops the boot). `declare_permission("reports.view")` also
  declares it for `permission:sync`. The navigation lists the pages a user
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
  `autoescape` are not available); one template per page (no `include`,
  `import`, `extends`); compiled at boot (an error stops it); bounded work and
  output (512 KiB). The context: `data`, `locale`, `user.id`,
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

### Script pages

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
page of the panel; `reload` reads the page again. Scripts register while the
panel loads (names `vendor/name`). A script that is missing or throws shows a
notice instead of the page. **Scripts run with the panel's full privileges**
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
    fn view() -> PageView { PageView::Template(include_str!(…)) }
    async fn data(ctx: &Ctx, record: &Envelope) -> AppResult<Value> { … }
}
// .slot::<CourseSeatsSlot>()
```

A slot appears on the record's view page, in a card titled with its label.
The record is loaded as that page loads it (missing or out of scope: 404;
denied: 403) and `data` gets it with readable fields only. Slots whose
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
literal `t("…")` of your templates. Script text uses
`window.rustcms.registerTranslations` ([Custom fields](custom-fields.md)).
