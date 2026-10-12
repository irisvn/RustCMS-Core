// Session lifecycle in the browser: one place clears every trace of the signed-in
// user (query cache, CSRF token) on logout, 401, or a logout in another tab.
import type { QueryClient } from "@tanstack/react-query";
import { ApiClient } from "@/api/client";
import { type PanelConfig, safeRedirect } from "@/lib/config";

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
  markUncertain(slug: string): void;
  isUncertain(slug: string): boolean;
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
  const listeners = new Set<() => void>();

  const clear = () => {
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
  const unauthenticated = () => {
    // A rotation in flight: the next request after it settles tells for sure.
    if (rotations > 0) return;
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
      session.rotated();
      try {
        return await work();
      } finally {
        session.rotated();
        rotations -= 1;
      }
    },
    markUncertain(slug) {
      uncertain.add(slug);
      for (const listener of listeners) listener();
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

  channel?.addEventListener("message", (event) => {
    if (event.data === "signed-out") session.signedOut({ broadcast: false });
    if (event.data === "rotated") api.resetCsrf();
    // Another tab signed in (maybe as someone else): drop this tab's data.
    if (event.data === "signed-in") {
      clear();
      window.location.reload();
    }
  });

  return session;
}
