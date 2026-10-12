import { createContext, useCallback, useContext, useSyncExternalStore } from "react";
import type { Session } from "@/app/session";
import type { PanelConfig } from "@/lib/config";
import type { ApiClient } from "./client";

export const SessionContext = createContext<Session | null>(null);

export function useSession(): Session {
  const ctx = useContext(SessionContext);
  if (!ctx) throw new Error("SessionContext missing");
  return ctx;
}

export const useApi = (): ApiClient => useSession().api;
export const usePanelConfig = (): PanelConfig => useSession().config;

/** Whether writes to `slug` are paused after one with an unknown outcome (until reload). */
export function useUncertain(slug: string): boolean {
  const session = useSession();
  const read = useCallback(() => session.isUncertain(slug), [session, slug]);
  return useSyncExternalStore(session.subscribe, read, read);
}
