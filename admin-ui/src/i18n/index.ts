// Translations of the panel's own text (ADR 0013). Labels of resources come
// translated from the server; validation messages are translated here by their
// stable code, with the server's English text as the fallback.

import { useSyncExternalStore } from "react";
import { en } from "./en";
import { vi } from "./vi";

/** Languages the panel is written in. */
export const LOCALES = ["en", "vi"] as const;
export type Locale = (typeof LOCALES)[number];

type Catalog = typeof en;
type Plural<K> = K extends `${infer Base}_other` ? Base : never;
/** A message key; plural messages are named without their `_one`/`_other` suffix. */
export type Key = Exclude<keyof Catalog, `${string}_one` | `${string}_other`> | Plural<keyof Catalog>;
export type Params = Record<string, string | number>;

const catalogs = { en, vi } as unknown as Record<Locale, Record<string, string>>;

let current: Locale = "en";
const listeners = new Set<() => void>();

export function isLocale(value: unknown): value is Locale {
  return typeof value === "string" && (LOCALES as readonly string[]).includes(value);
}

export function getLocale(): Locale {
  return current;
}

/** Switches the language; every component using `useT` re-renders. */
export function setLocale(locale: Locale) {
  if (locale === current) return;
  current = locale;
  if (typeof document !== "undefined") document.documentElement.lang = locale;
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** The first of the browser's languages the server offers, else `fallback`. */
export function pickLocale(preferred: readonly string[], offered: readonly string[], fallback: string): Locale {
  for (const tag of preferred) {
    const lower = tag.toLowerCase();
    const found = offered.find((l) => l.toLowerCase() === lower) ?? offered.find((l) => l.toLowerCase() === lower.split("-")[0]);
    if (isLocale(found)) return found;
  }
  return isLocale(fallback) ? fallback : "en";
}

/** `template` with `{name}` replaced by `params.name` (text, once). */
export function interpolate(template: string, params?: Params): string {
  if (!params) return template;
  return template.replace(/\{(\w+)\}/g, (match, name: string) => (name in params ? String(params[name]) : match));
}

/** The message `key` in `locale`, English when missing. */
export function translate(locale: Locale, key: Key, params?: Params): string {
  const catalog = catalogs[locale];
  let name: string = key;
  if (params && typeof params.count === "number" && !(key in en)) {
    const form = new Intl.PluralRules(locale).select(params.count);
    name = `${key}_${form}` in catalog || `${key}_${form}` in en ? `${key}_${form}` : `${key}_other`;
  }
  return interpolate(catalog[name] ?? (en as Record<string, string>)[name] ?? name, params);
}

/** Translates in the current language (for code outside components). */
export function t(key: Key, params?: Params): string {
  return translate(current, key, params);
}

/** Whether `key` names a message (e.g. `issue.<code>` from the server). */
export function has(key: string): key is Key {
  return key in en || `${key}_other` in en;
}

/** `t` for components: they re-render when the language changes. */
export function useT(): typeof t {
  useSyncExternalStore(subscribe, getLocale, getLocale);
  return t;
}

/** The current language, for components. */
export function useLocale(): Locale {
  return useSyncExternalStore(subscribe, getLocale, getLocale);
}

/** A server validation issue in the current language, else its English message. */
export function issueText(issue: { code: string; params?: Record<string, string> } | undefined, fallback: string): string {
  if (!issue || issue.code === "custom") return fallback;
  const key = `issue.${issue.code}`;
  return has(key) ? t(key, issue.params) : fallback;
}

/** An instant in the current language and the browser's time zone. */
export function formatDateTime(date: Date): string {
  return new Intl.DateTimeFormat(current, { dateStyle: "medium", timeStyle: "short" }).format(date);
}

/** A whole number for display (counts, sizes); never for wire integers or decimals. */
export function formatCount(n: number): string {
  return new Intl.NumberFormat(current).format(n);
}

/**
 * Text kept in state and shown later: a function is called when rendered, so
 * the text follows a change of language (`useT` re-renders the component).
 */
export type Text = string | (() => string);

export function show(text: Text): string {
  return typeof text === "function" ? text() : text;
}

/** `key` translated when shown. */
export const later = (key: Key, params?: Params): Text => () => t(key, params);

/** The message of `error` (read when shown), else `fallback`. */
export const errorText = (error: unknown, fallback: Key): Text => () => (error instanceof Error ? error.message : t(fallback));
