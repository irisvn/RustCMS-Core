// The `lms/progress` cell: an enrollment's progress as a bar and a number
// (`TextColumn::make("progress").cell("lms/progress")`). No inline styles:
// the bar is a native <meter>, styled by resources/admin/css/lms/progress.css.
// #region cell
window.rustcms.registerCell("lms/progress", {
  apiVersion: 1,
  mount(element, ctx) {
    const meter = document.createElement("meter");
    meter.className = "lms-progress";
    meter.min = 0;
    meter.max = 100;
    const label = document.createElement("span");
    label.className = "lms-progress-label";
    element.append(meter, label);
    const render = (current) => {
      const value = Number(current.data) || 0;
      meter.value = value;
      label.textContent = `${value}%`;
      meter.setAttribute("aria-label", `${current.t("Progress")} ${value}%`);
    };
    render(ctx);
    return {
      update: render,
      unmount() {
        element.replaceChildren();
      },
    };
  },
});
window.rustcms.registerTranslations("vi", { Progress: "Tiến độ" });
// #endregion cell
