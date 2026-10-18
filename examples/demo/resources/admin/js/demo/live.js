// The live page (`LivePage`): a counter streamed from `/admin/api/x/demo/ticks`
// (server-sent events) and reset with a POST, and an echo over the
// `/admin/api/x/demo/echo` WebSocket. Elements only, no innerHTML.
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
      const form = document.createElement("form");
      const input = document.createElement("input");
      input.className = "rc-input";
      input.setAttribute("aria-label", ctx.t("Message"));
      const send = document.createElement("button");
      send.className = "rc-button";
      send.textContent = ctx.t("Send");
      const echoed = document.createElement("output");
      echoed.setAttribute("aria-label", ctx.t("Echo"));
      form.append(input, send, echoed);
      element.append(form);
      const socket = ctx.realtime.socket("/demo/echo", {
        onMessage(data) {
          if (typeof data === "string") echoed.textContent = data;
        },
      });
      form.addEventListener("submit", (event) => {
        event.preventDefault();
        if (socket.send(input.value)) input.value = "";
      });
      return { update() {}, unmount() {} };
    },
  });
})();
