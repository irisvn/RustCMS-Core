// The theme's color scheme (`data-scheme` on <html>, which the stylesheet
// follows) and favicon. Colors, radius and fonts come from the theme's own
// stylesheet, loaded with the extension styles.
//
// The user may choose Light, Dark or System for this browser (kept in
// localStorage); without a choice, the app's `Theme::color_scheme` applies.
import type { PanelConfig } from "./config";

export type SchemeChoice = "system" | "light" | "dark";

const KEY = "rustcms.colorScheme";
const CHOICES: readonly SchemeChoice[] = ["system", "light", "dark"];
const listeners = new Set<() => void>();
let fallback: SchemeChoice = "system";
let refresh: () => void = () => {};
// A choice that could not be stored (blocked storage): this page only.
let override: SchemeChoice | null = null;

/** The user's choice in this browser, or `null` (storage empty, blocked or invalid). */
export function storedScheme(): SchemeChoice | null {
  try {
    const value = window.localStorage.getItem(KEY);
    return CHOICES.includes(value as SchemeChoice) ? (value as SchemeChoice) : null;
  } catch {
    return null;
  }
}

/** What applies now: the user's choice, else the app's default. */
export function schemeChoice(): SchemeChoice {
  return override ?? storedScheme() ?? fallback;
}

/** Keeps `choice` for this browser and applies it (a failed write still applies it here). */
export function chooseScheme(choice: SchemeChoice): void {
  try {
    window.localStorage.setItem(KEY, choice);
    override = null;
  } catch {
    override = choice;
  }
  refresh();
  for (const listener of listeners) listener();
}

export function subscribeScheme(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function applyTheme(config: PanelConfig, doc: Document = document): () => void {
  const root = doc.documentElement;
  const theme = config.theme ?? { colorScheme: "system" };
  if (theme.favicon) {
    const link = doc.createElement("link");
    link.rel = "icon";
    link.href = `${config.basePath}/${theme.favicon}`;
    doc.head.append(link);
  }
  fallback = theme.colorScheme;
  override = null;
  // As chosen, or as the operating system prefers (following its changes).
  const query = typeof window.matchMedia === "function" ? window.matchMedia("(prefers-color-scheme: dark)") : null;
  const apply = () => {
    const choice = schemeChoice();
    root.dataset.scheme = choice === "system" ? (query?.matches ? "dark" : "light") : choice;
  };
  refresh = apply;
  apply();
  query?.addEventListener?.("change", apply);
  // Another tab's choice (or storage cleared): it wins over this page's own.
  const storage = (e: StorageEvent) => {
    if (e.key === KEY || e.key === null) {
      override = null;
      apply();
      for (const listener of listeners) listener();
    }
  };
  window.addEventListener("storage", storage);
  return () => {
    query?.removeEventListener?.("change", apply);
    window.removeEventListener("storage", storage);
    refresh = () => {};
  };
}
