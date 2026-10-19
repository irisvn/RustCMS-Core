import { useCallback, useSyncExternalStore } from "react";
import type { ApiClient } from "@/api/client";

/** The session epoch, re-rendering when it changes. */
export function useEpoch(api: ApiClient): number {
  const subscribe = useCallback((listener: () => void) => api.onEpochChange(listener), [api]);
  return useSyncExternalStore(subscribe, () => api.epoch);
}
