// Auto refresh of the Queue and Schedules pages: every few seconds while the
// tab is visible, nothing blocks it (a dialog, older pages loaded) and the
// user keeps it on (kept per browser).
import { useCallback, useState, useSyncExternalStore } from "react";

export const REFRESH_MS = 5_000;
const KEY = "rustcms.autoRefresh";

function stored(): boolean {
  try {
    return window.localStorage.getItem(KEY) !== "off";
  } catch {
    return true;
  }
}

/** The user's choice (on by default), kept in this browser when it can be. */
export function useAutoRefresh(): [boolean, (on: boolean) => void] {
  const [on, setOn] = useState(stored);
  const choose = useCallback((next: boolean) => {
    setOn(next);
    try {
      window.localStorage.setItem(KEY, next ? "on" : "off");
    } catch {
      // Blocked storage: this page only.
    }
  }, []);
  return [on, choose];
}

function subscribe(change: () => void) {
  document.addEventListener("visibilitychange", change);
  return () => document.removeEventListener("visibilitychange", change);
}

/** The tab is shown. */
export function useVisible(): boolean {
  return useSyncExternalStore(subscribe, () => document.visibilityState !== "hidden", () => true);
}

/** The `refetchInterval` of a polled query: only while nothing holds it back. */
export function interval(on: boolean, visible: boolean, held: boolean): number | false {
  return on && visible && !held ? REFRESH_MS : false;
}
