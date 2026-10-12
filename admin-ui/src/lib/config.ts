// Runtime configuration injected by the server into index.html as
// <meta name="rustcms-admin" content='{"basePath":"/admin"}'> (no inline script: CSP).

export interface PanelConfig {
  /** URL prefix of the panel, without trailing slash (e.g. `/admin`). */
  basePath: string;
  /** URL prefix of the admin API (e.g. `/admin/api`). */
  apiPath: string;
}

export function readConfig(doc: Document = document): PanelConfig {
  const raw = doc.querySelector('meta[name="rustcms-admin"]')?.getAttribute("content");
  let basePath = "/admin";
  if (raw) {
    try {
      const parsed = JSON.parse(raw) as Partial<PanelConfig>;
      if (typeof parsed.basePath === "string") basePath = parsed.basePath;
    } catch {
      // Keep the default: a broken meta tag must not break the panel.
    }
  }
  basePath = "/" + basePath.replace(/^\/+|\/+$/g, "");
  if (basePath === "/") basePath = "";
  return { basePath, apiPath: `${basePath}/api` };
}

/**
 * A same-origin path inside the panel, or null. Used for `?redirect=` so a
 * crafted login link cannot send the user to another site.
 */
export function safeRedirect(target: string | null | undefined, basePath: string): string | null {
  if (!target || !target.startsWith("/") || target.startsWith("//") || target.includes("\\")) return null;
  let url: URL;
  try {
    url = new URL(target, "http://panel.invalid");
  } catch {
    return null;
  }
  if (url.origin !== "http://panel.invalid") return null;
  const path = url.pathname;
  if (path !== basePath && !path.startsWith(`${basePath}/`)) return null;
  if (path.startsWith(`${basePath}/api`)) return null;
  return path + url.search + url.hash;
}

/** Links from server data: only http(s), mailto and relative URLs are rendered. */
export function safeHref(href: string): string | null {
  const trimmed = href.trim();
  if (trimmed.startsWith("/") && !trimmed.startsWith("//")) return trimmed;
  try {
    const url = new URL(trimmed);
    return ["http:", "https:", "mailto:"].includes(url.protocol) ? url.href : null;
  } catch {
    return null;
  }
}
