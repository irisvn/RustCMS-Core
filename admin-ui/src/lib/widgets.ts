// Custom widgets from application extension scripts (trusted code, same origin).
// The host API is framework-agnostic: a widget mounts into an element and is
// told about every change; the panel owns form state and submission.

import type * as React from "react";
import { getLocale } from "@/i18n";
import { HOST_VERSIONS, MODULES, type Requires, incompatibility } from "./host";

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

/** What a custom page, slot or cell is given (a copy: changing it changes nothing). */
export interface ViewContext {
  /** Data from the server (`PanelPage::data`, `PanelSlot::data`), or the cell's value. */
  data: unknown;
  /** The record (slots, cells): readable fields only. */
  record?: unknown;
  /** Settings of a cell (`TableColumn::props`). */
  props?: unknown;
  /** The panel's language; `update` is called when it changes. */
  locale: string;
  /** `text` in the panel's language (extension catalogs). */
  t: (text: string) => string;
  /** Opens a page of the panel (`/admin/...`); anything else is ignored. */
  navigate: (path: string) => void;
  /** Reads the page or slot again from the server (pages and slots). */
  reload: () => void;
  /**
   * Server-sent events of an admin route (`AdminRoutes::events`), by its path
   * under `{admin}/api/x` (`/docker/stats/web`). Closed when the view is
   * unmounted or the session changes; see `openEvents`.
   */
  realtime: {
    events: (path: string, handlers: import("./realtime").EventsHandlers) => import("./realtime").EventsHandle;
    /**
     * A WebSocket of an admin route (`AdminRoutes::socket`), by its path under
     * `{admin}/api/x`. No reconnect unless `reconnect: true`; closed when the
     * view is unmounted or the session changes; see `openSocket`.
     */
    socket: (
      path: string,
      handlers: import("./realtime").SocketHandlers,
      options?: { reconnect?: boolean },
    ) => import("./realtime").SocketHandle;
  };
}

export interface ViewInstance {
  update(ctx: ViewContext): void;
  unmount(): void;
}

export interface View {
  apiVersion: 1;
  mount(element: HTMLElement, ctx: ViewContext): ViewInstance;
}

export type ViewKind = "page" | "slot" | "cell";
const views: Record<ViewKind, Map<string, View>> = { page: new Map(), slot: new Map(), cell: new Map() };
const failedViews = new Set<string>();

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

/** React views of v2 scripts (ADR 0032): same names as v1, taken once by either. */
const reactViews: Record<ViewKind, Map<string, React.ComponentType<never>>> = {
  page: new Map(),
  slot: new Map(),
  cell: new Map(),
};

function checkRegistration(kind: ViewKind, name: string): void {
  if (frozen || (document.currentScript && expired.has(document.currentScript))) {
    throw new Error(`rustcms: ${kind}s must be registered while extensions load (${name})`);
  }
  if (!NAME.test(name)) throw new Error(`rustcms: invalid ${kind} name ${name}`);
  if (views[kind].has(name) || reactViews[kind].has(name)) {
    throw new Error(`rustcms: ${kind} ${name} is already registered`);
  }
}

/** Registers a v2 React component (a page, slot or cell) under `name`. */
export function registerReactView(kind: ViewKind, name: string, component: React.ComponentType<never>): void {
  checkRegistration(kind, name);
  if (typeof component !== "function" && (typeof component !== "object" || component === null)) {
    throw new Error(`rustcms: ${kind} ${name} is not a React component`);
  }
  reactViews[kind].set(name, component);
}

/** The v2 component registered as `name`, unless missing or failed. */
export function reactView(kind: ViewKind, name: string | undefined): React.ComponentType<never> | undefined {
  return name && !failedViews.has(`${kind}:${name}`) ? reactViews[kind].get(name) : undefined;
}

function registerView(kind: ViewKind, name: string, view: View): void {
  checkRegistration(kind, name);
  if (!view || view.apiVersion !== WIDGET_API_VERSION || typeof view.mount !== "function") {
    throw new Error(`rustcms: ${kind} ${name} does not implement host API version ${WIDGET_API_VERSION}`);
  }
  views[kind].set(name, view);
}

/** Registers a custom page drawn by a script (`PageView::Script("vendor/name")`). */
export const registerPage = (name: string, view: View) => registerView("page", name, view);
/** Registers a slot drawn by a script. */
export const registerSlot = (name: string, view: View) => registerView("slot", name, view);
/** Registers a table cell (`TableColumn::cell("vendor/name")`). */
export const registerCell = (name: string, view: View) => registerView("cell", name, view);

/** The view registered as `name`, unless it is missing or has failed. */
export function view(kind: ViewKind, name: string | undefined): View | undefined {
  return name && !failedViews.has(`${kind}:${name}`) ? views[kind].get(name) : undefined;
}

/** A view that threw: drawn the default way from now on (cells), or as unavailable. */
export function markViewFailed(kind: ViewKind, name: string, error: unknown): void {
  console.error(`rustcms: ${kind} ${name} failed`, error);
  if (failedViews.has(`${kind}:${name}`)) return;
  failedViews.add(`${kind}:${name}`);
  for (const listener of listeners) listener();
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

/** What the server says about extension scripts. */
export interface ExtensionsConfig {
  scripts: string[];
  styles: string[];
  /** v2 scripts' requirements, by script path (checked before they run). */
  v2?: Record<string, Requires>;
  /** Each script's deadline (`AdminProvider::extension_timeout`). */
  timeoutMs?: number;
}

/**
 * Loads extension styles and scripts in order (each bounded), then freezes
 * the registry. A v2 script whose `requires` do not fit is never inserted.
 */
export async function loadExtensions(basePath: string, extensions: ExtensionsConfig): Promise<void> {
  const v2 = Object.freeze({
    apiVersion: 2,
    versions: HOST_VERSIONS,
    modules: MODULES,
    registerPage: (name: string, component: React.ComponentType<never>) => registerReactView("page", name, component),
    registerSlot: (name: string, component: React.ComponentType<never>) => registerReactView("slot", name, component),
    registerCell: (name: string, component: React.ComponentType<never>) => registerReactView("cell", name, component),
  });
  (window as unknown as { rustcms: unknown }).rustcms = {
    registerWidget,
    registerTranslations,
    registerPage,
    registerSlot,
    registerCell,
    // v1 scripts check `apiVersion === 1`: it stays 1; v2 lives under `v2`.
    apiVersion: WIDGET_API_VERSION,
    v2,
  };
  const deadline = Math.min(60_000, Math.max(1_000, extensions.timeoutMs ?? 10_000));
  const url = (path: string) => `${basePath}/${path}`;
  for (const href of extensions.styles) {
    const link = document.createElement("link");
    link.rel = "stylesheet";
    link.href = url(href);
    document.head.append(link);
  }
  for (const src of extensions.scripts) {
    const requires = extensions.v2?.[src];
    const reason = requires ? incompatibility(requires) : null;
    if (reason) {
      console.error(`rustcms: extension ${src} not loaded: it ${reason}`);
      continue;
    }
    await new Promise<void>((resolve) => {
      const script = document.createElement("script");
      const timer = setTimeout(() => {
        expired.add(script);
        console.error(`rustcms: extension ${src} did not load in time`);
        resolve();
      }, deadline);
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
  for (const map of Object.values(views)) map.clear();
  for (const map of Object.values(reactViews)) map.clear();
  failedViews.clear();
  translations.clear();
  frozen = false;
}
