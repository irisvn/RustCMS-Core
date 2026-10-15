// The theme's color scheme (`data-scheme` on <html>, which the stylesheet
// follows) and favicon. Colors, radius and fonts come from the theme's own
// stylesheet, loaded with the extension styles.
import type { PanelConfig } from "./config";

export function applyTheme(config: PanelConfig, doc: Document = document): () => void {
  const root = doc.documentElement;
  const theme = config.theme ?? { colorScheme: "system" };
  if (theme.favicon) {
    const link = doc.createElement("link");
    link.rel = "icon";
    link.href = `${config.basePath}/${theme.favicon}`;
    doc.head.append(link);
  }
  if (theme.colorScheme !== "system") {
    root.dataset.scheme = theme.colorScheme;
    return () => {};
  }
  // As the operating system prefers, following its changes.
  const query = typeof window.matchMedia === "function" ? window.matchMedia("(prefers-color-scheme: dark)") : null;
  const apply = () => {
    root.dataset.scheme = query?.matches ? "dark" : "light";
  };
  apply();
  query?.addEventListener?.("change", apply);
  return () => query?.removeEventListener?.("change", apply);
}
