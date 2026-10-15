import { keepPreviousData, queryOptions, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { t, useLocale } from "@/i18n";
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
import { ApiError, clientError, sessionReset } from "./client";
import type { PendingWrite, Session } from "@/app/session";
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
export function guardedWrite<T>(
  session: Session,
  slug: string,
  write: () => Promise<T>,
  pending?: Omit<PendingWrite, "user">,
): Promise<T> {
  // A queued write belongs to the session state it was submitted in.
  const epoch = session.api.epoch;
  const user = session.user();
  return enqueue(session, slug, () => {
    if (session.api.epoch !== epoch) throw sessionReset();
    return checkedWrite(session, slug, write, user, pending && user !== null ? { ...pending, user } : undefined);
  });
}

function enqueue<T>(session: Session, slug: string, run: () => Promise<T>): Promise<T> {
  let perSession = queues.get(session);
  if (!perSession) queues.set(session, (perSession = new Map()));
  const previous = perSession.get(slug) ?? Promise.resolve();
  const next = previous.catch(() => undefined).then(run);
  perSession.set(slug, next);
  return next;
}

async function checkedWrite<T>(
  session: Session,
  slug: string,
  write: () => Promise<T>,
  user: string | null,
  pending?: PendingWrite,
): Promise<T> {
  if (session.isUncertain(slug)) {
    throw clientError(409, "writes_paused", "error.pausedWrite");
  }
  try {
    return await write();
  } catch (e) {
    // An answer for another session (signed out, another user meanwhile) concerns
    // that session only: this one is neither paused nor handed the write.
    // (Credential rotations change the epoch, not the user: they still pause.)
    if (e instanceof ApiError && e.maybeApplied && session.user() === user) session.markUncertain(slug, pending);
    throw e;
  }
}

/**
 * A create or action sent with an `Idempotency-Key`: if its outcome is unknown,
 * it can be re-sent as is (see `retryPending`) and the server applies it once.
 */
export function keyedWrite<T>(
  session: Session,
  slug: string,
  method: string,
  path: string,
  body: unknown,
  origin?: string,
): Promise<T> {
  const text = JSON.stringify(body);
  const pending = { id: crypto.randomUUID(), method, path, body: text, key: crypto.randomUUID(), origin };
  return guardedWrite(session, slug, () => session.api.request<T>(method, path, JSON.parse(text), { "Idempotency-Key": pending.key }), pending);
}

/**
 * Re-sends the write that paused `slug` with its key, in the resource's queue.
 * The caller lifts the pause (`session.resolved`) once it has handled the
 * answer, so nothing can be submitted before then. Any failure leaves the
 * outcome unknown and the pause in place.
 */
export function retryPending(session: Session, slug: string): Promise<{ result: unknown; pending: PendingWrite }> {
  const pending = session.pendingWrite(slug);
  if (!pending) return Promise.reject(new Error(t("paused.nothing")));
  const send = () =>
    session.api.request<unknown>(pending.method, pending.path, JSON.parse(pending.body), { "Idempotency-Key": pending.key });
  return enqueue(session, slug, async () => {
    if (session.pendingWrite(slug)?.id !== pending.id) throw new Error(t("paused.handled"));
    try {
      return { result: await send(), pending };
    } catch (e) {
      // The CSRF token may have been rotated meanwhile: fetch a new one, once.
      if (!(e instanceof ApiError && e.code === "csrf_mismatch")) throw e;
      session.api.refreshCsrf();
      return { result: await send(), pending };
    }
  });
}

/** After any write: lists, records, schemas (abilities), relationship options and the audit log may have changed. */
function invalidateAfterWrite(client: ReturnType<typeof useQueryClient>, slug: string) {
  return Promise.all([
    client.invalidateQueries({ queryKey: keys.records(slug) }),
    client.invalidateQueries({ queryKey: ["schema", slug] }),
    client.invalidateQueries({ queryKey: ["options"] }),
    client.invalidateQueries({ queryKey: ["history", slug] }),
    client.invalidateQueries({ queryKey: ["audit"] }),
    // Custom pages and slots may show what changed.
    client.invalidateQueries({ queryKey: ["custom"] }),
  ]);
}

export const keys = {
  me: ["me"] as const,
  // Labels come translated: the language is part of the key, and the previous
  // answer stays on screen while the new one loads (forms keep their drafts).
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
  const locale = useLocale();
  return useQuery({
    queryKey: [...keys.panel, locale],
    queryFn: () => api.get<PanelResponse>("/panel"),
    placeholderData: keepPreviousData,
  });
}

export function useSchema(slug: string, operation: SchemaOperation, key?: string, opts: { enabled?: boolean } = {}) {
  const api = useApi();
  const locale = useLocale();
  return useQuery({
    queryKey: [...keys.schema(slug, operation, key), locale],
    queryFn: () =>
      api.get<ResourceSchema>(`/resources/${enc(slug)}/schema`, { operation, key }),
    enabled: opts.enabled ?? true,
    // Only the language changed: keep showing the same form meanwhile.
    placeholderData: (previous, query) => (query?.queryKey.slice(0, 4).join() === keys.schema(slug, operation, key).join() ? previous : undefined),
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

/** A record's relation: the parent, its key and the relation name. */
export interface Via {
  parent: string;
  record: string;
  relation: string;
}

/** Children of one record (a has-many relation), cached with the child resource's records. */
export function useRelationRecords(target: string, via: Via, query: URLSearchParams) {
  const api = useApi();
  const qs = query.toString();
  return useQuery({
    queryKey: [...keys.records(target), "relation", via.parent, via.record, via.relation, qs] as const,
    queryFn: () =>
      api.get<Page>(`/resources/${enc(via.parent)}/records/${enc(via.record)}/relations/${enc(via.relation)}`, new URLSearchParams(qs)),
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

export function useSaveRecord(slug: string, key?: string, origin?: string, via?: Via) {
  const api = useApi();
  const session = useSession();
  const client = useQueryClient();
  // Creates through a relation go to the parent's relation endpoint (the server sets the key).
  const createPath = via
    ? `/resources/${enc(via.parent)}/records/${enc(via.record)}/relations/${enc(via.relation)}`
    : `/resources/${enc(slug)}/records`;
  return useMutation({
    mutationFn: (data: Data) =>
      (key === undefined
        ? keyedWrite<DataResponse<Envelope>>(session, slug, "POST", createPath, data, origin)
        : guardedWrite(session, slug, () => api.patch<DataResponse<Envelope>>(`/resources/${enc(slug)}/records/${enc(key)}`, data))
      ).then((r) => r.data),
    // Only after a successful save: a rejected one must not refetch under the user's draft.
    onSuccess: async () => {
      await invalidateAfterWrite(client, slug);
      if (via) await client.invalidateQueries({ queryKey: keys.records(via.parent) });
    },
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

/** Restores a trashed record (soft deletes). */
export function useRestoreRecord(slug: string) {
  const api = useApi();
  const session = useSession();
  const client = useQueryClient();
  return useMutation({
    mutationFn: (key: string) =>
      guardedWrite(session, slug, () => api.post<DataResponse<Envelope>>(`/resources/${enc(slug)}/records/${enc(key)}/restore`, {})),
    onSettled: () => invalidateAfterWrite(client, slug),
  });
}

/** Deletes a trashed record for good (soft deletes). */
export function useForceDeleteRecord(slug: string) {
  const api = useApi();
  const session = useSession();
  const client = useQueryClient();
  return useMutation({
    mutationFn: (key: string) =>
      guardedWrite(session, slug, () => api.delete<void>(`/resources/${enc(slug)}/records/${enc(key)}/force`)),
    onSettled: () => invalidateAfterWrite(client, slug),
  });
}

export function useRunAction(slug: string) {
  const session = useSession();
  const client = useQueryClient();
  return useMutation({
    mutationFn: ({ action, keys: recordKeys, data }: { action: string; keys: string[]; data?: Data }) =>
      keyedWrite<ActionOutcome>(session, slug, "POST", `/resources/${enc(slug)}/actions/${enc(action)}`, { keys: recordKeys, data: data ?? {} }),
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
