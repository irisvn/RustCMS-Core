// Runtime configuration injected by the server into index.html as
// <meta name="rustcms-admin" content='{"basePath":"/admin"}'> (no inline script: CSP).

export interface PanelConfig {
  /** URL prefix of the panel, without trailing slash (e.g. `/admin`). */
  basePath: string;
  /** URL prefix of the admin API (e.g. `/admin/api`). */
  apiPath: string;
  /** Extension scripts and styles, relative to `basePath`, in load order. */
  extensions?: import("./widgets").ExtensionsConfig;
  /** Default interface language and the languages users may choose. */
  locale?: string;
  locales?: string[];
  /** The theme: logo and favicon (relative to `basePath`), color scheme. */
  theme?: { logo?: string; favicon?: string; colorScheme: "system" | "light" | "dark" };
  /** What the sign-in page offers. */
  signIn?: { passwordReset: boolean; remember: boolean };
}

export function readConfig(doc: Document = document): PanelConfig {
  const raw = doc.querySelector('meta[name="rustcms-admin"]')?.getAttribute("content");
  let basePath = "/admin";
  let extensions: PanelConfig["extensions"];
  let locale: string | undefined;
  let locales: string[] | undefined;
  const theme: NonNullable<PanelConfig["theme"]> = { colorScheme: "system" };
  const signIn = { passwordReset: false, remember: false };
  if (raw) {
    try {
      const parsed = JSON.parse(raw) as Partial<PanelConfig>;
      if (typeof parsed.basePath === "string") basePath = parsed.basePath;
      const ext = parsed.extensions;
      // Only extension asset paths of the panel itself.
      const own = (list: unknown, ext: string) =>
        Array.isArray(list)
          ? list.filter((p): p is string => typeof p === "string" && new RegExp(`^assets/ext/[a-z0-9-]+\\.[0-9a-f]{8}\\.${ext}$`).test(p))
          : [];
      if (ext) {
        const scripts = own(ext.scripts, "js");
        // v2 requirements, only for this panel's own scripts and well-formed.
        const v2: Record<string, import("./host").Requires> = {};
        const given = ext.v2 && typeof ext.v2 === "object" ? (ext.v2 as Record<string, unknown>) : {};
        for (const script of scripts) {
          const r = given[script] as Partial<import("./host").Requires> | undefined;
          if (r && typeof r.api === "number" && typeof r.react === "string" && typeof r.reactQuery === "string" && typeof r.ui === "number") {
            v2[script] = { api: r.api, react: r.react, reactQuery: r.reactQuery, ui: r.ui };
          }
        }
        const timeoutMs = typeof ext.timeoutMs === "number" && Number.isFinite(ext.timeoutMs) ? ext.timeoutMs : undefined;
        extensions = { scripts, styles: own(ext.styles, "css") };
        if (Object.keys(v2).length > 0) extensions.v2 = v2;
        if (timeoutMs !== undefined) extensions.timeoutMs = timeoutMs;
      }
      if (typeof parsed.locale === "string") locale = parsed.locale;
      if (Array.isArray(parsed.locales)) locales = parsed.locales.filter((l): l is string => typeof l === "string");
      if (parsed.signIn && typeof parsed.signIn === "object") {
        signIn.passwordReset = parsed.signIn.passwordReset === true;
        signIn.remember = parsed.signIn.remember === true;
      }
      const look = parsed.theme as Partial<NonNullable<PanelConfig["theme"]>> | undefined;
      const image = (path: unknown) =>
        typeof path === "string" && /^assets\/ext\/(logo|favicon)\.[0-9a-f]{8}\.(png|webp|ico)$/.test(path) ? path : undefined;
      if (look) {
        theme.logo = image(look.logo);
        theme.favicon = image(look.favicon);
        if (look.colorScheme === "light" || look.colorScheme === "dark") theme.colorScheme = look.colorScheme;
      }
    } catch {
      // Keep the default: a broken meta tag must not break the panel.
    }
  }
  basePath = "/" + basePath.replace(/^\/+|\/+$/g, "");
  if (basePath === "/") basePath = "";
  return { basePath, apiPath: `${basePath}/api`, extensions, locale, locales, theme, signIn };
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

/**
 * A link that stays in the panel, as a router path (`/r/posts`), or null: the
 * same origin, under `basePath` (segment boundary), not its API or assets.
 */
export function panelPath(href: string, basePath: string, origin: string = window.location.origin): string | null {
  if (!href || /[\s\\]/.test(href) || [...href].some((c) => c.charCodeAt(0) < 32)) return null;
  let url: URL;
  try {
    url = new URL(href, origin);
  } catch {
    return null;
  }
  if (url.origin !== origin) return null;
  const path = url.pathname;
  if (path !== basePath && !path.startsWith(`${basePath}/`)) return null;
  const inner = path.slice(basePath.length) || "/";
  if (inner === "/api" || inner.startsWith("/api/") || inner.startsWith("/assets/")) return null;
  return inner + url.search + url.hash;
}

/** Links from server data: only http(s), mailto and relative URLs are rendered. */
export function safeHref(href: string): string | null {
  const trimmed = href.trim();
  if (trimmed.includes("\\")) return null;
  if (trimmed.startsWith("/") && !trimmed.startsWith("//")) return trimmed;
  try {
    const url = new URL(trimmed);
    return ["http:", "https:", "mailto:"].includes(url.protocol) ? url.href : null;
  } catch {
    return null;
  }
}
