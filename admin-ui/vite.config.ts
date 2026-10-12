/// <reference types="vitest/config" />
import path from "node:path";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// Production: built into the Rust crate, served under the admin path with a
// `<base href>` injected by the server, so asset URLs are relative.
// Development: Vite serves /admin/ and proxies the API to the Rust server.
const backend = process.env.RUSTCMS_BACKEND ?? "http://127.0.0.1:8000";

export default defineConfig(({ command }) => ({
  base: command === "build" ? "./" : "/admin/",
  plugins: [react(), tailwindcss()],
  resolve: { alias: { "@": path.resolve(__dirname, "src") } },
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
    proxy: { "/admin/api": { target: backend, changeOrigin: false } },
  },
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./tests/setup.ts"],
    include: ["tests/**/*.test.{ts,tsx}"],
  },
}));
