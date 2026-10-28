# Custom fields

When none of the built-in inputs fits (a color swatch, a map, a code
editor), an application can ship its own form input: a **widget** written in
plain JavaScript, loaded by the panel from an **extension script** embedded in
the application binary. The server still validates the value by the field's
kind and rules; the widget only edits it.

![The category form with the color swatch widget](/img/admin/custom-widget.webp)
*A RustCMS demo application: the `demo/color` widget on the category form.*

## Quick start

```sh
rustcms make:widget acme/color
```

This writes `resources/admin/js/acme/color.js` (a starting point that
registers `acme/color`) and prints the two lines to add:

```rust
// src/providers/admin_panel_provider.rs
.extension_script("acme-color-…", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/resources/admin/js/acme/color.js")))

// The resource's form: the input names its widget.
CustomInput::make("color", "acme/color")
```

The extension name it prints is the widget name with `/` replaced by `-`,
followed by a short hash, so two widget names never share one. The command
never overwrites a file.

## Full example

The demo's category color is an ordinary text column with a length rule:

<<< @/../../../examples/demo/src/admin/resources/category_resource.rs#color-field [src/admin/resources/category_resource.rs]

Its form edits it with the `demo/color` widget and passes the palette as
props (JSON, at most 4 KiB, checked at boot):

<<< @/../../../examples/demo/src/admin/resources/category_resource.rs#color-input [src/admin/resources/category_resource.rs]

The provider loads the script and its stylesheet:

<<< @/../../../examples/demo/src/providers/admin_panel_provider.rs#color-extension [src/providers/admin_panel_provider.rs]

The widget itself:

<<< @/../../../examples/demo/resources/admin/js/demo/color.js#widget [resources/admin/js/demo/color.js]

## Names and assets

- Widget names are `vendor/name`, each part 1–32 of `a-z`, `0-9`, `-`.
- Extension names (the first argument of `extension_script` and
  `extension_style`) are 1–64 of the same characters. A name may serve once
  as a script and once as a stylesheet; adding the same script or stylesheet
  name twice stops the boot.
- Each extension is served from `{admin}/assets/ext/<name>.<hash>.js|css`,
  with a hash of its content, and cached as immutable.
- Stylesheets are inserted in the order they are registered (their loading
  is not awaited). Scripts load one after the other, in registration order,
  and the panel renders once they have loaded or timed out.

| `AdminProvider` method | Effect |
|---|---|
| `extension_script(name, js)` | a v1 script (widgets, script pages, slots, cells) |
| `extension_style(name, css)` | a stylesheet (inline styles are not allowed) |
| `extension_script_v2(name, js, Requires { … })` | a [React](react-pages.md) script: pages, slots and cells only |
| `extension_timeout(Duration)` | how long the panel waits for each script (1–60 s, default 10; panics outside that range) |

## The widget contract (API version 1)

```js
window.rustcms.registerWidget("acme/color", {
  apiVersion: 1,
  mount(element, ctx) {
    // Build the UI inside `element`; report changes with ctx.onChange(value).
    return {
      update(ctx) { /* the value, disabled or invalid state changed */ },
      unmount() { /* remove listeners; the panel empties `element` */ },
    };
  },
});
```

| `ctx` | |
|---|---|
| `id` | put it on the main control: it is the label's target |
| `label` | the field's label, as the form shows it |
| `value` | the field's [wire value](../reference/wire-format.md), `null`, or `undefined` when not set |
| `disabled`, `invalid` | the field's state |
| `describedBy` | ids of the help and error texts, for `aria-describedby` |
| `props` | the JSON given to `CustomInput::props` |
| `locale`, `t(text)` | the panel's language and your catalogs |
| `onChange(value)` | the only way to change the form |

- `value` is a string for decimals and 64-bit integers, an ISO string for
  dates. It is a copy: changing it does nothing.
- `onChange(value)`: `undefined` means "not set" (the field is left out of
  the request); `null` sends `null`. Calls while disabled, or after
  `unmount`, are ignored. `mount` and `update` must not call it.
- `update` is called only when something changed, including the language.
  React's development mode may mount, unmount and mount again: `unmount` must
  undo `mount`.
- Widgets translate their own text with `ctx.t`, from catalogs registered
  while the script loads
  (`window.rustcms.registerTranslations("vi", { None: "Không màu" })`;
  [Languages](i18n.md#extensions)). Props are never translated.
- Register widgets while the script runs. The registry is frozen once all
  extensions are loaded, and a script that missed its deadline can no longer
  register anything. A name can be registered once.

::: tip Widgets are v1 only
Field widgets exist only in this DOM API. The React SDK
([React pages](react-pages.md)) has pages, slots and cells, but no React
field widget.
:::

## Failures

![A field whose widget failed: the stored value shown read-only](/img/admin/custom-widget-fallback.webp)
*A RustCMS demo application: the fallback of a widget that failed.*

- A script that fails to load, or takes longer than
  `AdminProvider::extension_timeout`, is skipped (the error is in the
  browser console).
- A widget that throws, returns no `{update, unmount}`, or reports a number
  JSON cannot carry exactly (`NaN`, `Infinity`, an integer outside −(2^53−1) to 2^53−1) is
  disabled for the rest of the page's life, in every field that uses it.

In both cases its fields are shown read-only (the stored JSON) and never
sent; a required one blocks the form with "cannot be edited here: its widget
is not available".

## Trust and CSP

Extensions are application code, not user content. They run with the panel's
full privileges (the user's session, every API call), also before sign-in,
and the panel's CSP does not isolate them from each other. Only ship code you
review.

The CSP still applies to them, and it is not something to loosen: no inline
scripts, no `style` attributes or `<style>` elements (use `extension_style`),
images only from the panel or `data:` URLs. Build elements with the DOM
rather than writing `innerHTML` with data.

The server never trusts the widget: the value it sends is checked against the
field's kind and rules, `scope()` and the policy, like any other input.

## Pages, slots and cells

The same extension scripts may draw custom pages (`registerPage`), content
beside records (`registerSlot`) and table cells (`registerCell`), with the
same lifecycle (`mount`, `update`, `unmount`): see
[Customizing the panel](customizing.md#script-pages) and
[custom cells](customizing.md#custom-cells) (`TableColumn::cell("vendor/name")`
with `props`, also at most 4 KiB). React versions of pages, slots and cells
are [React pages](react-pages.md).

## Related pages

- [Forms](forms.md): the built-in inputs
- [Customizing the panel](customizing.md)
- [React pages](react-pages.md)
- [Languages](i18n.md#extensions)
- [Wire format](../reference/wire-format.md)
