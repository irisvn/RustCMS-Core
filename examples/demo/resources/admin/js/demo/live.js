// The live page (`LivePage`): a counter streamed from `/admin/api/x/demo/ticks`
// (server-sent events) and reset with a POST. Elements only, no innerHTML.
(() => {
  const base = document.baseURI.replace(/\/$/, "");

  window.rustcms.registerPage("demo/live", {
    apiVersion: 1,
    mount(element, ctx) {
      const count = document.createElement("output");
      count.setAttribute("aria-live", "polite");
      count.setAttribute("aria-label", ctx.t("Count"));
      count.textContent = "–";
      const status = document.createElement("p");
      const reset = document.createElement("button");
      reset.type = "button";
      reset.className = "rc-button";
      reset.textContent = ctx.t("Reset");
      reset.addEventListener("click", async () => {
        const csrf = await fetch(`${base}/api/auth/csrf`, { credentials: "same-origin" }).then((r) => r.json());
        await fetch(`${base}/api/x/demo/ticks/reset`, {
          method: "POST",
          credentials: "same-origin",
          headers: { "X-CSRF-Token": csrf.csrfToken },
        });
      });
      element.append(count, status, reset);
      ctx.realtime.events("/demo/ticks", {
        onEvent(name, data) {
          if (name === "tick") count.textContent = String(data.count);
        },
        onState(state) {
          status.textContent = state.state === "open" ? "" : ctx.t(state.state === "closed" ? "Stopped" : "Reconnecting…");
        },
      });
      return { update() {}, unmount() {} };
    },
  });
})();
