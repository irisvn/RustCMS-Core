// Toasts of v2 views (`props.toast`, ADR 0032): one store for the panel,
// each toast tied to the session epoch it was made in. The region drops them
// all when the epoch changes, so no message crosses sessions.

export type ToastKind = "success" | "error";

export interface Toast {
  id: number;
  kind: ToastKind;
  text: string;
  epoch: number;
}

/** At most this many at once (the oldest goes first). */
export const MAX_TOASTS = 5;
const MAX_TEXT = 500;

let toasts: readonly Toast[] = [];
let next = 1;
const listeners = new Set<() => void>();
const emit = () => {
  for (const listener of listeners) listener();
};

export function pushToast(kind: ToastKind, text: unknown, epoch: number): void {
  const toast = { id: next++, kind, text: String(text).slice(0, MAX_TEXT), epoch };
  toasts = [...toasts, toast].slice(-MAX_TOASTS);
  emit();
}

export function dismissToast(id: number): void {
  toasts = toasts.filter((t) => t.id !== id);
  emit();
}

/** Every toast not of `epoch` goes (all of them without one). */
export function clearToasts(epoch?: number): void {
  const kept = epoch === undefined ? [] : toasts.filter((t) => t.epoch === epoch);
  if (kept.length !== toasts.length) {
    toasts = kept;
    emit();
  }
}

export function subscribeToasts(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export const currentToasts = () => toasts;
