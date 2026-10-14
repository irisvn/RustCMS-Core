// Demo widget `demo/color`: picks a category color from the palette in
// `props.palette` ([{ value, label }]). Styles live in color.css: the panel's
// CSP forbids inline styles.
window.rustcms.registerWidget("demo/color", {
  apiVersion: 1,
  mount(element, ctx) {
    let current = ctx;
    const group = document.createElement("div");
    group.className = "demo-color";
    group.setAttribute("role", "group");
    const buttons = (ctx.props?.palette ?? []).map(({ value, label }) => {
      const button = document.createElement("button");
      button.type = "button";
      button.className = "demo-color-swatch";
      button.dataset.color = value;
      button.setAttribute("aria-label", label);
      button.title = label;
      button.addEventListener("click", () => current.onChange(value));
      return button;
    });
    const clear = document.createElement("button");
    clear.type = "button";
    clear.className = "demo-color-clear";
    clear.textContent = "None";
    clear.addEventListener("click", () => current.onChange(null));
    group.append(...buttons, clear);
    element.append(group);
    const render = () => {
      group.id = current.id;
      group.setAttribute("aria-label", current.label);
      if (current.describedBy) group.setAttribute("aria-describedby", current.describedBy);
      else group.removeAttribute("aria-describedby");
      for (const button of [...buttons, clear]) button.disabled = current.disabled;
      for (const button of buttons) button.setAttribute("aria-pressed", String(button.dataset.color === current.value));
      clear.setAttribute("aria-pressed", String(current.value == null));
      group.classList.toggle("demo-color-invalid", current.invalid);
    };
    render();
    return {
      update(next) {
        current = next;
        render();
      },
      unmount() {
        group.remove();
      },
    };
  },
});
