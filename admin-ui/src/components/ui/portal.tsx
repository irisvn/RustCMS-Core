import { createContext, useContext } from "react";

/**
 * Where menus and tooltips render: an open `<dialog>` (in the top layer, so
 * they stay above it and inside its interactive part) or `document.body`.
 */
export const PortalContainer = createContext<HTMLElement | null>(null);

export function usePortalContainer(): HTMLElement {
  return useContext(PortalContainer) ?? document.body;
}

const GAP = 4;
const MARGIN = 8;

/**
 * Places `floating` (`position: fixed`) under `anchor`, or above when there is
 * no room, kept inside the viewport. Per-property CSSOM writes only
 * (`style.setProperty`): the CSP's `style-src` governs style attributes and
 * elements, not these.
 */
export function place(anchor: HTMLElement, floating: HTMLElement, align: "start" | "end" | "center" = "start"): void {
  const a = anchor.getBoundingClientRect();
  const f = floating.getBoundingClientRect();
  const vw = document.documentElement.clientWidth || window.innerWidth;
  const vh = document.documentElement.clientHeight || window.innerHeight;
  let left = align === "end" ? a.right - f.width : align === "center" ? a.left + (a.width - f.width) / 2 : a.left;
  left = Math.min(Math.max(MARGIN, left), Math.max(MARGIN, vw - f.width - MARGIN));
  // Below, else above; when neither fits, the larger side, scrolled inside.
  const below = vh - MARGIN - (a.bottom + GAP);
  const above = a.top - GAP - MARGIN;
  const height = floating.scrollHeight || f.height;
  const under = height <= below || (height > above && below >= above);
  const room = Math.max(0, under ? below : above);
  const shown = Math.min(height, room);
  const top = under ? a.bottom + GAP : a.top - GAP - shown;
  floating.style.setProperty("max-height", `${Math.floor(room)}px`);
  floating.style.setProperty("left", `${Math.round(left)}px`);
  floating.style.setProperty("top", `${Math.round(top)}px`);
}

/** Re-places on scroll (any scroller) and resize while open; returns the cleanup. */
export function follow(update: () => void): () => void {
  update();
  window.addEventListener("scroll", update, true);
  window.addEventListener("resize", update);
  return () => {
    window.removeEventListener("scroll", update, true);
    window.removeEventListener("resize", update);
  };
}

const TABBABLE = 'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

/**
 * Moves focus to the tabbable element after (or before) `from` in `root`, as
 * Tab would: inside a modal dialog it wraps around; in the page it stops at
 * the ends (the browser's own controls are next).
 */
export function focusNextFrom(from: HTMLElement, root: ParentNode, backwards: boolean): void {
  const all = [...root.querySelectorAll<HTMLElement>(TABBABLE)].filter(
    (el) => el.tabIndex >= 0 && !el.closest("[inert]") && (el === from || (el.checkVisibility?.() ?? true)),
  );
  const index = all.indexOf(from);
  if (index < 0) return from.focus();
  let next = index + (backwards ? -1 : 1);
  if (root instanceof HTMLDialogElement) next = (next + all.length) % all.length;
  (all[next] ?? from).focus();
}
