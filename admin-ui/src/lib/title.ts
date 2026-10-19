// `props.title` of v2 pages (ADR 0032, ui 2): the heading and breadcrumb a
// page sets for one of its sub-paths. One page is shown at a time: the store
// holds at most one entry, tied to the view that set it, its sub-path and the
// session epoch; anything else shows the server's title.

import { useSyncExternalStore } from "react";

interface Entry {
  owner: object;
  slug: string;
  path: string;
  epoch: number;
  /** `null`: no visible heading (the page draws its own). */
  title: string | null;
}

const MAX_TITLE = 200;
let entry: Entry | null = null;
const listeners = new Set<() => void>();
const emit = () => {
  for (const listener of listeners) listener();
};

export function setPageTitle(owner: object, slug: string, path: string, epoch: number, title: unknown): void {
  const value = title === null ? null : String(title).slice(0, MAX_TITLE);
  entry = { owner, slug, path, epoch, title: value };
  emit();
}

/** Forgets `owner`'s title (its view unmounted). */
export function clearPageTitle(owner: object): void {
  if (entry?.owner === owner) {
    entry = null;
    emit();
  }
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** The title set for this page and sub-path in this epoch: `undefined` when none. */
export function usePageTitle(slug: string, path: string, epoch: number): string | null | undefined {
  const current = useSyncExternalStore(subscribe, () => entry);
  return current && current.slug === slug && current.path === path && current.epoch === epoch ? current.title : undefined;
}
