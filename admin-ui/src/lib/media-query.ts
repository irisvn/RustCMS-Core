import { useSyncExternalStore } from "react";

/** Narrower than the `sm` breakpoint (40rem): a phone. */
export const PHONE = "(max-width: 39.99rem)";

/**
 * Whether `query` matches now; re-renders when that changes. False where the
 * browser has no media queries (the wide layout).
 */
export function useMediaQuery(query: string): boolean {
  return useSyncExternalStore(
    (change) => {
      if (typeof window.matchMedia !== "function") return () => {};
      const list = window.matchMedia(query);
      list.addEventListener("change", change);
      return () => list.removeEventListener("change", change);
    },
    () => typeof window.matchMedia === "function" && window.matchMedia(query).matches,
    () => false,
  );
}
