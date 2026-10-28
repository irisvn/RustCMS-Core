// Session lifecycle in the browser: one place clears every trace of the signed-in
// user (query cache, CSRF token) on logout, 401, or a logout in another tab.
import type { QueryClient } from "@tanstack/react-query";
import { ApiClient } from "@/api/client";
import { type PanelConfig, safeRedirect } from "@/lib/config";
import { leaveOnPurpose } from "@/lib/leave";

/**
 * A keyed write whose outcome is unknown, kept in memory to be re-sent with the
 * same `Idempotency-Key` (the server then applies it at most once).
 */
export interface PendingWrite {
  /** Identity of this logical write. */
  id: string;
  /** The signed-in user who sent it: only they may re-send it. */
  user: string;
  method: string;
  path: string;
  /** Exactly the bytes sent the first time. */
  body: string;
  key: string;
  /** The form that sent it, if any: only that form continues after a retry. */
  origin?: string;
}

const CHANNEL = "rustcms-admin-session";

export interface Session {
  api: ApiClient;
  config: PanelConfig;
  /** Clears all user data; tells other tabs unless the signal came from one. */
  signedOut(opts?: { broadcast?: boolean }): void;
  /** After a successful login: rotated CSRF token, fresh caches, other tabs reload. */
  signedIn(): void;
  /** Where to go after login (`?redirect=`), validated. */
  afterLogin(redirect: string | null | undefined): string;
  /**
   * Records who is signed in. A different user than before (the cookie changed
   * under this tab) drops every cached record and draft by reloading the panel.
   */
  identify(userId: string): void;
  /** Whether a fresh CSRF token's user matches this tab (resets the tab when not). */
  verify(userId: string | null): boolean;
  /**
   * Resources with a write of unknown outcome (timeout, cut connection): writes
   * there stay paused until the page is reloaded, whatever the navigation.
   */
  markUncertain(slug: string, pending?: PendingWrite): void;
  isUncertain(slug: string): boolean;
  /** The write that paused `slug`, when it can be re-sent safely. */
  pendingWrite(slug: string): PendingWrite | undefined;
  /** Lifts the pause of `slug` if `id` is still the write that caused it. */
  resolved(slug: string, id: string): void;
  /** The signed-in user, once known. */
  user(): string | null;
  /**
   * The server rotated this session's credentials (password change, sign-out of
   * other sessions, own email change): start a new epoch here and in other tabs,
   * so answers to requests sent with the old cookie never sign the user out.
   */
  rotated(): void;
  /**
   * Runs a credential rotation: auth handling (401 → sign-in) is held while it
   * is in flight, since requests racing with it may see the old session gone.
   */
  rotating<T>(work: () => Promise<T>): Promise<T>;
  /** Subscribes to changes of the paused resources (for `useSyncExternalStore`). */
  subscribe(listener: () => void): () => void;
  /** Whether a response produced for `userId` may be used by this tab. */
  observe(userId: string): boolean;
}

export function createSession(
  config: PanelConfig,
  queryClient: QueryClient,
  toLogin: (redirect: string | null) => void,
): Session {
  const channel = typeof BroadcastChannel === "function" ? new BroadcastChannel(CHANNEL) : null;
  let redirecting = false;
  let userId: string | null = null;
  // Set when the cookie changed accounts under this tab: no write until the reload.
  let invalidated = false;
  const uncertain = new Set<string>();
  // Re-sendable writes by paused resource; never kept past a change of user.
  const pendingWrites = new Map<string, PendingWrite>();
  const listeners = new Set<() => void>();
  const notify = () => {
    for (const listener of listeners) listener();
  };

  const clear = () => {
    // A new session (sign-in, sign-out, 401, another account): nothing paused
    // or re-sendable carries over to it.
    if (pendingWrites.size || uncertain.size) {
      pendingWrites.clear();
      uncertain.clear();
      notify();
    }
    void queryClient.cancelQueries();
    queryClient.getMutationCache().clear();
    queryClient.clear();
    api.resetCsrf();
  };

  const currentPath = () => {
    const here = window.location.pathname + window.location.search;
    return safeRedirect(here, config.basePath) === here && !here.startsWith(`${config.basePath}/login`) ? here : null;
  };

  // Concurrent 401s navigate once.
  let rotations = 0;
  // A 401 seen while a rotation was in flight: checked again once it settles.
  let deferred = false;
  const settle = () => {
    if (rotations > 0 || !deferred) return;
    deferred = false;
    // Still signed in? A 401 here is handled normally (no rotation in flight).
    void api.request("GET", "/auth/me").catch(() => undefined);
  };
  const unauthenticated = () => {
    // A rotation in flight (here or in another tab): decide once it settles.
    if (rotations > 0) {
      deferred = true;
      return;
    }
    clear();
    userId = null;
    if (redirecting) return;
    redirecting = true;
    queueMicrotask(() => {
      toLogin(currentPath());
      redirecting = false;
    });
  };

  const api = new ApiClient({
    apiPath: config.apiPath,
    socketMaxBytes: config.realtime?.maxMessageBytes,
    onUnauthenticated: unauthenticated,
    verifyIdentity: (id) => session.verify(id),
    observeIdentity: (id) => session.observe(id),
    onSessionProbe: (me) => {
      const id = (me as { user?: { id?: unknown } } | null)?.user?.id;
      if (typeof id === "string") session.identify(id);
    },
  });

  const session: Session = {
    api,
    config,
    signedOut({ broadcast = true } = {}) {
      clear();
      userId = null;
      if (broadcast) channel?.postMessage("signed-out");
      toLogin(null);
    },
    signedIn() {
      clear();
      userId = null;
      channel?.postMessage("signed-in");
    },
    rotated() {
      api.resetCsrf();
      channel?.postMessage("rotated");
    },
    async rotating(work) {
      rotations += 1;
      api.resetCsrf();
      channel?.postMessage("rotation-start");
      try {
        return await work();
      } finally {
        api.resetCsrf();
        channel?.postMessage("rotation-end");
        rotations -= 1;
        settle();
      }
    },
    markUncertain(slug, pending) {
      uncertain.add(slug);
      // A newer unknown outcome replaces the older one (the server has both keys).
      if (pending && pending.user === userId) pendingWrites.set(slug, pending);
      else pendingWrites.delete(slug);
      notify();
    },
    pendingWrite(slug) {
      const pending = pendingWrites.get(slug);
      return pending && pending.user === userId ? pending : undefined;
    },
    resolved(slug, id) {
      if (pendingWrites.get(slug)?.id !== id) return;
      pendingWrites.delete(slug);
      uncertain.delete(slug);
      notify();
    },
    user() {
      return userId;
    },
    subscribe(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    observe(id) {
      if (invalidated) return false;
      if (userId === null) {
        userId = id;
        return true;
      }
      if (id === userId) return true;
      session.identify(id);
      return false;
    },
    isUncertain(slug) {
      return uncertain.has(slug);
    },
    verify(id) {
      if (invalidated) return false;
      // Signing in: no identity yet, any account is fine.
      if (userId === null) return true;
      if (id === null) {
        unauthenticated();
        return false;
      }
      if (id !== userId) {
        session.identify(id);
        return false;
      }
      return true;
    },
    identify(id) {
      if (invalidated) return;
      if (userId !== null && userId !== id) {
        invalidated = true;
        clear();
        leaveOnPurpose();
        window.location.assign(`${config.basePath}/`);
        return;
      }
      userId = id;
    },
    afterLogin(redirect) {
      const target = safeRedirect(redirect, config.basePath);
      if (!target || target.startsWith(`${config.basePath}/login`)) return "/";
      // Router paths are relative to the base path.
      return target.slice(config.basePath.length) || "/";
    },
  };

  const pending = new Set<ReturnType<typeof setTimeout>>();
  channel?.addEventListener("message", (event) => {
    if (event.data === "signed-out") session.signedOut({ broadcast: false });
    if (event.data === "rotated") api.resetCsrf();
    // Another tab rotates the shared session: hold auth handling here too (at
    // most 30 s, in case that tab closes before telling us it is done).
    if (event.data === "rotation-start") {
      rotations += 1;
      api.resetCsrf();
      const timer = setTimeout(() => {
        if (pending.delete(timer)) {
          rotations -= 1;
          settle();
        }
      }, 30_000);
      pending.add(timer);
    }
    if (event.data === "rotation-end") {
      const timer = pending.values().next().value;
      if (timer !== undefined && pending.delete(timer)) {
        clearTimeout(timer);
        rotations -= 1;
      }
      api.resetCsrf();
      settle();
    }
    // Another tab signed in (maybe as someone else): drop this tab's data.
    if (event.data === "signed-in") {
      clear();
      leaveOnPurpose();
      window.location.reload();
    }
  });

  return session;
}
