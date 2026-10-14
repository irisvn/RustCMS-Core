// Where the panel's language comes from: the signed-in user's choice (saved on
// the server, `me.locale`), before sign-in the browser's languages or the last
// choice made on this device. Other tabs follow a change at once.

import { LOCALES, type Locale, isLocale, pickLocale, setLocale } from "./index";

const STORAGE = "rustcms-admin-locale";
const CHANNEL = "rustcms-admin-locale";

const channel = typeof BroadcastChannel === "function" ? new BroadcastChannel(CHANNEL) : null;
/** Set while a choice is being saved: an older `me` must not switch back. */
let choosing = 0;
/** When the last choice was made (here or in another tab): older answers of `/me` are ignored. */
let choiceAt = 0;
const followers = new Set<() => void>();

channel?.addEventListener("message", (event: MessageEvent<unknown>) => {
  if (!isLocale(event.data)) return;
  choiceAt = Date.now();
  setLocale(event.data);
  for (const follow of followers) follow();
});

/** Called when another tab chose a language (e.g. to refetch `/me`). */
export function onOtherTabChoice(follow: () => void): () => void {
  followers.add(follow);
  return () => {
    followers.delete(follow);
  };
}

function stored(): string | null {
  try {
    return localStorage.getItem(STORAGE);
  } catch {
    return null;
  }
}

/** The language to start with, before the user is known. */
export function initialLocale(offered: readonly string[] = LOCALES, fallback = "en"): Locale {
  const saved = stored();
  const browser = typeof navigator === "undefined" ? [] : navigator.languages ?? [navigator.language];
  return pickLocale(saved ? [saved, ...browser] : browser, offered, fallback);
}

/**
 * Applies the language the server chose for the signed-in user (`/me`
 * fetched at `fetchedAt`), unless a choice was made since or is being saved.
 */
export function serverLocale(locale: string | undefined, fetchedAt: number) {
  if (choosing === 0 && fetchedAt >= choiceAt && isLocale(locale)) setLocale(locale);
}

/**
 * The user picks a language, remembered on this device and in other tabs.
 * Signed in, `save` stores it on the account first: the server then answers
 * in that language, so nothing fetched for it can be in the old one.
 */
export async function chooseLocale(locale: Locale, save?: (locale: Locale) => Promise<unknown>) {
  choosing += 1;
  try {
    await save?.(locale);
    choiceAt = Date.now();
    setLocale(locale);
    try {
      localStorage.setItem(STORAGE, locale);
    } catch {
      // Private mode: the choice lasts for this page.
    }
    channel?.postMessage(locale);
  } finally {
    choosing -= 1;
  }
}
