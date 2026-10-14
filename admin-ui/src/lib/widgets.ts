// Custom widgets from application extension scripts (trusted code, same origin).
// The host API is framework-agnostic: a widget mounts into an element and is
// told about every change; the panel owns form state and submission.

import { getLocale } from "@/i18n";

/** What a widget is given; `value` is the field's wire JSON (undefined: not set). */
export interface WidgetContext {
  id: string;
  label: string;
  value: unknown;
  disabled: boolean;
  invalid: boolean;
  describedBy?: string;
  props: unknown;
  /** The panel's language (`en`, `vi`); `update` is called when it changes. */
  locale: string;
  /** `text` in the panel's language, from the catalogs given to `registerTranslations`. */
  t: (text: string) => string;
  /** Reports a new value (the only way a widget changes the form). */
  onChange: (value: unknown) => void;
}

export interface WidgetInstance {
  update(ctx: WidgetContext): void;
  unmount(): void;
}

export interface Widget {
  apiVersion: 1;
  mount(element: HTMLElement, ctx: WidgetContext): WidgetInstance;
}

/** Version of the widget host API. */
export const WIDGET_API_VERSION = 1;

const NAME = /^[a-z0-9-]{1,32}\/[a-z0-9-]{1,32}$/;
const widgets = new Map<string, Widget>();
const failed = new Set<string>();
const listeners = new Set<() => void>();
// Scripts that missed their deadline: whatever they register later is refused.
const expired = new WeakSet<Element>();
let frozen = false;

/** Registers `widget` under `name` (`vendor/name`), during extension loading only. */
export function registerWidget(name: string, widget: Widget): void {
  if (frozen || (document.currentScript && expired.has(document.currentScript))) {
    throw new Error(`rustcms: widgets must be registered while extensions load (${name})`);
  }
  if (!NAME.test(name)) throw new Error(`rustcms: invalid widget name ${name}`);
  if (widgets.has(name)) throw new Error(`rustcms: widget ${name} is already registered`);
  if (!widget || widget.apiVersion !== WIDGET_API_VERSION || typeof widget.mount !== "function") {
    throw new Error(`rustcms: widget ${name} does not implement host API version ${WIDGET_API_VERSION}`);
  }
  widgets.set(name, widget);
}

const translations = new Map<string, Map<string, string>>();

/**
 * Translations of an extension's own text for `locale` (English source →
 * translation), during extension loading only. Widgets read them with `ctx.t`.
 */
export function registerTranslations(locale: string, strings: Record<string, string>): void {
  if (frozen || (document.currentScript && expired.has(document.currentScript))) {
    throw new Error(`rustcms: translations must be registered while extensions load (${locale})`);
  }
  const catalog = translations.get(locale) ?? new Map<string, string>();
  for (const [source, text] of Object.entries(strings)) {
    if (typeof text === "string") catalog.set(source, text);
  }
  translations.set(locale, catalog);
}

/** `text` in the current language for extensions, or as written. */
export function translateExtension(text: string): string {
  return translations.get(getLocale())?.get(text) ?? text;
}

/** The widget registered under `name`, unless it is missing or has failed. */
export function widget(name: string | undefined): Widget | undefined {
  return name && !failed.has(name) ? widgets.get(name) : undefined;
}

/** A widget that threw: shown read-only and never submitted from now on. */
export function markFailed(name: string, error: unknown): void {
  console.error(`rustcms: widget ${name} failed`, error);
  if (failed.has(name)) return;
  failed.add(name);
  for (const listener of listeners) listener();
}

/** Calls `listener` whenever a widget becomes unavailable (for `useSyncExternalStore`). */
export function subscribeWidgets(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** Loads extension styles and scripts in order (each bounded), then freezes the registry. */
export async function loadExtensions(basePath: string, extensions: { scripts: string[]; styles: string[] }): Promise<void> {
  (window as unknown as { rustcms: unknown }).rustcms = { registerWidget, registerTranslations, apiVersion: WIDGET_API_VERSION };
  const url = (path: string) => `${basePath}/${path}`;
  for (const href of extensions.styles) {
    const link = document.createElement("link");
    link.rel = "stylesheet";
    link.href = url(href);
    document.head.append(link);
  }
  for (const src of extensions.scripts) {
    await new Promise<void>((resolve) => {
      const script = document.createElement("script");
      const timer = setTimeout(() => {
        expired.add(script);
        console.error(`rustcms: extension ${src} did not load in time`);
        resolve();
      }, 10_000);
      script.src = url(src);
      // Loaded one at a time here: an `async=false` script would also hold back
      // the next ones until it runs.
      script.onload = () => {
        clearTimeout(timer);
        resolve();
      };
      script.onerror = () => {
        clearTimeout(timer);
        console.error(`rustcms: extension ${src} failed to load`);
        resolve();
      };
      document.head.append(script);
    });
  }
  frozen = true;
}

/** For tests: forget every widget. */
export function resetWidgets(): void {
  widgets.clear();
  failed.clear();
  translations.clear();
  frozen = false;
}
