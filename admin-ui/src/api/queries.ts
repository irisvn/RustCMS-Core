import { keepPreviousData, queryOptions, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { ApiClient } from "./client";
import type {
  ActionOutcome,
  Data,
  DataResponse,
  Envelope,
  Me,
  Page,
  PanelResponse,
  ResourceSchema,
  SchemaOperation,
  SelectOption,
} from "./types";
import { ApiError, sessionReset } from "./client";
import type { Session } from "@/app/session";
import { useApi, useSession } from "./context";

const enc = encodeURIComponent;

/**
 * Every write to a resource goes through here: refused while the resource is
 * paused, and pausing it when the outcome is unknown, even if the page that
 * started it is gone by then.
 */
const queues = new WeakMap<Session, Map<string, Promise<unknown>>>();

/**
 * Writes to one resource run one at a time, so the pause check below always
 * sees the outcome of the previous write before the next one is sent.
 */
function guardedWrite<T>(session: Session, slug: string, write: () => Promise<T>): Promise<T> {
  let perSession = queues.get(session);
  if (!perSession) queues.set(session, (perSession = new Map()));
  const previous = perSession.get(slug) ?? Promise.resolve();
  // A queued write belongs to the session state it was submitted in.
  const epoch = session.api.epoch;
  const next = previous
    .catch(() => undefined)
    .then(() => {
      if (session.api.epoch !== epoch) throw sessionReset();
      return checkedWrite(session, slug, write);
    });
  perSession.set(slug, next);
  return next;
}

async function checkedWrite<T>(session: Session, slug: string, write: () => Promise<T>): Promise<T> {
  if (session.isUncertain(slug)) {
    throw new ApiError(409, {
      code: "writes_paused",
      message: "An earlier change here may or may not have been applied. Reload the page and check before trying again.",
    });
  }
  try {
    return await write();
  } catch (e) {
    if (e instanceof ApiError && e.maybeApplied) session.markUncertain(slug);
    throw e;
  }
}

/** After any write: lists, records, schemas (abilities) and relationship options may have changed. */
function invalidateAfterWrite(client: ReturnType<typeof useQueryClient>, slug: string) {
  return Promise.all([
    client.invalidateQueries({ queryKey: keys.records(slug) }),
    client.invalidateQueries({ queryKey: ["schema", slug] }),
    client.invalidateQueries({ queryKey: ["options"] }),
  ]);
}

export const keys = {
  me: ["me"] as const,
  panel: ["panel"] as const,
  schema: (slug: string, operation: SchemaOperation, key?: string) => ["schema", slug, operation, key ?? null] as const,
  list: (slug: string, query: string) => ["records", slug, "list", query] as const,
  record: (slug: string, key: string) => ["records", slug, "one", key] as const,
  records: (slug: string) => ["records", slug] as const,
};

export const meQuery = (api: ApiClient) =>
  queryOptions({ queryKey: keys.me, queryFn: () => api.get<Me>("/auth/me"), retry: false, staleTime: 30_000 });

export function usePanel() {
  const api = useApi();
  return useQuery({ queryKey: keys.panel, queryFn: () => api.get<PanelResponse>("/panel") });
}

export function useSchema(slug: string, operation: SchemaOperation, key?: string) {
  const api = useApi();
  return useQuery({
    queryKey: keys.schema(slug, operation, key),
    queryFn: () =>
      api.get<ResourceSchema>(`/resources/${enc(slug)}/schema`, { operation, key }),
  });
}

/** `query` is the list query string (page, per_page, sort, search, filter[f][op]). */
export function useRecords(slug: string, query: URLSearchParams) {
  const api = useApi();
  const qs = query.toString();
  return useQuery({
    queryKey: keys.list(slug, qs),
    queryFn: () => api.get<Page>(`/resources/${enc(slug)}/records`, new URLSearchParams(qs)),
    placeholderData: keepPreviousData,
  });
}

export function useRecord(slug: string, key: string, opts: { enabled?: boolean } = {}) {
  const api = useApi();
  return useQuery({
    enabled: opts.enabled ?? true,
    queryKey: keys.record(slug, key),
    queryFn: () => api.get<DataResponse<Envelope>>(`/resources/${enc(slug)}/records/${enc(key)}`).then((r) => r.data),
  });
}

export function useSaveRecord(slug: string, key?: string) {
  const api = useApi();
  const session = useSession();
  const client = useQueryClient();
  return useMutation({
    mutationFn: (data: Data) => guardedWrite(session, slug, () =>
      (key === undefined
        ? api.post<DataResponse<Envelope>>(`/resources/${enc(slug)}/records`, data)
        : api.patch<DataResponse<Envelope>>(`/resources/${enc(slug)}/records/${enc(key)}`, data)
      ).then((r) => r.data)),
    // Only after a successful save: a rejected one must not refetch under the user's draft.
    onSuccess: () => invalidateAfterWrite(client, slug),
  });
}

export function useDeleteRecord(slug: string) {
  const api = useApi();
  const session = useSession();
  const client = useQueryClient();
  return useMutation({
    mutationFn: (key: string) =>
      guardedWrite(session, slug, () => api.delete<void>(`/resources/${enc(slug)}/records/${enc(key)}`)),
    onSettled: () => invalidateAfterWrite(client, slug),
  });
}

export function useRunAction(slug: string) {
  const api = useApi();
  const session = useSession();
  const client = useQueryClient();
  return useMutation({
    mutationFn: ({ action, keys: recordKeys, data }: { action: string; keys: string[]; data?: Data }) =>
      guardedWrite(session, slug, () =>
        api.post<ActionOutcome>(`/resources/${enc(slug)}/actions/${enc(action)}`, { keys: recordKeys, data: data ?? {} }),
      ),
    onSettled: () => invalidateAfterWrite(client, slug),
  });
}

/** Options of a relationship field; `record` is the edited/viewed record (server authorizes on it). */
export function fetchOptions(
  api: ApiClient,
  slug: string,
  field: string,
  params: { search?: string; keys?: string[]; record?: string },
) {
  return api
    .get<DataResponse<SelectOption[]>>(`/resources/${enc(slug)}/fields/${enc(field)}/options`, {
      search: params.search,
      keys: params.keys?.length ? params.keys.join(",") : undefined,
      record: params.record,
    })
    .then((r) => r.data);
}
