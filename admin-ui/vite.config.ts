/// <reference types="vitest/config" />
import path from "node:path";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { type Plugin, defineConfig } from "vite";
import pkg from "./package.json" with { type: "json" };

// Production: built into the Rust crate, served under the admin path with a
// `<base href>` injected by the server, so asset URLs are relative.
// Development: Vite serves /admin/ and proxies the API to the Rust server.
const backend = process.env.RUSTCMS_BACKEND ?? "http://127.0.0.1:8000";

// Development: the runtime config (extensions included) comes from the Rust
// server's shell, as in production.
const runtimeConfig = (): Plugin => ({
  name: "rustcms-runtime-config",
  apply: "serve",
  async transformIndexHtml(html) {
    try {
      const shell = await (await fetch(`${backend}/admin/`)).text();
      const meta = /<meta name="rustcms-admin"[^>]*>/.exec(shell)?.[0];
      return meta ? html.replace("<head>", `<head>\n    ${meta}`) : html;
    } catch {
      return html;
    }
  },
});

export default defineConfig(({ command }) => ({
  base: command === "build" ? "./" : "/admin/",
  plugins: [react(), tailwindcss(), runtimeConfig()],
  resolve: { alias: { "@": path.resolve(__dirname, "src") } },
  // The host's versions, for v2 extensions' `requires` (ADR 0032).
  define: {
    __RUSTCMS_HOST__: JSON.stringify({ reactQuery: pkg.dependencies["@tanstack/react-query"] }),
  },
  build: {
    outDir: "../crates/rustcms-admin/assets",
    emptyOutDir: true,
    assetsDir: "assets",
    sourcemap: false,
    // No inline <script>/<style> or data: modules: the panel's CSP only allows 'self'.
    assetsInlineLimit: 0,
    modulePreload: { polyfill: false },
  },
  server: {
    port: 5173,
    strictPort: true,
    proxy: {
      "/admin/api": { target: backend, changeOrigin: false },
      "/admin/assets/ext": { target: backend, changeOrigin: false },
    },
  },
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./tests/setup.ts"],
    include: ["tests/**/*.test.{ts,tsx}"],
  },
}));
