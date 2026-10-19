# Custom fields

When none of the built-in inputs fits, an application can ship its own: a
**widget** written in plain JavaScript, loaded by the panel from an
**extension script** embedded in the application binary.

```rust,ignore
// The field: any column; the server validates the value by its kind and rules.
Field::column(category::Column::Color).max_length(7),

// The form: the input names its widget, with optional JSON props (≤ 4 KiB).
CustomInput::make("color", "demo/color").props(json!({"palette": [{"value": "#2563eb", "label": "Blue"}]})),

// The provider: scripts (and stylesheets) load in order before the panel renders.
AdminProvider::new()
    .extension_script("demo-color", include_str!("../../resources/admin/js/demo/color.js"))
    .extension_style("demo-color", include_str!("../../resources/admin/css/demo/color.css"))
```

`rustcms make:widget acme/color` writes a starting point to
`resources/admin/js/acme/color.js` and prints the registration line.

Widget names are `vendor/name` (each part 1–32 of `a-z`, `0-9`, `-`); extension
names are 1–64 of the same characters. Each extension is served from
`{admin}/assets/ext/<name>.<hash>.js|css`, with the hash of its content, and
cached as immutable.

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

`ctx` holds `id` (put it on the main control, it is the label's target),
`label`, `value`, `disabled`, `invalid`, `describedBy` (ids of the help and
error texts, for `aria-describedby`), `props`, `locale`, `t(text)` and
`onChange(value)`. Widgets translate their own text with `ctx.t`, from
catalogs registered while the script loads
(`window.rustcms.registerTranslations("vi", { None: "Không màu" })`); `update`
is called when the language changes ([i18n](i18n.md#extensions)).

- `value` is the field's [wire value](../reference/wire-format.md) (a
  string for decimals and 64-bit integers, an ISO string for dates), `null`, or
  `undefined` when not set. It is a copy: changing it does nothing.
- `onChange(value)` is the only way to change the form. `undefined` means "not
  set" (the field is left out of the request); `null` sends `null`. Calls while
  disabled, or after `unmount`, are ignored. `mount` and `update` must not call
  it.
- `update` is called only when something changed. React's development mode may
  mount, unmount and mount again: `unmount` must undo `mount`.
- Register widgets while the script runs: the registry is frozen once all
  extensions are loaded. A name can be registered once.

## Failures

A script that fails to load, or takes longer than
`AdminProvider::extension_timeout` (10 seconds by default, 1–60), is skipped
(the error is in the browser console). A widget that throws, returns no
`{update, unmount}`, or reports a number JSON cannot carry exactly (`NaN`,
`Infinity`, an integer beyond 2^53) is disabled for the rest of the page's
life, in every field that uses it. In both cases its fields are shown read-only (the stored
JSON) and never sent; a required one blocks the form with "cannot be edited
here: its widget is not available".

## Trust and CSP

Extensions are application code, not user content: they run with the panel's
full privileges (the user's session, every API call) and before sign-in, and
the panel's CSP does not isolate them from each other. Only ship code you
review. The CSP still applies to them: no inline scripts, no `style`
attributes or `<style>` elements (use `extension_style`), images only from the
panel or `data:` URLs.

## Pages, slots and cells

The same extension scripts may draw custom pages (`registerPage`), content
beside records (`registerSlot`) and table cells (`registerCell`), with the
same lifecycle (`mount`, `update`, `unmount`): see
[Customizing the panel](customizing.md).

