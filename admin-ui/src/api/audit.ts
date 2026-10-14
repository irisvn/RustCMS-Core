// The audit log (`/api/audit`) and record histories (`…/records/{key}/history`).
import { useInfiniteQuery } from "@tanstack/react-query";
import { useApi } from "./context";
import type { AuditPage } from "./types";

const enc = encodeURIComponent;

/** Filters of the audit log page (all optional). */
export interface AuditFilters {
  event?: string;
  subject_type?: string;
  subject_key?: string;
  actor?: string;
  from?: string;
  to?: string;
}

const clean = (filters: Record<string, string | undefined>) =>
  Object.fromEntries(Object.entries(filters).filter((e): e is [string, string] => !!e[1]));

export function useAuditLog(filters: AuditFilters) {
  const api = useApi();
  return useInfiniteQuery({
    queryKey: ["audit", clean({ ...filters })],
    queryFn: ({ pageParam }) => api.get<AuditPage>("/audit", clean({ ...filters, cursor: pageParam })),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (last) => last.meta.nextCursor,
  });
}

export function useHistory(slug: string, key: string, enabled: boolean) {
  const api = useApi();
  return useInfiniteQuery({
    queryKey: ["history", slug, key],
    queryFn: ({ pageParam }) =>
      api.get<AuditPage>(`/resources/${enc(slug)}/records/${enc(key)}/history`, clean({ cursor: pageParam })),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (last) => last.meta.nextCursor,
    enabled,
  });
}
